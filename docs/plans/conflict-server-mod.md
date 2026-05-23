## Objective
Resolve the merge conflict in `src/server/mod.rs` between local readiness features and upstream's dedicated watcher database connection.

## Scope
- `src/server/mod.rs`, specifically `QartezServer` struct definition and `QartezServer::attach_watcher()`.

## Context Summary
- Local (`HEAD`): Added `writer_chunk_size` logic to configure watchers to perform chunked database commits, initialized via `Watcher::with_prefix_with_chunk_size`.
- Upstream (`upstream/main`): Added a dedicated SQLite connection mechanism for the watcher. If `db_path` is present, it creates an independent connection using `crate::storage::open_db(path)` to avoid blocking on the shared connection mutex, initialized via `Watcher::with_prefix(db, ...)`.

## Implementation Plan
1. Update `QartezServer` struct to include both `db_path: Option<PathBuf>` (upstream) and `writer_chunk_size: Option<usize>` (local).
2. In `QartezServer::attach_watcher()`, combine the two features:
   - Determine the `db` connection: if `self.db_path` exists, open the dedicated DB connection (upstream logic); otherwise use `self.db_arc()`.
   - Retrieve `chunk_size` from `self.writer_chunk_size.unwrap_or(DEFAULT_WRITER_CHUNK_SIZE)` (local logic).
   - Initialize the watcher combining both arguments. (Note: Ensure `Watcher` constructor in `src/watch.rs` takes both the dedicated `db` connection and `writer_chunk_size`).

## Edge Cases to Investigate
- If the dedicated DB connection fails to open (`open_db`), does it correctly bubble up the error? (Upstream logic uses `?` which should be preserved).
- Does the dedicated connection fully support the `set_writer_state` WAL readiness state commands introduced by local `HEAD`?

## Acceptance criteria
- `src/server/mod.rs` compiles cleanly without Git markers.
- `QartezServer::attach_watcher` establishes a dedicated database connection when `db_path` is available and correctly passes the `writer_chunk_size` to the watcher.