# Bead Review: qartez-mcp-latest-2ss.6

## Bead
**ID**: qartez-mcp-latest-2ss.6
**Title**: Feature: Enforce writer_state gating or remove unused WriterState storage
**Priority**: P1
**Issue Type**: feature
**Labels**: writer-state, readiness, gating, dead-code-or-feature

## Manual Review (Subagents unavailable — self-analysis)

### Problem Analysis

`WriterState` is written to the DB meta table during indexing operations, but `call_tool` never reads it. The gating check in `server/mod.rs:452` uses only `ReadinessState`.

### Failure Modes (Self-identified)

#### #1 (MEDIUM): writer_state is dead code if never enforced
**Scenario**: `set_writer_state` writes to the DB, but no code path reads it for gating.

**Consequence**: Wasted DB writes, stale state in meta table, unnecessary complexity.

**Detection**: Static analysis — grep for all reads/writes of `writer_state` shows writes but no reads in `call_tool` or `is_queryable`.

**Mitigation**: Either implement gating or remove the field entirely.

#### #2 (MEDIUM): If enforced, writer_state blocks queries during incremental indexing
**Scenario**: If `WriterState` is added to gating, then during a large incremental reindex (common in active projects), ALL queries are deferred for seconds or minutes.

**Consequence**: Worse user experience than current behavior (which serves queries against slightly stale data).

**Mitigation**: Only enforce gating if `WriterState` can indicate "chunk boundary" — i.e., the writer is between batches, not during. Or, use chunked writes where each chunk is a consistent snapshot, making queries safe even during writes.

#### #3 (LOW): writer_state and readiness can become desynchronized
**Scenario**: `set_writer_state` and `set_readiness` are separate writes. If one succeeds and the other doesn't, the state is inconsistent.

**Mitigation**: Wrap them in a single transaction, or derive writer_state from the presence of a write lock.

### Review of Proposed Solutions

**Option A (Enforce):** Add `writer_state` to `should_defer()` check. During `IncrementalIndexing`, defer queries. During `Idle`, serve queries. During `FullIndexing`, defer.

Pros:
- More accurate gating during write operations.
- Prevents queries from seeing inconsistent mid-batch state.

Cons:
- All queries blocked during any incremental reindex, which could be frequent.
- No information on reindex chunk size → could block for seconds.

**Option B (Remove):** Delete `writer_state` entirely. Remove from meta table, write functions, and all callers.

Pros:
- Simplifies code.
- Removes a column of dead data.

Cons:
- Loses information about what the writer is doing (used only for logging/diagnostics).

### Preferred Approach (My recommendation)

**Hybrid**: Keep `writer_state` for **diagnostics only** (logging, health checks), but do NOT gate queries on it. Update the `call_tool` gating logic to NOT check `writer_state`.

Rationale:
- SQLite in WAL mode allows concurrent reads and writes. Queries during incremental reindex see a consistent snapshot (the last committed checkpoint).
- The only risk is if the reindex writes directly to the main DB without a transaction. If chunked commits are used, each chunk is a consistent snapshot.
- Thus, `writer_state` provides no safety value for gating, but is useful for observability.

**Implementation:**
1. Update comments/docs to clarify `writer_state` is for logging/health only, not gating.
2. Do NOT add `writer_state` to `should_defer()`.
3. Optionally, remove `writer_state` entirely if it's truly dead.

### Updated Acceptance Criteria
- [ ] Decision recorded: `writer_state` is either enforced for gating OR removed entirely.
- [ ] If enforced: `call_tool` defers during `IncrementalIndexing` and `FullIndexing`.
- [ ] If removed: `set_writer_state`, `get_writer_state`, and `WriterState` enum are deleted.
- [ ] All references to `writer_state` in `main.rs`, `watch.rs`, and `readiness.rs` are updated.
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.
