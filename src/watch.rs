use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use ignore::gitignore::Gitignore;
use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use rusqlite::Connection;
use tokio::sync::mpsc;

use crate::graph;
use crate::index;
use crate::index::languages;
use crate::lock::RepoLock;

const QARTEZIGNORE_FILENAME: &str = ".qartezignore";

/// Default writer chunk size — matches the Allium spec and acceptance.rs.
pub const DEFAULT_WRITER_CHUNK_SIZE: usize = 50;

/// Debounce window: events arriving within this interval after the first
/// event in a batch are folded into the same re-index cycle.
const DEBOUNCE_MS: u64 = 500;

/// Source of the database connection for a watcher, allowing either a file
/// path (production), a pre-opened in-memory connection (tests), or the
/// legacy shared `Arc<Mutex<Connection>>` (in-memory test fallback).
enum DbSource {
    /// Production: open a dedicated connection to the on-disk database.
    /// Used when the server was started with a file-backed database.
    Path(PathBuf),
    /// Shared connection: wraps `Connection` in `Arc<Mutex>` so that
    /// `Watcher: Sync` and `&Watcher: Send` (required by `tokio::spawn`).
    ///
    /// Two concrete callers:
    /// - Tests that construct a `Watcher` via `new_with_connection` or
    ///   `with_prefix_with_connection` (single-owner, no contention).
    /// - The legacy fallback path in `attach_watcher` where the server
    ///   passes `self.db_arc()` (shared connection, in-memory test DBs).
    Arc(Arc<Mutex<Connection>>),
}

/// A batch of filesystem events, separated into changed (created/modified)
/// and deleted paths so the incremental indexer can handle them differently.
struct WatchBatch {
    changed: Vec<PathBuf>,
    deleted: Vec<PathBuf>,
}

pub struct Watcher {
    db: DbSource,
    project_root: PathBuf,
    /// Path prefix to prepend to each file's relative path when writing
    /// index rows. Must match the prefix `full_index_multi` used for this
    /// root (empty in single-root mode). Without it, incremental rows in
    /// multi-root projects would orphan the original full-index rows.
    path_prefix: String,
    /// Directory hosting the cross-process index lock file. When set,
    /// `reindex` acquires the lock with a short deadline and skips with a
    /// log message if another qartez process holds it. When `None`, the
    /// watcher writes without coordination (used by tests that drive
    /// indexing through an in-memory connection only).
    lock_dir: Option<PathBuf>,
    /// Maximum number of file changes to commit in a single DB transaction.
    /// Larger batches are split into chunks of this size, each committed
    /// separately with a yield point between chunks so reader tasks can
    /// make progress. Default: 50.
    writer_chunk_size: usize,
}

impl Watcher {
    /// Production constructor: the watcher will open its own dedicated
    /// connection to `db_path` on every re-index cycle.
    pub fn new(db_path: PathBuf, project_root: PathBuf) -> Self {
        Self::with_prefix(db_path, project_root, String::new())
    }

    /// Production constructor with a path prefix.
    pub fn with_prefix(db_path: PathBuf, project_root: PathBuf, path_prefix: String) -> Self {
        Self::with_prefix_with_chunk_size(db_path, project_root, path_prefix, None)
    }

    /// Test / CLI constructor: use a caller-supplied connection, wrapped
    /// in `Arc<Mutex>` so the watcher is `Sync`.
    pub fn new_with_connection(conn: Connection, project_root: PathBuf) -> Self {
        Self::with_prefix_with_connection(conn, project_root, String::new(), None)
    }

    /// Full production constructor.
    pub fn with_prefix_with_chunk_size(
        db_path: PathBuf,
        project_root: PathBuf,
        path_prefix: String,
        writer_chunk_size: Option<usize>,
    ) -> Self {
        Self {
            db: DbSource::Path(db_path),
            project_root,
            path_prefix,
            lock_dir: None,
            writer_chunk_size: writer_chunk_size.unwrap_or(DEFAULT_WRITER_CHUNK_SIZE),
        }
    }

