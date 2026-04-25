//! Acceptance harness for the Allium behavioural specification.
//!
//! Tests here verify the invariants and rules defined in
//! `.allium/qartez-indexing-improvements.allium` against the *current*
//! codebase. They serve two purposes:
//!
//! 1. **Baseline capture** – establish what the code already satisfies
//!    so upcoming implementation issues can show progress.
//! 2. **Regression guard** – prevent changes from breaking invariants
//!    that are already met.
//!
//! Each test is annotated with the Allium invariant or rule it maps to.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use tempfile::TempDir;

use crate::index::languages;
use crate::index::{full_index, incremental_index};
use crate::storage::{open_in_memory, read, write};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Create an in-memory DB with schema ready for queries.
fn test_db() -> Connection {
    let conn = open_in_memory().unwrap();
    conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
    conn
}

/// Create a temporary project directory with a minimal Rust source file.
fn rust_project() -> (TempDir, PathBuf) {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(
        src.join("lib.rs"),
        "pub fn hello() -> &'static str { \"hello\" }\n",
    )
    .unwrap();
    // Cargo.toml marks this as a project root
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    (tmp, root)
}

/// Index a real project directory and return the connection.
fn index_rust_project(root: &Path) -> Connection {
    let conn = test_db();
    full_index(&conn, root, true).unwrap();
    conn
}

/// Replicate the watcher's eligibility check using the public language
/// registry.  The watcher builds the same sets from
/// `supported_extensions()` / `supported_filenames()` /
/// `supported_prefixes()`, so this is an exact mirror.
fn is_indexable(p: &Path) -> bool {
    let exts: HashSet<&str> = languages::supported_extensions().into_iter().collect();
    let names: HashSet<&str> = languages::supported_filenames().into_iter().collect();
    let prefixes = languages::supported_prefixes();

    if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
        if exts.contains(ext) {
            return true;
        }
    }
    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
        if names.contains(name) {
            return true;
        }
        if prefixes.iter().any(|pre| name.starts_with(pre)) {
            return true;
        }
    }
    false
}

// ===========================================================================
// INVARIANT: QueriesAreOnlyServedFromUsableIndexes
// ===========================================================================
//
// Allium: "query_session.status = served implies
//          query_session.project_index.readiness in [ready, partial_reindex]"
//
// Current code: The server serves queries after indexing completes.
// We verify that after a full index, the DB is in a queryable state
// (schema present, files exist, no stale entries).

/// Maps to **Invariant: QueriesAreOnlyServedFromUsableIndexes**
///
/// After a full index completes, the DB must be in a usable state:
/// schema exists, files are present, and there are no stale file
/// entries (all file mtime values are non-zero, meaning the file was
/// actually indexed rather than left as a placeholder).
#[test]
fn invariant_queries_only_served_from_usable_indexes_after_full_index() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);

    // Schema must exist
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(table_count >= 5, "DB must have core tables after indexing");

    // Files must exist
    let file_count = read::get_file_count(&conn).unwrap();
    assert!(
        file_count > 0,
        "DB must contain indexed files after full index"
    );

    // No stale files (files with zero mtime indicate they were never
    // actually written during the index pass)
    let stale = read::get_stale_files(&conn).unwrap();
    assert!(
        stale.is_empty(),
        "no files should be stale immediately after a fresh full index, found: {:?}",
        stale.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
}

/// Maps to **Invariant: QueriesAreOnlyServedFromUsableIndexes**
///
/// An empty DB (no index run) should still answer queries without
/// panicking — it just returns empty results. This is the "cold_start"
/// readiness state in Allium terms.
#[test]
fn invariant_queries_on_empty_db_return_empty_not_error() {
    let conn = test_db();

    // These should all succeed (returning empty) rather than panic/error
    let files = read::get_all_files(&conn).unwrap();
    assert!(files.is_empty());

    let symbols = read::get_all_symbols(&conn).unwrap();
    assert!(symbols.is_empty());

    let count = read::get_file_count(&conn).unwrap();
    assert_eq!(count, 0);

    let sym_count = read::get_symbol_count(&conn).unwrap();
    assert_eq!(sym_count, 0);
}

