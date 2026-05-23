# Bead Review: qartez-mcp-latest-2ss.4

## Bead
**ID**: qartez-mcp-latest-2ss.4
**Title**: Bug: Add Failed state recovery path (defer queries + Failed→ColdStart)
**Priority**: P0 (Critical)
**Issue Type**: bug
**Labels**: critical, readiness, state-machine

## the-fool Pre-mortem Analysis

### Failure Mode #1: No producer of `Failed` state exists — change is dead code (CRITICAL)
**What**: `ReadinessState::Failed` is defined and round-tripped in tests, but **no production code path ever sets it**. The background indexer (`main.rs:196-206`) logs `tracing::error!` and returns. The watcher (`watch.rs:151-152`) logs `tracing::error!` and continues. Neither calls `set_readiness(Failed)`. The change plan adds consumer logic (defer, recover, retry) without a producer.

**Detection**: A query-against-corrupted-index scenario cannot be reproduced because the corrupted index state is never persisted as `Failed`. Readiness stays at `Indexing` (set at line 164) indefinitely. Queries get deferred with 2s retry but never trigger recovery because no code transitions `Indexing`→`Failed`.

**Mitigation**: Do step 0 first: **add the error-to-Failed wiring**. Replace `tracing::error!` with `set_readiness(&conn, Failed)` in both `main.rs` background indexer and `watch.rs:reindex`.

### Failure Mode #2: `Failed` + 5s retry floods the server with pointless retries (HIGH)
**What**: The plan proposes `retry_after_secs() = 5` for `Failed`. A full reindex takes minutes. Clients retry every 5 seconds during the multi-minute recovery, producing 20+ req/s of deferred responses that waste CPU, DB reads, and I/O.

**Mitigation**: Use exponential backoff: `Failed` → 10s minimum, `ColdStart` → 5s, `Indexing` → 2s (existing). Tie `retry_after` to a `last_transition_at` timestamp.

### Failure Mode #3: Allium spec contract drift — `Failed` excluded from both deferred AND served paths (HIGH)
**What**: The Allium spec `QueryDefersUntilIndexIsUsable` requires `readiness in [cold_start, indexing, maintenance]` to defer. `Failed` is not included. The invariant `QueriesAreOnlyServedFromUsableIndexes` requires `readiness in [ready, partial_reindex]`. `Failed` is not in that list either. Current spec says `Failed` queries are neither deferred nor served.

**Mitigation**: Update the Allium spec in the same change set. Add `failed` to `QueryDefersUntilIndexIsUsable` requires list. Add a new rule for `Failed` recovery.

### Failure Mode #4: `recover_to()` defines the correct next state but nobody calls it (MEDIUM)
**What**: `can_recover()` and `recover_to()` are pure functions on the state enum. Nothing calls them. No periodic recovery loop, no watcher callback, no health-check timer. The state stays `Failed` indefinitely even with the clean recovery path defined.

**Mitigation**: Hook recovery into at least one caller: first deferred query in `Failed` spawns recovery, or hook into watcher first file change after `Failed`, or add a `qartez_maintenance --recover` tool.

### Failure Mode #5: Full reindex for scoped transient error causes unnecessary data loss (MEDIUM)
**What**: `Failed` means "unrecoverable error," but the error could be scoped (one file failed to parse). `recover_to()` → `ColdStart` → full reindex discards the entire index for a 1-file transient failure. 2-5 minute query downtime for a 1-file error.

**Mitigation**: Distinguish scoped failures (transition to `PartialReindex` — index is partial but usable) from catastrophic failures (transition to `Failed` — require full recovery). The existing code already handles scoped errors by logging (too permissive), but the proposed fix is too aggressive.

### Failure Mode #6: Auto-recovery livelocks against persistent FS error (MEDIUM-LOW)
**What**: A persistent FS error (permission denied on a directory, broken symlink) triggers during recovery reindex, re-setting `Failed` before recovery completes. Creates a livelock: recover → ColdStart → Indexing → (error) → Failed → recover → ...

**Mitigation**: Add error debounce / circuit breaker. Track `failed_count` in meta table. If `failed_count > N` (e.g., 3) within a window, escalate to `Maintenance` state requiring manual intervention.

## ce-code-review Findings

### Finding 1 (P0): `Failed` falling through to serve queries violates Allium invariant
**File**: `src/server/mod.rs:546-562`, `src/readiness.rs:75-80`
**Issue**: The match gating on `should_defer()` has a fallthrough `Ok(Some(_))` arm that proceeds with the query. Since `Failed` returns `false` from `should_defer()`, queries proceed silently against a potentially corrupt index.

**Recommendation**: Add `Failed` to `should_defer()`. Update Allium rule `QueryDefersUntilIndexIsUsable` to include `failed`.

