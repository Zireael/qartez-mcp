# Bead Review: qartez-mcp-latest-2ss.8

## Bead
**ID**: qartez-mcp-latest-2ss.8
**Title**: Chore: Restore WAL checkpointing (time + size based)
**Priority**: P1
**Issue Type**: chore
**Labels**: wal, checkpoint, performance, upstream-regression

## Manual Review (Subagents unavailable — self-analysis)

### Problem Analysis

Upstream merge removed `PRAGMA wal_checkpoint(TRUNCATE/PASSIVE)` calls. WAL file grows unbounded during burst writes. SQLite's auto-checkpoint (default 1000 pages) may not be enough for large repos.

### Failure Modes (Self-identified)

#### #1 (HIGH): WAL file grows to GB during initial large repo index
**Scenario**: 10,000+ file repo, initial index writes all in one batch. WAL grows to 500MB+.

**Consequence**: Disk exhaustion on small filesystems (e.g., CI runners, VM).

**Mitigation**: Add size-based trigger: if WAL > 100MB, force checkpoint.

#### #2 (MEDIUM): Read performance degrades with large WAL
**Scenario**: Large WAL must be scanned for each read. SQLite has to check multiple versions.

**Consequence**: Query tool response time increases by 100-500ms.

**Mitigation**: Time-based checkpoint cadence: after every X minutes, force checkpoint.

#### #3 (LOW): Checkpoint fails on busy DB, but no retry
**Scenario**: `PRAGMA wal_checkpoint(TRUNCATE)` fails with `SQLITE_BUSY` if reader holds snapshot.

**Consequence**: No checkpoint, WAL continues to grow. Error is silently swallowed or only logged.

**Mitigation**: Log warning, retry with backoff, or use `PASSIVE` mode first.

### Recommended Solution

1. Add `wal_checkpoint(conn, mode)` helper:
```rust
pub fn wal_checkpoint(conn: &Connection, mode: CheckpointMode) -> Result<()> {
    let mode_str = match mode {
        CheckpointMode::Passive => "PRAGMA wal_checkpoint(PASSIVE)",
        CheckpointMode::Truncate => "PRAGMA wal_checkpoint(TRUNCATE)",
    };
    conn.execute_batch(mode_str)?;
    Ok(())
}
```

2. Add cadence-based guard to watcher's post-batch cleanup:
```rust
const CHECKPOINT_INTERVAL: Duration = Duration::from_secs(60); // 1 minute
const MAX_WAL_SIZE: u64 = 100 * 1024 * 1024; // 100MB

struct CheckpointGuard { last: Instant, interval: Duration, max_size: u64 }
impl CheckpointGuard {
    fn maybe_checkpoint(&self, conn: &Connection) -> Result<()> {
        if self.last.elapsed() >= self.interval || self.wal_size() > self.max_size {
            wal_checkpoint(conn, CheckpointMode::Truncate)?;
            self.last = Instant::now();
        }
        Ok(())
    }
}
```

3. In `watch.rs:reindex()`, call `guard.maybe_checkpoint(&watcher.conn)`.

### Updated Acceptance Criteria
- [ ] WAL checkpoint called after every reindex batch.
- [ ] Time-based trigger: at least every 60 seconds (configurable).
- [ ] Size-based trigger: if WAL > 100MB (configurable).
- [ ] `PASSIVE` checkpoint attempted first; on failure, log and continue.
- [ ] `TRUNCATE` checkpoint periodically to free disk space.
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.
