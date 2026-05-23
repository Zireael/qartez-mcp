# Bead #1 Review Context

## Bead ID: qartez-mcp-latest-2ss.1
## Title: Bug: Give watcher its own dedicated DB connection
## Priority: P0 (Critical)

## Problem Statement
The watcher and server share `Arc<Mutex<Connection>>` to the SQLite database. When the watcher performs a reindex batch, it holds the mutex for the entire duration (seconds to minutes), blocking ALL server queries. This defeats SQLite WAL mode's concurrent read capability.

## Current Code Evidence (src/watch.rs)

```rust
// Line 191-197: Watcher acquires lock
let conn = match self.db.lock() {
    Ok(g) => g,
    Err(poisoned) => { ... poisoned.into_inner() }
};

// Line 200-203: Sets writer state while holding lock
crate::storage::write::set_writer_state(
    &conn,
    crate::readiness::WriterState::IncrementalIndexing,
)?;

// Line 204-216: Entire reindex + pagerank computation while holding lock
let result = (|| {
    index::incremental_index_with_prefix_chunked(...) // potentially seconds to minutes
    graph::pagerank::compute_pagerank(...) // additional computation
    graph::pagerank::compute_symbol_pagerank(...)
    Ok::<(), anyhow::Error>(())
})();

// Line 219: Reset state while holding lock (let _ = ignores errors)
let _ = crate::storage::write::set_writer_state(&conn, crate::readiness::WriterState::Idle);

result // returned at line 220
```

## Desired Change
Remove `db_arc()` sharing. Watcher should open its own `Connection` to the same `db_path` (following the background indexer pattern at main.rs:193).

## Key Files to Change
- `src/watch.rs`: Watcher struct no longer holds `Arc<Mutex<Connection>>`
- `src/main.rs`: Pass `db_path` to watcher instead of `server.db_arc()`
- `src/server/mod.rs`: `attach_watcher` passes `db_path` instead of connection

## Acceptance Criteria
- [ ] Watcher has its own dedicated connection
- [ ] Server queries NOT blocked during reindex
- [ ] Readiness/writer_state transitions still work
- [ ] All existing tests pass + new latency test