### Finding 2 (P1): `Failed` is never actually written anywhere
**Issue**: `ReadinessState::Failed` is defined but no code ever writes it. The recovery path is dead code without an entry path.

**Recommendation**: Write the transition rule and implement error→`Failed` wiring in `main.rs` and `watch.rs`.

### Finding 3 (P1): No `qartez_reindex` MCP tool exists for client-initiated recovery
**Issue**: The design says "return deferred response with reindex suggestion," but no MCP tool exists to actually trigger recovery. The suggestion is noise.

**Recommendation**: Add `qartez_reindex` as an MCP tool, bypassing the readiness gate.

### Finding 4 (P1): No throttling on Failed→ColdStart→Failed loop
**Issue**: Without throttling, the recovery loop can cycle indefinitely on a persistent error.

**Recommendation**: Add `recovery_attempts` counter, `last_recovery_attempt_at` timestamp, and exponential backoff.

### Finding 5 (P2): No max retry limit
**Issue**: Clients spin forever against a permanently failed index.

**Recommendation**: After K consecutive deferred responses from `Failed`, return an explicit error.

### Finding 6 (P2): Unified `retry_after_secs` (always 2s) needs differentiation
**Issue**: Adding `Failed`-specific retry creates a disconnect from the config-based spec value.

**Recommendation**: Add `failed_retry_after: Duration` to the Allium config.

## Synthesis & Action Items for Bead #4

### Must-Fix Before Implementation
1. **CRITICAL**: Add error→`Failed` wiring. No production code currently calls `set_readiness(Failed)`. Add it to `main.rs` background indexer error handler and `watch.rs:reindex` error handler. This is step 0.
2. **CRITICAL**: Add `Failed` to `should_defer()`. A query served from `Failed` state violates the Allium invariant.
3. **HIGH**: Add a `qartez_reindex` MCP tool so clients can act on the "reindex suggestion" in the deferred response. This tool must bypass the readiness gate.
4. **HIGH**: Update the Allium spec in the same change set: add `failed` to `QueryDefersUntilIndexIsUsable`, add a recovery rule, document invariants.
5. **HIGH**: Add exponential backoff to `retry_after_secs` for `Failed` (minimum 10s baseline, scale with elapsed time since failure).

### Should-Fix During Implementation
6. **MEDIUM**: Distinguish scoped failures (`PartialReindex`) from catastrophic (`Failed`). A single-file parse error should not trigger a full reindex.
7. **MEDIUM**: Add throttling: `recovery_attempts` counter, `last_recovery_attempt_at` timestamp, circuit breaker after N consecutive failures.
8. **MEDIUM**: Add a concrete caller for `recover_to()`—hook into the first deferred query in `Failed` state, not just define the function.
9. **MEDIUM**: Add `failed_retry_after` to Allium config (`Duration = 5.seconds`) instead of hardcoding.
10. **P3**: After K consecutive deferred responses from `Failed`, return an explicit error instead of deferring.

### Updated Acceptance Criteria
- [ ] `should_defer()` returns `true` for `Failed` readiness state.
- [ ] `retry_after_secs()` returns an exponential backoff (≥5s baseline) for `Failed`.
- [ ] Some production code path actually calls `set_readiness(&conn, Failed)` on unrecoverable error (main.rs background indexer or watch.rs reindex).
- [ ] A `qartez_reindex` MCP tool exists that can be called to trigger recovery from `Failed`.
- [ ] A `Failed → ColdStart` recovery transition exists and is *called* by somebody (first deferred query, watcher event, or MCP tool).
- [ ] Allium spec is updated: `failed` in `QueryDefersUntilIndexIsUsable`, new recovery rule, config entry for `failed_retry_after`.
- [ ] Recovery attempts are throttled: counter, backoff, circuit breaker.
- [ ] Unit tests verify: Failed defers queries, Failed→ColdStart transition, error→Failed wiring, recovery throttling.
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.

## Additional Edge Cases Not Addressed in Bead
1. **No entry path for `Failed`**: The bead describes what happens *after* `Failed` but doesn't specify how to *enter* it. This is the single biggest gap.
2. **Client-side recovery protocol**: If queries are deferred in `Failed` with a "reindex suggestion", how does the client trigger the reindex via MCP? A new tool is needed.
3. **Allium spec divergence**: The bead focuses on Rust code changes but doesn't mention the required Allium spec updates. Without them, spec-code alignment is broken.
4. **Livelock with persistent error**: If a faulty source file trips every reindex attempt, the recovery path is a busy loop of Failed → ColdStart → Indexing → Failed.
5. **Scoped vs catastrophic distinction**: The bead doesn't distinguish a single-file parse failure from a DB corruption. Both would transition to `Failed`, but only one truly requires a full rebuild.
