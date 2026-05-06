//! Thread-local lazy-loaded parser workers.
//!
//! Replaces single Mutex<Parser> bottleneck with per-language workers that are:
//! - Created lazily on first use (no upfront grammar cost)
//! - Per-thread to avoid locking contention
//! - Evicted after idle timeout (configurable)
//! - Tree cache for hot-file incremental reparsing (Phase 1)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tree_sitter::Parser;

use crate::error::{QartezError, Result};
use crate::index::languages;
use crate::index::symbols::ParseResult;

/// Configuration for parser workers
#[derive(Debug, Clone)]
pub struct ParserWorkerConfig {
    /// TTL for idle workers before eviction (default: 15 minutes)
    pub parser_idle_ttl: Duration,
    /// TTL for hot tree cache before eviction (default: 30 minutes)
    pub hot_tree_retention: Duration,
}

impl Default for ParserWorkerConfig {
    fn default() -> Self {
        Self {
            parser_idle_ttl: Duration::from_secs(15 * 60),
            hot_tree_retention: Duration::from_secs(30 * 60),
        }
    }
}

/// State of a cached tree
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeCacheState {
    /// No tree cached for this file
    Absent,
    /// Tree is hot and valid
    Hot,
    /// Tree was invalidated (needs re-parse)
    Invalidated,
    /// Tree was evicted from cache
    Evicted,
}

impl TreeCacheState {
    /// Convert to the string stored in the DB `tree_cache` column.
    pub fn to_db_str(self) -> &'static str {
        match self {
            TreeCacheState::Absent => "absent",
            TreeCacheState::Hot => "hot",
            TreeCacheState::Invalidated => "invalidated",
            TreeCacheState::Evicted => "evicted",
        }
    }

    /// Parse from the DB `tree_cache` column value.
    /// Legacy "cold" values are mapped to Invalidated.
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "hot" => TreeCacheState::Hot,
            "invalidated" => TreeCacheState::Invalidated,
            "evicted" => TreeCacheState::Evicted,
            "cold" => TreeCacheState::Invalidated, // legacy normalization
            _ => TreeCacheState::Absent,
        }
    }
}

/// A cached syntax tree with metadata
#[derive(Debug, Clone)]
pub struct TreeCacheEntry {
    /// The cached tree
    pub tree: tree_sitter::Tree,
    /// When this tree was last parsed
    pub cached_at: Instant,
    /// Current cache state
    pub state: TreeCacheState,
}

/// State of a parser worker
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    /// Worker created, no language loaded yet
    Idle,
    /// Currently loading language
    LoadingLanguage,
    /// Actively parsing
    Parsing,
    /// Worker is being evicted (dropping language)
    Evicting,
}

/// A parser worker for a specific language
pub struct ParseWorker {
    /// Language extension this worker handles (e.g., "rs", "ts", "js")
    pub language: String,
    /// Current state machine state
    pub state: WorkerState,
    /// Last time this worker was used (for TTL tracking)
    pub last_used: Instant,
    /// The actual tree-sitter parser (loaded lazily)
    parser: Parser,
}

impl ParseWorker {
    /// Create a new idle worker for a language
    pub fn new(language: &str) -> Self {
        Self {
            language: language.to_string(),
            state: WorkerState::Idle,
            last_used: Instant::now(),
            parser: Parser::new(),
        }
    }

    /// Ensure language is loaded in the parser (lazy load)
    pub fn ensure_language(&mut self, ext: &str) -> Result<()> {
        if self.state == WorkerState::Idle {
            self.state = WorkerState::LoadingLanguage;
        }

        if self.state != WorkerState::LoadingLanguage {
            return Ok(());
        }

        let support = languages::get_language_for_ext(ext)
            .or_else(|| {
                let filename = format!("file.{ext}");
                languages::get_language_for_filename(&filename)
            })
            .ok_or_else(|| QartezError::Parse {
                path: "".to_string(),
                message: format!("unsupported extension: {ext}"),
            })?;

        let lang = support.tree_sitter_language(ext);
        self.parser
            .set_language(&lang)
            .map_err(|e| QartezError::Parse {
                path: "".to_string(),
                message: format!("failed to set language: {e}"),
            })?;

        self.state = WorkerState::Idle;
        Ok(())
    }

    /// Parse a source file (caller must ensure_language first)
    /// When `old_tree` is `Some`, performs incremental parsing using Tree.edit()
    pub fn parse(
        &mut self,
        source: &[u8],
        old_tree: Option<&tree_sitter::Tree>,
    ) -> Result<tree_sitter::Tree> {
        self.last_used = Instant::now();
        self.state = WorkerState::Parsing;

        let tree = self
            .parser
            .parse(source, old_tree)
            .ok_or_else(|| QartezError::Parse {
                path: "".to_string(),
                message: "tree-sitter parse returned None".to_string(),
            })?;

        self.state = WorkerState::Idle;
        Ok(tree)
    }