    /// Full test / CLI constructor with a caller-supplied connection.
    /// The connection is wrapped in `Arc<Mutex>` so the watcher is `Sync`
    /// and can be passed to `tokio::spawn`.
    pub fn with_prefix_with_connection(
        conn: Connection,
        project_root: PathBuf,
        path_prefix: String,
        writer_chunk_size: Option<usize>,
    ) -> Self {
        Self {
            db: DbSource::Arc(Arc::new(Mutex::new(conn))),
            project_root,
            path_prefix,
            lock_dir: None,
            writer_chunk_size: writer_chunk_size.unwrap_or(DEFAULT_WRITER_CHUNK_SIZE),
        }
    }

    /// Fallback constructor for in-memory test databases where the caller
    /// already has an `Arc<Mutex<Connection>>`.  Uses `Arc<Mutex>` sharing
    /// (the legacy pattern), which is fine for tests but causes lock
    /// contention in production.  Prefer `new` or `new_with_connection`
    /// whenever possible.
    pub fn with_prefix_with_arc(
        db: Arc<Mutex<Connection>>,
        project_root: PathBuf,
        path_prefix: String,
        writer_chunk_size: Option<usize>,
    ) -> Self {
        Self {
            db: DbSource::Arc(db),
            project_root,
            path_prefix,
            lock_dir: None,
            writer_chunk_size: writer_chunk_size.unwrap_or(DEFAULT_WRITER_CHUNK_SIZE),
        }
    }

    /// Set the directory hosting the cross-process index lock. The watcher
    /// will acquire the lock briefly before each re-index and skip the
    /// cycle if another qartez process is already writing.
    pub fn with_lock_dir(mut self, lock_dir: PathBuf) -> Self {
        self.lock_dir = Some(lock_dir);
        self
    }

    /// Set the maximum number of file changes to commit per DB transaction
    /// during watcher-driven incremental reindexing. Large batches are split
    /// into chunks of this size with a yield point between chunks so that
    /// reader tasks can make progress. Default: 50.
    pub fn with_chunk_size(mut self, chunk_size: usize) -> Self {
        self.writer_chunk_size = chunk_size;
        self
    }

    /// Obtain a database connection for the current re-index cycle.
    ///
    /// * If the watcher was created with `new_with_connection` or the
    ///   `with_prefix_with_connection` variant, the existing connection is
    ///   returned (test path).
    /// * If the watcher was created with `new` or the `with_prefix` variant,
    ///   a fresh connection to the on-disk database is opened (production
    ///   path).  This completely eliminates contention with the server's
    ///   shared connection.
    fn get_conn(&self) -> anyhow::Result<ConnectionAdapter> {
        match &self.db {
            DbSource::Path(path) => {
                let conn = crate::storage::open_db(path)?;
                Ok(ConnectionAdapter::Owned(conn))
            }
            DbSource::Arc(arc) => {
                let guard = arc.lock().expect("watcher DB mutex poisoned");
                Ok(ConnectionAdapter::LockGuard(guard))
            }
        }
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        let supported_ext: HashSet<&str> = languages::supported_extensions().into_iter().collect();
        let supported_names: HashSet<&str> = languages::supported_filenames().into_iter().collect();
        let supported_prefixes: Vec<&str> = languages::supported_prefixes();

        let (tx, mut rx) = mpsc::channel::<WatchBatch>(64);

        let project_root = self.project_root.clone();
        let _watcher = start_notify_watcher(
            project_root.clone(),
            supported_ext,
            supported_names,
            supported_prefixes,
            tx,
        )?;

        tracing::info!("file watcher active on {}", self.project_root.display());

        loop {
            let batch = match rx.recv().await {
                Some(b) => b,
                None => break,
            };

            let mut changed = batch.changed;
            let mut deleted = batch.deleted;

            // Debounce: drain any additional events that arrive within the window.
            while let Ok(Some(more)) =
                tokio::time::timeout(Duration::from_millis(DEBOUNCE_MS), rx.recv()).await
            {
                changed.extend(more.changed);
                deleted.extend(more.deleted);
            }

            changed.sort();
            changed.dedup();
            deleted.sort();
            deleted.dedup();
            // A file that was deleted then re-created within the same batch
            // should only appear in `changed`.
            deleted.retain(|p| !changed.contains(p));

            let total = changed.len() + deleted.len();
            tracing::info!(
                "watcher: {total} events ({} changed, {} deleted), re-indexing",
                changed.len(),
                deleted.len(),
            );

            if let Err(e) = self.reindex(&changed, &deleted) {
                tracing::error!("re-index after watch event failed: {e}");
            }
        }

        Ok(())
    }

