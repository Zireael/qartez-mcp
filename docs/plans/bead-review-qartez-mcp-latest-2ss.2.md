# Bead Review: qartez-mcp-latest-2ss.2

## Bead
**ID**: qartez-mcp-latest-2ss.2
**Title**: Bug: Add startup crash recovery for stuck readiness/writer_state
**Priority**: P0 (Critical)
**Issue Type**: bug

## the-fool Pre-mortem Analysis

### Failure #1: Corrupt meta table → silent startup hang (CRITICAL)
Process crash corrupts the meta table (partial write during WAL checkpoint, torn page, or filesystem-level corruption). On startup, recovery code reads readiness/writer_state but gets garbage or deserialization error.

- **Trigger**: OS crash, disk full during prior write, filesystem cache flush interrupted mid-page
- **Detection**: `read()` on meta table returns `Err(...)` — if swallowed (`.ok()` / `unwrap_or_default`)
- **Mitigation**: Read readiness in the **same transaction** as the fingerprint check; if meta read errors, treat as crash and abort startup; validate serialized bytes before deserializing

### Failure #2: Over-eager Ready — fingerprint matches but index is partially corrupted (CRITICAL)
Crash during partial reindex writes the fingerprint *before* the reindex completes. Recovery sees the fingerprint, concludes "index is valid," sets `Ready`, and serves queries against a corrupt/incomplete index — silent data corruption.

- **Trigger**: Crash during partial reindex after fingerprint was written but before index write completed
- **Detection**: No startup error; queries may return stale results or inconsistent cross-references
- **Mitigation**: Do NOT rely solely on `fingerprint_matches`. Track an explicit **`last_completed_write_timestamp`** or **monotonic generation counter** in the meta table. If generation shows an in-progress write, force full reindex.

### Failure #3: Recovery race with watcher thread (HIGH)
Startup spawns file watcher (triggers incremental reindexes) before or concurrently with crash recovery. Watcher acquires write lock, then recovery tries to acquire same lock → deadlock.

- **Trigger**: Asynchronous startup where watcher is spawned before recovery
- **Detection**: Startup hangs indefinitely, liveness probes fail
- **Mitigation**: Sequence startup: `Recovery → Ready → WatcherRunning`. Use a startup phase enum. Add a `try_lock_for(5s)` with timeout on write lock.

### Failure #4: Recovery race between two concurrent qartez instances (HIGH)
Two qartez instances start simultaneously (duplicate pods, container restart race). Both read stale state, both attempt recovery. They corrupt each other's writes.

- **Trigger**: Orchestration race (K8s rolling update, duplicate pod)
- **Detection**: SQLite `SQLITE_BUSY` errors, `writer_state` flips between Ready and ColdStart
- **Mitigation**: Use a **filesystem lock** (e.g., `flock()` on `.lock` file, or SQLite `BEGIN IMMEDIATE`) before any meta table read. Store a **server instance ID (UUID)** in meta table on recovery.

### Failure #5: set_readiness fails silently, leaving state stranded (MEDIUM)
Recovery calls `set_readiness(&conn, Ready)` but the write fails (disk full, permissions changed, concurrent writer). Error is logged but not propagated — code continues as if recovery succeeded.

- **Trigger**: Disk full, permission changes, concurrent SQLite writer
- **Detection**: Server starts but meta table still shows `ColdStart`. External monitoring sees stuck readiness.
- **Mitigation**: Make all `set_readiness` calls fallible — if write fails, abort startup. Use a **two-phase commit** pattern within the same transaction as fingerprint check and state validation. After writing, read back and verify.

### Failure #6: Fingerprint check is expensive → kills startup latency (MEDIUM)
Fingerprint check traverses the entire index. On a large index (millions of symbols), this takes 10-60 seconds. Every startup pays this cost, even clean startups.

- **Trigger**: Normal restart, no crash
- **Detection**: Startup time regression, container probes timeout
- **Mitigation**: Use a lightweight fingerprint (hash of manifest with timestamps/sizes, not full content). Cache the last-known-good fingerprint in a separate meta row. Make the check lazy: fast hash-of-hashes first, deep check only if fast fails.