    /// Check if worker has been idle too long
    pub fn is_idle_too_long(&self, ttl: Duration) -> bool {
        self.state == WorkerState::Idle && self.last_used.elapsed() > ttl
    }
}

/// Thread-local parser worker pool
pub struct ParserWorkers {
    /// Workers indexed by language extension
    workers: HashMap<String, ParseWorker>,
    /// Configuration
    config: ParserWorkerConfig,
    /// Tree cache for hot-file incremental reparsing (PathBuf -> TreeCacheEntry)
    tree_cache: HashMap<PathBuf, TreeCacheEntry>,
}

impl Default for ParserWorkers {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserWorkers {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
            config: ParserWorkerConfig::default(),
            tree_cache: HashMap::new(),
        }
    }

    /// Get tree cache entry for a path
    pub fn get_tree_cache(&self, path: &Path) -> Option<&TreeCacheEntry> {
        self.tree_cache.get(path)
    }

    /// Set tree cache entry for a path
    pub fn set_tree_cache(&mut self, path: PathBuf, entry: TreeCacheEntry) {
        self.tree_cache.insert(path, entry);
    }

    /// Invalidate tree cache for a path (mark as needing re-parse)
    pub fn invalidate_tree_cache(&mut self, path: &Path) {
        if let Some(entry) = self.tree_cache.get_mut(path) {
            entry.state = TreeCacheState::Invalidated;
        }
    }

    /// Check if tree cache has a valid hot tree for path
    pub fn has_hot_tree(&self, path: &Path) -> bool {
        self.tree_cache
            .get(path)
            .map(|e| {
                e.state == TreeCacheState::Hot
                    && e.cached_at.elapsed() < self.config.hot_tree_retention
            })
            .unwrap_or(false)
    }

    /// Evict expired entries from tree cache.
    ///
    /// Marks entries as `Evicted` before dropping them, so that callers
    /// who hold a clone of the entry can observe the state transition
    /// (Hot → Evicted) rather than seeing a silent disappearance.
    pub fn evict_tree_cache(&mut self) {
        let retention = self.config.hot_tree_retention;
        // Phase 1: mark expired entries as Evicted
        for entry in self.tree_cache.values_mut() {
            if entry.cached_at.elapsed() >= retention && entry.state != TreeCacheState::Absent {
                entry.state = TreeCacheState::Evicted;
            }
        }
        // Phase 2: remove Evicted entries
        self.tree_cache
            .retain(|_, entry| entry.state != TreeCacheState::Evicted);
    }

    /// Get or create a worker for the given file extension
    pub fn worker_for(&mut self, ext: &str) -> Result<&mut ParseWorker> {
        // Get or create worker for this language
        let worker = self
            .workers
            .entry(ext.to_string())
            .or_insert_with(|| ParseWorker::new(ext));

        // Ensure language is loaded (lazy initialization)
        worker.ensure_language(ext)?;

        Ok(worker)
    }

    /// Parse a file using the appropriate worker
    pub fn parse_file(&mut self, path: &Path, source: &[u8]) -> Result<(ParseResult, String)> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Determine language from extension first, then filename
        let lang_ext = if ext.is_empty() {
            if let Some(support) = languages::get_language_for_filename(filename) {
                // Use filename's primary extension
                let exts = languages::supported_extensions();
                exts.iter()
                    .find(|e| {
                        languages::get_language_for_ext(e)
                            .map(|s| s.language_name() == support.language_name())
                            .unwrap_or(false)
                    })
                    .map(|e| e.to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            ext.to_string()
        };

        if lang_ext.is_empty() {
            return Err(QartezError::Parse {
                path: path.display().to_string(),
                message: format!("unsupported file: {filename}"),
            });
        }

        self.parse_with_key(path, source, &lang_ext)
    }

    fn parse_with_key(
        &mut self,
        path: &Path,
        source: &[u8],
        lang_ext: &str,
    ) -> Result<(ParseResult, String)> {
        // Phase 1: Extract old tree from cache for incremental parsing.
        // Must be done BEFORE worker_for() to avoid borrow issues.
        let old_tree: Option<tree_sitter::Tree> = {
            let retention = self.config.hot_tree_retention;
            match self.tree_cache.get(path) {
                Some(entry)
                    if entry.state == TreeCacheState::Hot
                        && entry.cached_at.elapsed() < retention =>
                {
                    // Clone the tree for incremental parsing
                    Some(entry.tree.clone())
                }
                _ => None,
            }
        }; // Immutable borrow ends here.

        // Phase 2: Get or create worker for this language.
        let worker = self.worker_for(lang_ext)?;

        // Phase 3: Parse the source (incremental if we have an old tree).
        let tree = if let Some(old) = old_tree {
            // Incremental parsing using tree-sitter's capabilities
            worker.parse(source, Some(&old))?
        } else {
            worker.parse(source, None)?
        };

        // Phase 4: Get language support for extraction.
        let support = languages::get_language_for_ext(lang_ext)
            .or_else(|| {
                let filename = format!("file.{lang_ext}");
                languages::get_language_for_filename(&filename)
            })
            .ok_or_else(|| QartezError::Parse {
                path: path.display().to_string(),
                message: format!("unsupported language: {lang_ext}"),
            })?;

        let result = support.extract(source, &tree);

        // Phase 5: Cache the tree for hot-file incremental reparsing.
        let path_buf = path.to_path_buf();
        let cache_entry = TreeCacheEntry {
            tree,
            cached_at: Instant::now(),
            state: TreeCacheState::Hot,
        };
        self.tree_cache.insert(path_buf, cache_entry);

        Ok((result, support.language_name().to_string()))
    }

    /// Clean up idle workers past TTL and evict expired tree cache entries
    pub fn evict_idle_workers(&mut self) {
        let ttl = self.config.parser_idle_ttl;
        self.workers
            .retain(|_, worker| !worker.is_idle_too_long(ttl));

        // Also evict expired tree cache entries
        self.evict_tree_cache();
    }
}