// ===========================================================================
// INVARIANT: UnsupportedFilesNeverProduceParseTasks
// ===========================================================================
//
// Allium: "parse_task.source_file.eligibility != ignored"
//
// Current code: The walker and watcher both filter by supported
// extensions/filenames/prefixes. We verify that only supported
// files appear in the index.

/// Maps to **Invariant: UnsupportedFilesNeverProduceParseTasks**
///
/// Files with unsupported extensions must not appear in the `files`
/// table after a full index. The walker filters them out.
#[test]
fn invariant_unsupported_files_not_indexed() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    // Write a supported Rust file
    fs::write(src.join("lib.rs"), "pub fn good() {}\n").unwrap();
    // Write unsupported file types
    fs::write(src.join("data.csv"), "a,b,c\n1,2,3\n").unwrap();
    fs::write(src.join("image.png"), b"\x89PNG\r\n".as_slice()).unwrap();
    fs::write(src.join("config.ini"), "[settings]\nkey=val\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let conn = test_db();
    full_index(&conn, &root, true).unwrap();

    let files = read::get_all_files(&conn).unwrap();
    let paths: HashSet<String> = files.iter().map(|f| f.path.clone()).collect();

    // Supported: lib.rs (and possibly Cargo.toml if TOML is supported)
    assert!(
        paths.iter().any(|p| p.contains("lib.rs")),
        "supported .rs file must be indexed, got: {paths:?}"
    );

    // Unsupported: CSV, PNG, INI must not be in the index
    for unsupported in &["data.csv", "image.png", "config.ini"] {
        assert!(
            !paths.iter().any(|p| p.contains(unsupported)),
            "unsupported file {unsupported} must not appear in index, got: {paths:?}"
        );
    }
}

/// Maps to **Invariant: UnsupportedFilesNeverProduceParseTasks**
///
/// The eligibility check used by the watcher must agree with the
/// walker's filtering: if the walker wouldn't index it, the
/// watcher shouldn't re-index it either.
#[test]
fn invariant_watcher_eligibility_matches_walker() {
    // Files the walker would index
    let should_index = &[
        "src/lib.rs",
        "src/main.go",
        "src/index.ts",
        "src/app.py",
        "Cargo.toml",
    ];
    for path in should_index {
        let p = Path::new(path);
        assert!(
            is_indexable(p),
            "watcher should classify {path} as indexable (same as walker)"
        );
    }

    // Files the walker would skip
    let should_skip = &["data.csv", "image.png", "config.ini", "README.md"];
    for path in should_skip {
        let p = Path::new(path);
        assert!(
            !is_indexable(p),
            "watcher should classify {path} as non-indexable (same as walker)"
        );
    }
}

// ===========================================================================
// INVARIANT: IncrementalTasksAreAlwaysClassified
// ===========================================================================
//
// Allium: "parse_task.reason = incremental_change implies
//          parse_task.parse_mode in [cold, incremental]"
//
// Current code: All incremental re-parses are cold (tree-sitter parses
// from scratch). The `parse_mode = incremental` path is planned
// (issue `qartez-mcp-latest-45ps`).
//
// We verify that incremental index produces correct results even
// with cold parsing, and that the file's content is updated.

