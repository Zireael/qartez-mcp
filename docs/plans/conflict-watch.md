## Objective
Resolve the merge conflict in `src/watch.rs` to unify chunked indexing, readiness state updates, and debounce/cadence logic.

## Scope
- `src/watch.rs`, specifically constants, `Watcher` struct, `Watcher::new`/`with_prefix`, and `Watcher::reindex`.

## Context Summary
- Local (`HEAD`): Introduced chunked database commits for large batches (`DEFAULT_WRITER_CHUNK_SIZE`, `writer_chunk_size` in `Watcher`), using `incremental_index_with_prefix_chunked`. Wrapped `reindex` execution with `set_writer_state` (IncrementalIndexing -> Idle) to signal readiness state correctly.
- Upstream (`upstream/main`): Introduced `WatcherCadence` logic to debounce and periodically trigger `pagerank` updates and WAL checkpointing (`PRAGMA wal_checkpoint(TRUNCATE)`), modifying `Watcher` struct to hold `cadence: Mutex<WatcherCadence>` and replacing fixed `index::incremental_index_with_prefix` calls with a conditional execution block based on cadence timing.

## Implementation Plan
1. Combine module constants: retain `DEFAULT_WRITER_CHUNK_SIZE` along with upstream's debouncer/cadence constants.
2. Update `Watcher` struct to include both `writer_chunk_size` and `cadence: Mutex<WatcherCadence>`.
3. Consolidate constructor functions (`Watcher::new` / `with_prefix` / `with_prefix_with_chunk_size`) to initialize both chunking and cadence fields cleanly.
4. Merge `Watcher::reindex`:
   - Keep local `HEAD` logic to update writer state (`set_writer_state(..., WriterState::IncrementalIndexing)`) at the start.
   - Execute the indexing using local's `incremental_index_with_prefix_chunked` (passing `self.writer_chunk_size`).
   - Run the upstream `WatcherCadence::tick()` logic to determine if `pagerank` updates and WAL `TRUNCATE` checkpoints should be executed.
   - Ensure the `wal_checkpoint` execution handles chunked states safely. (We want to ensure chunked commits are cleanly synced and checkpoints do not interfere midway through a batch loop).
   - Finally, reset writer state (`WriterState::Idle`).

## Edge Cases to Investigate
- Does running `PRAGMA wal_checkpoint` at the end of a chunked batch interact safely with concurrent read pools?
- Should pagerank recalculation only run after ALL chunks are completed (currently done successfully at the end of the batch in both branches, but cadence needs to evaluate if the time elapsed).

## Acceptance criteria
- `src/watch.rs` compiles without Git conflict markers.
- Watcher reindexing runs in bounded chunks of `writer_chunk_size`.
- Readiness signals (IncrementalIndexing, Idle) wrap the reindex loop successfully.
- Upstream debounce and `WatcherCadence` time gating (PageRank, WAL checkpoints) fires correctly at the end of the batch according to defined intervals.