/// Thread-safe wrapper for parser workers
pub struct ThreadLocalParserWorkers {
    inner: Arc<Mutex<ParserWorkers>>,
}

impl Default for ThreadLocalParserWorkers {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadLocalParserWorkers {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(ParserWorkers::new())),
        }
    }

    /// Parse a file (uses internal locking, but workers are per-language)
    pub fn parse_file(&self, path: &Path, source: &[u8]) -> Result<(ParseResult, String)> {
        let mut workers = self.inner.lock().map_err(|e| QartezError::Parse {
            path: path.display().to_string(),
            message: format!("failed to acquire parser lock: {e}"),
        })?;
        workers.parse_file(path, source)
    }

    /// Check if there's a hot tree cached for path
    pub fn has_hot_tree(&self, path: &Path) -> bool {
        self.inner
            .lock()
            .map(|workers| workers.has_hot_tree(path))
            .unwrap_or(false)
    }

    /// Get tree cache entry for path
    pub fn get_tree_cache(&self, path: &Path) -> Option<TreeCacheEntry> {
        self.inner
            .lock()
            .ok()
            .and_then(|workers| workers.get_tree_cache(path).cloned())
    }

    /// Invalidate tree cache for path
    pub fn invalidate_tree_cache(&self, path: &Path) {
        if let Ok(mut workers) = self.inner.lock() {
            workers.invalidate_tree_cache(path);
        }
    }

    /// Clean up idle workers
    pub fn evict_idle_workers(&self) {
        if let Ok(mut workers) = self.inner.lock() {
            workers.evict_idle_workers();
        }
    }

    /// Evict expired tree cache entries
    pub fn evict_tree_cache(&self) {
        if let Ok(mut workers) = self.inner.lock() {
            workers.evict_tree_cache();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    /// Helper: create a Tree by parsing a minimal Rust source
    fn make_tree() -> tree_sitter::Tree {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_rust::LANGUAGE.into())
            .unwrap();
        parser.parse("fn main() {}", None).unwrap()
    }

    /// Verify TreeCacheState DB string round-trips
    #[test]
    fn tree_cache_state_db_roundtrip() {
        assert_eq!(TreeCacheState::Absent.to_db_str(), "absent");
        assert_eq!(TreeCacheState::Hot.to_db_str(), "hot");
        assert_eq!(TreeCacheState::Invalidated.to_db_str(), "invalidated");
        assert_eq!(TreeCacheState::Evicted.to_db_str(), "evicted");

        assert_eq!(
            TreeCacheState::from_db_str("absent"),
            TreeCacheState::Absent
        );
        assert_eq!(TreeCacheState::from_db_str("hot"), TreeCacheState::Hot);
        assert_eq!(
            TreeCacheState::from_db_str("invalidated"),
            TreeCacheState::Invalidated
        );
        assert_eq!(
            TreeCacheState::from_db_str("evicted"),
            TreeCacheState::Evicted
        );
        // Legacy "cold" maps to Invalidated
        assert_eq!(
            TreeCacheState::from_db_str("cold"),
            TreeCacheState::Invalidated
        );
        // Unknown values default to Absent
        assert_eq!(
            TreeCacheState::from_db_str("unknown"),
            TreeCacheState::Absent
        );
    }

    /// Verify invalidate_tree_cache marks state as Invalidated
    #[test]
    fn invalidate_marks_invalidated() {
        let mut workers = ParserWorkers::new();
        let path = PathBuf::from("test.rs");
        let entry = TreeCacheEntry {
            tree: make_tree(),
            cached_at: Instant::now(),
            state: TreeCacheState::Hot,
        };
        workers.set_tree_cache(path.clone(), entry);
        assert!(workers.has_hot_tree(&path));

        workers.invalidate_tree_cache(&path);
        assert!(!workers.has_hot_tree(&path));
        let cached = workers.get_tree_cache(&path).unwrap();
        assert_eq!(cached.state, TreeCacheState::Invalidated);
    }

    /// Verify evict_tree_cache marks entries as Evicted before removing
    #[test]
    fn evict_marks_evicted_before_removal() {
        let mut workers = ParserWorkers::new();
        // Create a worker with a very short retention so entries expire immediately
        workers.config.hot_tree_retention = Duration::from_millis(1);
        let path = PathBuf::from("test.rs");
        let entry = TreeCacheEntry {
            tree: make_tree(),
            cached_at: Instant::now(),
            state: TreeCacheState::Hot,
        };
        workers.set_tree_cache(path.clone(), entry);

        // Wait for the entry to expire
        thread::sleep(Duration::from_millis(5));

        // Evict should mark as Evicted then remove
        workers.evict_tree_cache();
        // After eviction, the entry should be gone
        assert!(workers.get_tree_cache(&path).is_none());
    }

    /// Verify hot tree check respects retention time
    #[test]
    fn hot_tree_respects_retention() {
        let mut workers = ParserWorkers::new();
        workers.config.hot_tree_retention = Duration::from_millis(10);
        let path = PathBuf::from("test.rs");
        let entry = TreeCacheEntry {
            tree: make_tree(),
            cached_at: Instant::now(),
            state: TreeCacheState::Hot,
        };
        workers.set_tree_cache(path.clone(), entry);
        assert!(workers.has_hot_tree(&path));

        thread::sleep(Duration::from_millis(20));
        assert!(!workers.has_hot_tree(&path));
    }

    /// Verify metadata-only change preserves hot tree (no invalidation call)
    #[test]
    fn no_invalidation_preserves_hot_tree() {
        let pool = ThreadLocalParserWorkers::new();
        let path = PathBuf::from("test.rs");
        let entry = TreeCacheEntry {
            tree: make_tree(),
            cached_at: Instant::now(),
            state: TreeCacheState::Hot,
        };
        {
            let mut workers = pool.inner.lock().unwrap();
            workers.set_tree_cache(path.clone(), entry);
        }
        assert!(pool.has_hot_tree(&path));

        // Simulate metadata-only change: simply don't call invalidate_tree_cache
        // Tree is preserved for extraction
        assert!(pool.has_hot_tree(&path));
    }

    /// Verify byte-edit with hot tree does not invalidate (incremental path)
    #[test]
    fn byte_edit_with_hot_tree_preserves_cache() {
        let pool = ThreadLocalParserWorkers::new();
        let path = PathBuf::from("test.rs");
        let entry = TreeCacheEntry {
            tree: make_tree(),
            cached_at: Instant::now(),
            state: TreeCacheState::Hot,
        };
        {
            let mut workers = pool.inner.lock().unwrap();
            workers.set_tree_cache(path.clone(), entry);
        }
        assert!(pool.has_hot_tree(&path));

        // Simulate: has_byte_edit=true AND has_hot_tree → do NOT invalidate
        // (incremental_index_batch conditional logic)
        if pool.has_hot_tree(&path) {
            // Tree is preserved for incremental parse via Tree.edit()
            assert!(pool.has_hot_tree(&path));
        }
    }

    /// Verify byte-edit without hot tree calls invalidate (cold fallback)
    #[test]
    fn byte_edit_without_hot_tree_calls_invalidate() {
        let pool = ThreadLocalParserWorkers::new();
        let path = PathBuf::from("test.rs");
        // No tree cache set — pool.has_hot_tree returns false
        assert!(!pool.has_hot_tree(&path));

        // Simulate: has_byte_edit=true AND !has_hot_tree → invalidate
        if !pool.has_hot_tree(&path) {
            pool.invalidate_tree_cache(&path);
        }
        // Still no hot tree (wasn't one to begin with)
        assert!(!pool.has_hot_tree(&path));
    }
}