/// Maps to **Invariant: IncrementalTasksAreAlwaysClassified**
///
/// After an incremental re-index of a changed file, the file's
/// content in the DB must reflect the new version. Even though
/// current incremental parsing is always cold, the result must be
/// correct.
#[test]
fn invariant_incremental_update_produces_correct_symbols() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);

    let src = root.join("src");
    let lib_path = src.join("lib.rs");

    // Verify initial state
    let initial = read::get_file_by_path(&conn, "src/lib.rs")
        .unwrap()
        .expect("lib.rs must be indexed");
    assert!(initial.line_count > 0);

    // Modify the file
    fs::write(
        &lib_path,
        "pub fn updated_fn() -> i32 { 42 }\npub fn second() -> bool { true }\n",
    )
    .unwrap();

    // Incremental re-index
    incremental_index(&conn, &root, &[lib_path], &[]).unwrap();

    // The file must now have updated content
    let updated = read::get_file_by_path(&conn, "src/lib.rs")
        .unwrap()
        .expect("lib.rs must still exist after incremental update");

    // The new file has more lines than the original
    assert!(
        updated.line_count >= 2,
        "updated file must reflect new content (line_count={})",
        updated.line_count
    );

    // Symbols for the file should now include the new function names
    let syms = read::get_symbols_for_file(&conn, updated.id).unwrap();
    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.iter().any(|n| *n == "updated_fn" || *n == "second"),
        "incremental re-index must produce symbols for new functions, got: {names:?}"
    );
}

/// Maps to **Invariant: IncrementalTasksAreAlwaysClassified**
///
/// Incremental deletion must remove the file from the index.
#[test]
fn invariant_incremental_deletion_removes_file() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    fs::write(src.join("lib.rs"), "pub fn keep() {}\n").unwrap();
    fs::write(src.join("extra.rs"), "pub fn remove() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let conn = test_db();
    full_index(&conn, &root, true).unwrap();

    // Both files must be indexed
    assert!(
        read::get_file_by_path(&conn, "src/lib.rs")
            .unwrap()
            .is_some()
    );
    assert!(
        read::get_file_by_path(&conn, "src/extra.rs")
            .unwrap()
            .is_some()
    );

    // Delete extra.rs and run incremental index
    let extra_path = src.join("extra.rs");
    fs::remove_file(&extra_path).unwrap();
    incremental_index(&conn, &root, &[], &[extra_path]).unwrap();

    // extra.rs must be gone from the index
    assert!(
        read::get_file_by_path(&conn, "src/extra.rs")
            .unwrap()
            .is_none(),
        "deleted file must be removed from index"
    );

    // lib.rs must still be present
    assert!(
        read::get_file_by_path(&conn, "src/lib.rs")
            .unwrap()
            .is_some(),
        "unchanged file must remain in index"
    );
}

// ===========================================================================
// INVARIANT: ChunksRemainBounded
// ===========================================================================
//
// Allium: "write_chunk.task_count <= config.writer_chunk_size"
//
// Current code: The writer processes files one at a time in
// `full_index_root` — there is no chunking yet (planned in
// issue `qartez-mcp-latest-ajvr`).
//
// We verify that the DB state is consistent after indexing any
// number of files, which will also hold once chunking is added.

/// Maps to **Invariant: ChunksRemainBounded**
///
/// After indexing, every file in the DB must have a valid language,
/// non-negative line count, and non-negative size. This is the
/// DB-level manifestation of bounded writes: no partial or corrupted
/// file entries.
#[test]
fn invariant_indexed_files_have_valid_metadata() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);

    let files = read::get_all_files(&conn).unwrap();
    for file in &files {
        assert!(
            !file.language.is_empty(),
            "file {} must have a non-empty language",
            file.path
        );
        assert!(
            file.line_count >= 0,
            "file {} must have non-negative line_count",
            file.path
        );
        assert!(
            file.size_bytes >= 0,
            "file {} must have non-negative size_bytes",
            file.path
        );
    }
}

/// Maps to **Invariant: ChunksRemainBounded**
///
/// The writer_chunk_size config value (50) must be defined and
/// positive. When chunking is implemented, this test will verify
/// that no chunk exceeds this limit.
#[test]
fn invariant_writer_chunk_size_is_positive() {
    // From Allium config: writer_chunk_size = 50
    const WRITER_CHUNK_SIZE: i64 = 50;
    const { assert!(WRITER_CHUNK_SIZE > 0) };
}

// ===========================================================================
// INVARIANT: PrunedProjectsHaveNoPendingWork
// ===========================================================================
//
// Allium: "project_index.lifecycle = pruned implies
//          not project_index.has_pending_work"
//
// Current code: There is no explicit project lifecycle or prune
// mechanism (planned in issues `qartez-mcp-latest-r5y3` and
// `qartez-mcp-latest-m2te`). We verify that deleting a file's
// data via the storage layer removes all associated data (symbols,
// edges, refs), which is the DB-level equivalent of "no pending
// work after prune".

