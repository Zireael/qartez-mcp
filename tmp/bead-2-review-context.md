# Bead #2 Review Context

## Bead ID: qartez-mcp-latest-2ss.2
## Title: Bug: Add startup crash recovery for stuck readiness/writer_state
## Priority: P0 (Critical)

## Problem Statement
If the process dies mid-index (readiness=Indexing or writer_state≠Idle in the DB meta table), the next startup should auto-recover. Currently, `fingerprint_matches` short-circuits reindex on restart, so readiness never transitions to Ready. Server starts with readiness stuck at Indexing or ColdStart, permanently deferring all queries.

## Current Code Context (src/main.rs, inferred)
```rust
// Startup pseudocode:
let conn = storage::open_db(&config.db_path)?;
set_readiness(&conn, ColdStart)?; // Sets readiness=ColdStart, but if it was Indexing...
if fingerprint_matches(&conn) {  // TRUE if index is complete
    // Reindex SKIPPED
    // readiness stays at ColdStart (or whatever was set above)
    // Server starts with readiness=ColdStart or Indexing
}
```

## Key Concerns for Pre-mortem and Code Review
1. What if the process dies during `set_readiness(ColdStart)` or `set_readiness(Indexing)`? The state is written but not yet durable.
2. What if the crash happened during a long `spawn_blocking` indexer — the fingerprint was already written, so `fingerprint_matches` returns true, but some index state is incomplete?
3. What if the DB file is corrupted (not just incomplete) — does recovery from ColdStart/Indexing trigger a full reindex or a partial repair?
4. What if multiple qartez processes are writing to the same DB simultaneously — recovery from one could confusingly overwrite another's state.
5. What if the crash happened during FullIndexing but not all pages were written to disk (OS-level write-back)? SQLite WAL handles this, but the meta table state might not reflect the full index completion.
6. What if `set_readiness` and `set_writer_state` are in a different table or the DB is in WAL mode — the recovery logic needs to be WAL-aware (checkpoint before reading state).
7. Startup performance: recovery logic should not add measurable startup latency.