    fn reindex(&self, changed: &[PathBuf], deleted: &[PathBuf]) -> anyhow::Result<()> {
        // Acquire the cross-process lock briefly. If another qartez process
        // is in the middle of a full index, skip this cycle rather than
        // pile up watcher events behind a multi-second writer. The next
        // file save will retry, and `incremental_index` is idempotent over
        // the actual on-disk state, so missing one cycle does not lose
        // information - it just defers the index update.
        let _index_lock = if let Some(dir) = self.lock_dir.as_ref() {
            match RepoLock::try_acquire_briefly(dir) {
                Ok(Some(g)) => Some(g),
                Ok(None) => {
                    tracing::info!(
                        "watcher: another qartez process is indexing; skipping this batch"
                    );
                    return Ok(());
                }
                Err(e) => {
                    tracing::warn!("watcher: lock IO error, proceeding without lock: {e}");
                    None
                }
            }
        } else {
            None
        };

        // Open a dedicated connection for this re-index batch, or re-use
        // the caller-supplied in-memory connection (test path).
        let adapter = match self.get_conn() {
            Ok(a) => a,
            Err(e) => {
                tracing::error!(
                    "watcher: failed to open dedicated connection, skipping batch: {e}"
                );
                return Ok(());
            }
        };
        let conn = adapter.as_ref();

        // Set writer_state to IncrementalIndexing before batch.
        if let Err(e) = crate::storage::write::set_writer_state(
            conn,
            crate::readiness::WriterState::IncrementalIndexing,
        ) {
            tracing::warn!("watcher: failed to set writer_state to IncrementalIndexing: {e}");
        }
        let result = (|| {
            index::incremental_index_with_prefix_chunked(
                conn,
                &self.project_root,
                &self.path_prefix,
                changed,
                deleted,
                self.writer_chunk_size,
            )?;
            graph::pagerank::compute_pagerank(&conn, &Default::default())?;
            graph::pagerank::compute_symbol_pagerank(&conn, &Default::default())?;
            Ok::<(), anyhow::Error>(())
        })();

        // Reset writer_state to Idle after batch completes (success or failure).
        if let Err(e) =
            crate::storage::write::set_writer_state(&conn, crate::readiness::WriterState::Idle)
        {
            tracing::warn!("watcher: failed to reset writer_state to Idle: {e}");
        }
        result
    }
}

/// Adapter that lets `reindex` work with either an owned `Connection`
/// (production: freshly opened per batch) or a `MutexGuard` to a shared
/// connection (test / legacy fallback path).
enum ConnectionAdapter<'a> {
    Owned(Connection),
    LockGuard(MutexGuard<'a, Connection>),
}

impl<'a> AsRef<Connection> for ConnectionAdapter<'a> {
    fn as_ref(&self) -> &Connection {
        match self {
            ConnectionAdapter::Owned(c) => c,
            ConnectionAdapter::LockGuard(g) => g,
        }
    }
}

fn load_qartezignore(root: &Path) -> Gitignore {
    let ignore_path = root.join(QARTEZIGNORE_FILENAME);
    if ignore_path.exists() {
        let (gi, err) = Gitignore::new(&ignore_path);
        if let Some(e) = err {
            tracing::warn!(path = %ignore_path.display(), error = %e, "partial parse of .qartezignore");
        }
        gi
    } else {
        Gitignore::empty()
    }
}