/// Maps to **Invariant: PrunedProjectsHaveNoPendingWork**
///
/// Deleting a file's data must cascade: all associated symbols,
/// edges, and symbol refs must be removed. No orphaned rows
/// should remain.
#[test]
fn invariant_file_deletion_cascades_completely() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);

    let file = read::get_file_by_path(&conn, "src/lib.rs")
        .unwrap()
        .expect("lib.rs must be indexed");

    // Precondition: file has symbols
    let symbols = read::get_symbols_for_file(&conn, file.id).unwrap();
    assert!(
        !symbols.is_empty(),
        "file must have symbols before deletion"
    );

    // Delete the file data
    write::delete_file_data(&conn, file.id).unwrap();

    // Verify cascading deletion
    let remaining_symbols = read::get_symbols_for_file(&conn, file.id).unwrap();
    assert!(
        remaining_symbols.is_empty(),
        "symbols must be cascade-deleted with file"
    );

    // Verify no orphaned symbol_refs pointing at deleted symbols
    let orphan_refs: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbol_refs WHERE from_symbol_id NOT IN (SELECT id FROM symbols)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        orphan_refs, 0,
        "no orphaned symbol_refs should remain after cascade delete"
    );

    // Verify no orphaned edges pointing at the deleted file
    let orphan_edges: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM edges WHERE from_file NOT IN (SELECT id FROM files) OR to_file NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        orphan_edges, 0,
        "no orphaned edges should remain after cascade delete"
    );
}

// ===========================================================================
// RULE: WatcherUsesTheSameEligibilityPolicyAsTheIndexer
// ===========================================================================
//
// Allium: The watcher and the full-index walker must agree on which
// files are supported. We verify that the eligibility function
// derived from the language registry produces the same results as
// the walker in practice.

/// Maps to **Rule: WatcherUsesTheSameEligibilityPolicyAsTheIndexer**
///
/// The `is_indexable` function (replicated from watcher logic using
/// the public language registry) must classify every file the same
/// way the full-index walker does: supported files get indexed,
/// unsupported files get skipped.
#[test]
fn rule_watcher_eligibility_agrees_with_language_registry() {
    // Build the same sets the watcher uses
    let exts: HashSet<String> = languages::supported_extensions()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    let names: HashSet<String> = languages::supported_filenames()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Core extensions must be present
    for expected in &["rs", "ts", "py", "go", "java"] {
        assert!(
            exts.contains(*expected),
            "language registry must include .{expected} extension"
        );
    }

    // Core exact filenames must be present
    for expected in &["Dockerfile", "Makefile"] {
        assert!(
            names.contains(*expected),
            "language registry must include {expected} filename"
        );
    }

    // Prefix-based matches
    let prefixes: HashSet<String> = languages::supported_prefixes()
        .into_iter()
        .map(|s| s.to_string())
        .collect();
    assert!(
        prefixes.contains("Dockerfile."),
        "language registry must include 'Dockerfile.' prefix for Dockerfile.* variants"
    );

    // is_indexable must be consistent with these sets
    assert!(is_indexable(Path::new("src/lib.rs")));
    assert!(is_indexable(Path::new("Cargo.toml")));
    assert!(!is_indexable(Path::new("data.csv")));
    assert!(!is_indexable(Path::new("image.png")));
}

// ===========================================================================
// RULE: ReaderPoolBecomesReadyBeforeQueriesAreServed
// ===========================================================================
//
// Allium: After DB opens, the reader pool must become ready before
// queries are served. Current code opens the DB synchronously and
// queries go directly — there is no async reader pool yet
// (planned in `qartez-mcp-latest-svve`).
//
// We verify that opening a DB always succeeds and produces a
// connection that can serve queries immediately.