### Inversion Check
| Assume the opposite | What breaks? |
|---|---|
| **Don't recover at all** → start with ColdStart every time | Full reindex on every restart. O(10m) startup. Always correct but terrible UX. |
| **Recover too aggressively** → always set Ready | Corrupted index serves bad queries silently. Data corruption visible to users. |
| **Recover too conservatively** → always reindex | Safe but expensive. Every restart rebuilds the index. |
| **No recovery — flag crash and stop** | Server won't start after crash. Requires manual intervention. Safe but not self-healing. |

## Synthesis & Action Items for Bead #2

### Must-Fix Before Implementation
1. **CRITICAL**: Do NOT rely solely on `fingerprint_matches` to determine if index is valid. In a crash scenario, fingerprint could exist but the index is partial. Implement a **monotonic generation counter** committed only after all write operations complete. Recovery checks generation, not just fingerprint.
2. **CRITICAL**: If meta table read returns an error, do NOT silently fall back to ColdStart — this could mask real corruption. Treat as unrecoverable and abort startup.
3. **HIGH**: Add **startup phase sequencing** before watcher starts. Recovery must complete before the watcher is allowed to spawn.
4. **HIGH**: Use a **filesystem-level lock** (`flock()` on `.lock` file or SQLite `BEGIN IMMEDIATE`) before any meta read/write. This prevents two qartez instances from simultaneously attempting recovery.

### Should-Fix During Implementation
5. **MEDIUM**: Make all `set_readiness` calls fallible and propagate errors up. Do not silently ignore them (`let _ =`)
6. **MEDIUM**: Optimize fingerprint check to use a lightweight manifest hash (O(files) not O(symbols)) as the primary check. Only run the full content hash when the fast check fails.
7. **MEDIUM**: After writing the readiness/writer_state, read it back and verify it matches. This is a **read-after-write sanity check**.
8. **P3**: Consider adding a `--force-recovery` flag for manual recovery scenarios, but default to automatic recovery in the absence of `readiness=Ready` + `writer_state=Idle`.

### Updated Acceptance Criteria
- [ ] Startup recovery checks current readiness/writer_state from meta table before overwriting
- [ ] If state is non-terminal (Indexing, PartialReindex, Maintenance, Failed, or writer_state≠Idle):
  - [ ] If generation counter is at latest → set Ready (normal startup)
  - [ ] If generation counter shows in-progress write → set ColdStart (force reindex)
- [ ] Recovery is protected by a filesystem lock (`flock()` or `.lock` file) to prevent concurrent startup races
- [ ] Watcher is NOT started until recovery phase is complete
- [ ] `set_readiness` failures abort startup (no silent ignoring)
- [ ] After write, readiness/writer_state are read back and verified
- [ ] Fingerprint check uses a lightweight manifest hash, deep check deferred to when fast check fails
- [ ] All existing tests pass + new recovery tests: simulate crash at various points
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`

## Additional Edge Cases Not Addressed in Bead
1. **Generation counter bookkeeping**: Who increments the counter? Before the write or after? Will the counter wrap (unlikely but possible)? Only useful if it's monotonically increasing.
2. **Tree corruption vs reindex state mismatch**: `fingerprint_matches` checks tree integrity (hash of tree structure), not the index. If the meta table is corrupt but the tree is fine, the server might still serve correct data with a missing meta row. Conversely, if meta is fine but tree is corrupt, the server starts with Ready but serves broken data.
3. **Recovery after power loss on a filesystem that supports copy-on-write (e.g., btrfs, zfs)**: The WAL might be recoverable but the meta table write might be lost in a different copy. This is filesystem-specific but could lead to a "recover but actually fine" vs "don't recover but actually should" ambiguity.
4. **Testing gap**: The acceptance criteria have no explicit test for "simulate a crash during partial reindex, verify recovery detects it as a crash rather than normal startup." This requires a test harness that can inject a `std::process::abort()` or `panic!()` at a specific point in the reindex and then verify startup.