/// Hot-reload wrapper for `.qartezignore`. Holds the parsed matcher together
/// with the mtime that was observed when it was loaded, so the closure can
/// refresh the cache after the user edits the ignore file during a live
/// watcher session (rather than requiring a full restart).
struct QartezIgnoreCache {
    gi: Gitignore,
    mtime: Option<SystemTime>,
}

impl QartezIgnoreCache {
    fn new(root: &Path) -> Self {
        Self {
            gi: load_qartezignore(root),
            mtime: fs_mtime(&root.join(QARTEZIGNORE_FILENAME)),
        }
    }

    fn refresh_if_changed(&mut self, root: &Path) {
        let current = fs_mtime(&root.join(QARTEZIGNORE_FILENAME));
        if current != self.mtime {
            self.gi = load_qartezignore(root);
            self.mtime = current;
        }
    }
}

fn fs_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

fn start_notify_watcher(
    project_root: PathBuf,
    extensions: HashSet<&'static str>,
    filenames: HashSet<&'static str>,
    prefixes: Vec<&'static str>,
    tx: mpsc::Sender<WatchBatch>,
) -> anyhow::Result<RecommendedWatcher> {
    let mut gitignore_cache = QartezIgnoreCache::new(&project_root);

    let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        let event = match res {
            Ok(e) => e,
            Err(err) => {
                tracing::warn!("watch error: {err}");
                return;
            }
        };

        // Refresh `.qartezignore` whenever any file event is observed in
        // the project root.  This keeps the ignore cache fresh without a
        // dedicated timer.
        gitignore_cache.refresh_if_changed(&project_root);
        let local_gitignore = Gitignore::empty();
        let qartezignore = &gitignore_cache.gi;

        let paths: Vec<PathBuf> = event
            .paths
            .into_iter()
            .filter(|p| {
                // Skip paths that are ignored by either .gitignore or .qartezignore
                let is_gitignored = local_gitignore.matched(p, false).is_ignore();
                let is_qartezignored = qartezignore.matched(p, false).is_ignore();
                !(is_gitignored || is_qartezignored)
            })
            .collect();

        let (mut changed, mut deleted) = (Vec::new(), Vec::new());
        match event.kind {
            EventKind::Create(_)
            | EventKind::Modify(ModifyKind::Data(_))
            | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                for p in paths {
                    if is_interesting_path(&p, &extensions, &filenames, &prefixes) {
                        changed.push(p);
                    }
                }
            }
            EventKind::Remove(_) => {
                for p in paths {
                    if is_interesting_path(&p, &extensions, &filenames, &prefixes) {
                        deleted.push(p);
                    }
                }
            }
            EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                // Renaming away counts as a deletion.
                for p in paths {
                    if is_interesting_path(&p, &extensions, &filenames, &prefixes) {
                        deleted.push(p);
                    }
                }
            }
            _ => {}
        }

        if !changed.is_empty() || !deleted.is_empty() {
            let batch = WatchBatch { changed, deleted };
            if let Err(e) = tx.try_send(batch) {
                tracing::warn!("watch event dropped, channel full: {e}");
            }
        }
    })?;

    Ok(watcher)
}

fn is_interesting_path(
    path: &Path,
    extensions: &HashSet<&str>,
    filenames: &HashSet<&str>,
    prefixes: &[&str],
) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    let name = path.file_name().and_then(|n| n.to_str());
    let stem = path.file_stem().and_then(|s| s.to_str());

    let by_extension = ext.is_some_and(|e| extensions.contains(e));
    let by_filename = name.is_some_and(|n| filenames.contains(n));
    let by_prefix = stem.is_some_and(|s| {
        prefixes.iter().any(|prefix| s.starts_with(prefix))
    });

    by_extension || by_filename || by_prefix
}