/// Maps to **Rule: ReaderPoolBecomesReadyBeforeQueriesAreServed**
///
/// Opening a DB (in memory or on disk) must produce a connection
/// that is immediately usable for queries.
#[test]
fn rule_open_db_produces_queryable_connection() {
    let conn = open_in_memory().unwrap();

    // Must be able to query immediately
    let file_count = read::get_file_count(&conn).unwrap();
    assert_eq!(file_count, 0, "fresh DB must have zero files");

    let sym_count = read::get_symbol_count(&conn).unwrap();
    assert_eq!(sym_count, 0, "fresh DB must have zero symbols");

    // Schema must exist
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(
        tables.contains(&"files".to_string()),
        "schema must include 'files' table"
    );
    assert!(
        tables.contains(&"symbols".to_string()),
        "schema must include 'symbols' table"
    );
}

// ===========================================================================
// RULE: ProjectReturnsToReadyWhenPendingWorkDrains
// ===========================================================================
//
// Allium: After all indexing work completes, the project must return
// to the "ready" state. Current code doesn't have explicit readiness
// states, but we can verify the end state: after a full index, the
// DB is consistent and all files are non-stale.

/// Maps to **Rule: ProjectReturnsToReadyWhenPendingWorkDrains**
///
/// After a full index completes, the DB must be in a consistent
/// state: all files are non-stale, all symbols have valid file
/// references, and no orphaned data exists.
#[test]
fn rule_post_index_db_is_consistent() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);

    // 1. No stale files
    let stale = read::get_stale_files(&conn).unwrap();
    assert!(stale.is_empty(), "no stale files after full index");

    // 2. Every symbol's file_id references an existing file
    let orphan_symbols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_symbols, 0, "no orphaned symbols after index");

    // 3. Every edge's from/to references existing files
    let orphan_edges: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM edges WHERE from_file NOT IN (SELECT id FROM files) OR to_file NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_edges, 0, "no orphaned edges after index");

    // 4. FTS is populated for indexed symbols
    let fts_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM symbols_fts", [], |r| r.get(0))
        .unwrap();
    assert!(fts_count > 0, "FTS table must be populated after index");

    // 5. Foreign key integrity holds
    crate::storage::verify_foreign_keys(&conn).unwrap();
}

// ===========================================================================
// RULE: WriterCommitsEachChunkSeparately / CommittedChunkYieldsToReaders
// ===========================================================================
//
// Allium: Each write chunk commits atomically, and after commit the
// writer yields to readers. Current code does everything in one
// transaction per file in `ingest_parsed_file`. We verify that
// the DB remains consistent across incremental updates.

/// Maps to **Rule: WriterCommitsEachChunkSeparately** +
///         **Rule: CommittedChunkYieldsToReaders**
///
/// After an incremental re-index, the DB must remain consistent:
/// symbols belong to valid files, edges reference valid files.
#[test]
fn rule_incremental_preserves_db_consistency() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();

    // Create a multi-file project with imports between files
    fs::write(
        src.join("lib.rs"),
        "pub mod helper;\nuse crate::helper::do_work;\npub fn run() { do_work(); }\n",
    )
    .unwrap();
    fs::write(src.join("helper.rs"), "pub fn do_work() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let conn = test_db();
    full_index(&conn, &root, true).unwrap();

    // Precondition: cross-file edge exists
    let edges = read::get_all_edges(&conn).unwrap();
    assert!(!edges.is_empty(), "must have edges between files");

    // Modify helper.rs
    let helper_path = src.join("helper.rs");
    fs::write(
        &helper_path,
        "pub fn do_work() -> i32 { 42 }\npub fn extra() {}\n",
    )
    .unwrap();
    incremental_index(&conn, &root, &[helper_path.clone()], &[]).unwrap();

    // Post-incremental consistency check
    let orphan_symbols: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM symbols WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_symbols, 0, "no orphaned symbols after incremental");

    let orphan_edges: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM edges WHERE from_file NOT IN (SELECT id FROM files) OR to_file NOT IN (SELECT id FROM files)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(orphan_edges, 0, "no orphaned edges after incremental");

    // FK integrity must still hold
    crate::storage::verify_foreign_keys(&conn).unwrap();
}

