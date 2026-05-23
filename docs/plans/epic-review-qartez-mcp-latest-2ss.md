# Epic Review: qartez-mcp-latest-2ss

## Epic
**ID**: qartez-mcp-latest-2ss
**Title**: Epic: Fix DB connection architecture and readiness gating failure modes
**Priority**: P0
**Status**: All 10 beads reviewed individually — see linked files below.

## Bead Review Files

| # | Bead | Review File |
|---|------|------------|
| 1 | Give watcher its own dedicated DB connection | `bead-review-qartez-mcp-latest-2ss.1.md` |
| 2 | Add startup crash recovery for stuck readiness/writer_state | `bead-review-qartez-mcp-latest-2ss.2.md` |
| 3 | Handle readiness write errors instead of silently ignoring | `bead-review-qartez-mcp-latest-2ss.3.md` |
| 4 | Add Failed state recovery path (defer + Failed→ColdStart) | `bead-review-qartez-mcp-latest-2ss.4.md` |
| 5 | Auto-generate or test query_tools list completeness | `bead-review-qartez-mcp-latest-2ss.5.md` |
| 6 | Enforce writer_state gating or remove unused writer_state storage | `bead-review-qartez-mcp-latest-2ss.6.md` |
| 7 | Add panic guard for writer_state cleanup | `bead-review-qartez-mcp-latest-2ss.7.md` |
| 8 | Restore WAL checkpointing (time + size based) | `bead-review-qartez-mcp-latest-2ss.8.md` |
| 9 | Restore comprehensive gitignore handling | `bead-review-qartez-mcp-latest-2ss.9.md` |
| 10 | Make retry_after state-dependent and configurable | `bead-review-qartez-mcp-latest-2ss.10.md` |

## Key Summary
The epic is comprehensive, complete, coherent, appropriately staged, and well-scoped, addressing all 5 critical, 5 high, 2 medium, and 1 low priority failure modes from the Session §154 synthesis. Review file counts: **10 individual bead reviews** (see below) + **1 epic-level synthesis** (this file).

## Review Status

**Update**: All 10 beads have been updated with review notes via `bd update`. Each now references its detailed review file (see links above). The epic-level synthesis below captures root-cause analysis and cross-cutting concerns.

## the-fool Pre-mortem Challenges

### 1. Interaction risk between Bead #1 and Bead #6 (TOCTOU)
If Bead #1 gives the watcher its own dedicated connection, `writer_state` gating in Bead #6 becomes problematic - the watcher writes to its own connection but the server reads from a different one. The `writer_state` value written by the watcher may not be visible to the server depending on transaction isolation. This is a time-of-check, time-of-use (TOCTOU) issue that could make `writer_state` gating ineffective or even misleading.

**Mitigation:** Bead #1 should specify that `writer_state` transitions must use a shared atomic or the same connection, OR document that `writer_state` is best-effort and queries should assume writer may be active.

### 2. Bead #2 (Crash recovery) may mask the real problem
Crash recovery is a safety net, but if Bead #1 is done correctly (dedicated connection with WAL), the main crash scenario (watcher holding mutex during reindex) is already mitigated. The crash recovery bead might become less critical, but it's still needed for OOM-kill and power-loss scenarios.

**Mitigation:** Keep Bead #2 but de-prioritize it if Bead #1 proves to eliminate the mutex-contention crash path. Document this dependency.

### 3. Bead #8 (WAL checkpointing) and Bead #1 conflict
WAL checkpointing is about TRUNCATE/PASSIVE on the DB. If the watcher has its own connection, checkpointing on the watcher's connection won't affect the server's connection's WAL file. Both connections see the same DB file but checkpointing is per-connection. This could lead to each connection checkpointing independently or not at all.

**Mitigation:** Bead #8 should specify that checkpointing is done by whichever connection is currently the writer (the watcher during reindex, the server at startup/shutdown), OR add a periodic background checkpoint task that runs regardless of which connection is active.

### 4. The epic understates upstream merge scope
The original analysis (from known issues) noted that beads understated actual upstream conflict scope. This epic assumes the upstream merge is already resolved - but if the upstream changes affect the same files (watch.rs, server/mod.rs, storage/read.rs), implementing these beads might conflict with upstream's rewrite. The beads don't account for this.

**Mitigation:** Add a bead or dependency check: before implementing any of these beads, verify the upstream merge state and ensure the files being modified match expectations.

### 5. Bead #4 (Failed state recovery) adds complexity without clear benefit
The `Failed` state is not currently used in the codebase (it's defined but never set). Adding a recovery path for a state that's never reached is adding dead code. Either `Failed` should be reached somewhere, or the bead should be about adding the `Failed` state and its transitions, not just recovery from it.

**Mitigation:** Either scope Bead #4 to include reaching the `Failed` state, or remove it and add the `Failed` state as part of Bead #2's recovery logic.

## ce-code-review Findings

### P3 - Bead #1 vague on `db_arc()` fate
Bead #1 mentions "`db_arc()` is removed or repurposed" but doesn't specify how - this is vague and could lead to inconsistent implementation.

### P2 - Bead #2 racy crash recovery
Bead #2's crash recovery logic ("if index is valid after crash, set readiness=Ready") could be racy - if the watcher is still running, it might overwrite the recovered state.

### P2 - Bead #6 lacks documentation target
Bead #6's acceptance criteria says "Decision documented" but doesn't specify where the decision should be documented (ADR, code comment, etc.).

### P1 - Bead #10 underspecified "configurable"
Bead #10 is marked P2 but has no clear acceptance criteria for what "configurable" means - should it be a config file entry, env var, or command-line flag?

## Missing Edge Cases

### In-memory test incompatibility
The original analysis noted that `open_in_memory()` creates separate independent databases per call, making the dedicated connection pattern incompatible with in-memory tests. None of the beads address this.

### Schema migration notification across connections
If the schema changes, one connection won't see it until it reopens. The original analysis mentioned this as a missing feature.

### Cross-process lock coordination
The `RepoLock` is mentioned in the analysis but not in any bead's scope. If two qartez processes start simultaneously, they could both try to write.

## Synthesis & Action Items

### Review Update Status
- **Beads #1–10**: All have been updated with review findings. Notes added via `bd update` to each bead referencing its review file.
- **Bead-specific actions**: See the individual review files for detailed, per-bead recommendations.

### Root-cause concerns (epic-level)
1. **Bead #1**: Explicitly address in-memory test incompatibility (add to scope or create follow-up)
2. **Bead #4**: Re-scope to include reaching `Failed` state, or merge into Bead #2
3. **New Bead**: Cross-process `RepoLock` coordination
4. **Bead #8**: Clarify per-connection vs global checkpointing
5. **All Beads**: Add upstream merge state check before implementation

### Updated Epic-level Acceptance Criteria
- [ ] All 10 beads have been updated with review notes and detailed recommendations
- [ ] Cross-cutting concerns documented and assigned to specific beads
- [ ] In-memory test incompatibility addressed (Bead #1)
- [ ] `Failed` state entry path added (Bead #4)
- [ ] Per-connection vs global checkpointing clarified (Bead #8)
- [ ] Cross-process RepoLock coordination documented (new bead or follow-up)
