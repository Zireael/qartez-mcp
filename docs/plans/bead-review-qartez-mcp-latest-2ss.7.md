# Bead Review: qartez-mcp-latest-2ss.7

## Bead
**ID**: qartez-mcp-latest-2ss.7
**Title**: Bug: Add panic guard for writer_state cleanup
**Priority**: P1
**Issue Type**: bug
**Labels**: panic, writer-state, cleanup, guard

## Manual Review (Subagents unavailable — self-analysis)

### Problem Analysis

If the watcher (`reindex()`) or background indexer (`spawn_blocking`) panics (e.g., parser bug, OOM, assertion failure), `writer_state` stays set to `IncrementalIndexing` or `FullIndexing` in the DB. There's no `catch_unwind` or `Drop` guard to reset it.

Since `writer_state` might be used for gating (Bead #6), a stale state could permanently block queries.

### the-fool Pre-mortem (Manual)

#### Failure Mode #1: Panic guard's Drop is never called if the guard itself panics (HIGH)
**Scenario**: The `WriterStateGuard::drop()` method calls `set_writer_state`, which itself might panic (e.g., DB connection closed, `unwrap()` in the setter).

**Consequence**: If `Drop::drop()` panics, the program aborts (Rust's "double panic" → abort). The writer_state is never reset, and the process terminates uncleanly.

**Mitigation**: Ensure `set_writer_state` never panics inside `Drop`. If it does, catch the panic using `std::panic::catch_unwind` inside the `Drop` method (or use `AssertUnwindSafe`).

#### Failure Mode #2: Guard is not created if `set_writer_state` panics (MEDIUM)
**Scenario**: `set_writer_state` is called BEFORE the guard is created. If it panics, the guard's `Drop` never runs.

**Consequence**: writer_state stays at non-Idle, no cleanup.

**Mitigation**: Use RAII — create the guard FIRST (which sets `Idle`), then let the normal code set `writer_state` to `Indexing`. If a panic happens, the guard's `Drop` resets it to `Idle`. If the normal code completes without panic, it manually sets back to `Idle` before the guard drops.

#### Failure Mode #3: Guard is skipped due to early return (MEDIUM)
**Scenario**: An early `return` or `?` in the indexing code bypasses the guard's normal cleanup.

**Consequence**: Guard drops as intended (RAII handles this correctly for non-panic returns).

**Mitigation**: RAII naturally handles early returns (Drops on scope exit). This is already fine.

#### Failure Mode #4: Guard uses wrong connection (LOW, depends on Bead #1)
**Scenario**: If Bead #1 is not done yet, the server and watcher share an `Arc<Mutex<Connection>>`. If the server is panicking, the mutex is poisoned. The guard's `Drop` tries to lock the mutex, which detects poison and returns `PoisonError`. The guard skips the write.

**Consequence**: writer_state is not reset.

**Mitigation**: After Bead #1 (dedicated watcher connection), the watcher's guard uses only the watcher's own connection, which is never shared with the server's mutex. If the watcher's task panics, its connection is dropped. The guard must detect if the connection is still valid (not closed) before writing.

### Review of Proposed Solutions

The proposed `WriterStateGuard` is a good RAII pattern, but needs careful design:

1. **RAII order**: Create guard (sets `Idle` initially), then normal code sets `writer_state` to `Incremental/Full`. If panic, guard drops and resets to `Idle`. If normal completion, normal code sets `Idle` and guard's drop does nothing.

2. **Connection independence**: After Bead #1, strictly use dedicated connection for the watcher. The guard holds a reference to its own connection.

3. **No panic in Drop**: `Drop::drop()` must be infallible. The setter must be infallible. If it returns an error, guard's drop swallows it but doesn't panic.

### Updated Acceptance Criteria
- [ ] `WriterStateGuard` RAII struct wraps `writer_state` cleanup.
- [ ] Guard created before any non-Idle writer_state is set.
- [ ] `Drop::drop()` is infallible (never panics, even if DB write fails).
- [ ] Guard logs a warning if it detects a panic via `std::thread::panicking()`.
- [ ] Normal code manually resets `writer_state` to `Idle` on success before guard drops (so guard's drop is a no-op on happy path).
- [ ] All `reindex()` and `spawn_blocking` paths are covered by guards.
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.