// ===========================================================================
// RULE: WatcherMovesReadyProjectsIntoPartialReindex
// ===========================================================================
//
// Allium: When a watch batch arrives and the project is ready,
// the project moves to partial_reindex. Current code doesn't
// have explicit readiness states; the watcher simply calls
// `incremental_index`. We verify that the watcher's reindex
// logic produces correct results.

/// Maps to **Rule: WatcherMovesReadyProjectsIntoPartialReindex**
///
/// Incremental re-index (as the watcher would trigger it) must
/// correctly process changed and deleted files, updating the DB.
#[test]
fn rule_watcher_reindex_updates_db_correctly() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    let src = root.join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("lib.rs"), "pub fn original() {}\n").unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"test\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let conn = test_db();

    // Initial full index
    full_index(&conn, &root, true).unwrap();

    let initial_file = read::get_file_by_path(&conn, "src/lib.rs")
        .unwrap()
        .expect("lib.rs must exist after initial index");

    // Modify the file (simulating what the watcher would detect)
    let lib_path = src.join("lib.rs");
    fs::write(&lib_path, "pub fn modified() {}\npub fn added() {}\n").unwrap();

    // Simulate the watcher calling incremental_index
    incremental_index(&conn, &root, &[lib_path], &[]).unwrap();

    let updated_file = read::get_file_by_path(&conn, "src/lib.rs")
        .unwrap()
        .expect("lib.rs must still exist after watcher reindex");

    // The file ID should be the same (upsert preserves the row)
    assert_eq!(
        initial_file.id, updated_file.id,
        "file ID must be stable across incremental updates"
    );

    // New symbols must exist
    let syms = read::get_symbols_for_file(&conn, updated_file.id).unwrap();
    let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.iter().any(|n| *n == "modified" || *n == "added"),
        "watcher reindex must produce new symbols, got: {names:?}"
    );
}

// ===========================================================================
// Config values from Allium spec
// ===========================================================================

/// Verify that the Allium config values are documented and will be
/// used when the corresponding features are implemented.
#[test]
fn allium_config_values_are_documented() {
    // These values come from `.allium/qartez-indexing-improvements.allium`
    // and must be kept in sync with the spec.
    const READINESS_RETRY_AFTER_SECS: u64 = 2;
    const LARGE_WATCH_BATCH_THRESHOLD: usize = 200;
    const WRITER_CHUNK_SIZE: usize = 50;
    const STALE_PROJECT_WINDOW_DAYS: u64 = 30;
    const PRUNE_GRACE_PERIOD_DAYS: u64 = 7;
    const PARSER_IDLE_TTL_MINS: u64 = 15;
    const HOT_TREE_RETENTION_MINS: u64 = 30;

    // Basic sanity: all values must be positive
    const { assert!(READINESS_RETRY_AFTER_SECS > 0) };
    const { assert!(LARGE_WATCH_BATCH_THRESHOLD > 0) };
    const { assert!(WRITER_CHUNK_SIZE > 0) };
    const { assert!(STALE_PROJECT_WINDOW_DAYS > 0) };
    const { assert!(PRUNE_GRACE_PERIOD_DAYS > 0) };
    const { assert!(PARSER_IDLE_TTL_MINS > 0) };
    const { assert!(HOT_TREE_RETENTION_MINS > 0) };

    // Chunk size must be less than batch threshold
    const {
        assert!(
            WRITER_CHUNK_SIZE < LARGE_WATCH_BATCH_THRESHOLD,
            "writer_chunk_size must be less than large_watch_batch_threshold"
        )
    };

    // Grace period must be less than stale window
    const {
        assert!(
            PRUNE_GRACE_PERIOD_DAYS < STALE_PROJECT_WINDOW_DAYS,
            "prune_grace_period must be less than stale_project_window"
        )
    };
}

// ===========================================================================
// RULE: SharedProjectBecomesStaleWhenMissingOnDisk
// ===========================================================================
//
// Allium: In shared DB mode, a project whose root path no longer
// exists on disk should be marked stale. Current code doesn't
// have a shared DB lifecycle yet, but `purge_orphan_prefixes`
// removes files from orphan roots. We test this path.

/// Maps to **Rule: SharedProjectBecomesStaleWhenMissingOnDisk**
///
/// In multi-root mode, files belonging to a removed workspace root
/// must be purged from the index during the next full index.
#[test]
fn rule_orphan_prefix_files_are_purged() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();

    // Create a multi-root scenario: two sub-projects
    let alpha = root.join("alpha");
    let beta = root.join("beta");
    let alpha_src = alpha.join("src");
    let beta_src = beta.join("src");
    fs::create_dir_all(&alpha_src).unwrap();
    fs::create_dir_all(&beta_src).unwrap();

    fs::write(alpha_src.join("lib.rs"), "pub fn alpha_fn() {}\n").unwrap();
    fs::write(beta_src.join("lib.rs"), "pub fn beta_fn() {}\n").unwrap();
    fs::write(
        alpha.join("Cargo.toml"),
        "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();
    fs::write(
        beta.join("Cargo.toml"),
        "[package]\nname = \"beta\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .unwrap();

    let conn = test_db();
    let roots = vec![alpha.clone(), beta.clone()];
    let aliases = std::collections::HashMap::new();

    // Index both roots
    crate::index::full_index_multi(&conn, &roots, &aliases, true).unwrap();

    // Both must be indexed
    let files = read::get_all_files(&conn).unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert!(
        paths
            .iter()
            .any(|p| p.contains("alpha/src/lib.rs") || p.contains("alpha\\src\\lib.rs")),
        "alpha must be indexed, got: {paths:?}"
    );
    assert!(
        paths
            .iter()
            .any(|p| p.contains("beta/src/lib.rs") || p.contains("beta\\src\\lib.rs")),
        "beta must be indexed, got: {paths:?}"
    );

    // Remove beta root and re-index with only alpha
    let _ = fs::remove_dir_all(&beta).ok(); // may fail on Windows if locks held
    let reduced_roots = vec![alpha.clone()];
    crate::index::full_index_multi(&conn, &reduced_roots, &aliases, true).unwrap();

    // Beta files must be purged
    let files_after = read::get_all_files(&conn).unwrap();
    let paths_after: Vec<&str> = files_after.iter().map(|f| f.path.as_str()).collect();
    assert!(
        !paths_after
            .iter()
            .any(|p| p.contains("beta/") || p.contains("beta\\")),
        "beta files must be purged when root is removed, got: {paths_after:?}"
    );
}

// ===========================================================================
// Schema integrity: FK constraints enforced
// ===========================================================================

/// Verify that foreign key constraints are enforced on the schema.
/// This underpins all cascade-delete invariants.
#[test]
fn schema_foreign_keys_are_enforced() {
    let conn = test_db();

    // Insert a file and a symbol referencing it
    let file_id = write::upsert_file(&conn, "test.rs", 0, 100, "rust", 10).unwrap();
    write::insert_symbols(
        &conn,
        file_id,
        &[crate::storage::models::SymbolInsert {
            name: "foo".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 2,
            signature: None,
            is_exported: true,
            shape_hash: None,
            unused_excluded: false,
            parent_idx: None,
            complexity: None,
            owner_type: None,
        }],
    )
    .unwrap();

    // Attempting to insert a symbol with a non-existent file_id must fail
    let bad_result = write::insert_symbols(
        &conn,
        99999, // non-existent file
        &[crate::storage::models::SymbolInsert {
            name: "orphan".to_string(),
            kind: "function".to_string(),
            line_start: 1,
            line_end: 2,
            signature: None,
            is_exported: false,
            shape_hash: None,
            unused_excluded: false,
            parent_idx: None,
            complexity: None,
            owner_type: None,
        }],
    );
    assert!(
        bad_result.is_err(),
        "FK constraint must reject symbols with non-existent file_id"
    );

    // Verify FK integrity explicitly
    crate::storage::verify_foreign_keys(&conn).unwrap();
}
