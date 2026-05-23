# Bead Review: qartez-mcp-latest-dzg — Resolve src/server/mod.rs merge conflict

**Reviewed:** 2026-05-20  
**Mode:** Pre-mortem (the-fool) + Code Review (ce-code-review, report-only)  
**Reviewer:** Hephaestus

---

## 1. Pre-Mortem Analysis

**Timeframe:** 3 weeks from now

### Failure Narratives

#### F1. Upstream removes readiness gating entirely; bead assumes additive merge — Likelihood: HIGH | Impact: HIGH

The bead description frames the conflict as "local added writer_chunk_size, upstream added db_path — combine both." But the actual upstream diff removes 126 lines and adds only 32. Upstream **removes** the entire readiness system from server/mod.rs: `ReadinessState` import, readiness checks in `dispatch_tool_call`, and readiness/writer state initialization in test fixtures. The bead's plan to "combine both" by adding `db_path` to the struct while keeping `writer_chunk_size` doesn't account for this large deletion.

**Consequence chain:**
- 1st order: Manual merge produces code that has both `db_path` AND readiness gating, but upstream's `dispatch_tool_call` no longer has the readiness gate
- 2nd order: The readiness system is only half-integrated — the server doesn't gate queries on readiness anymore, defeating the purpose of the local feature
- 3rd order: Must decide: keep readiness gating (diverge from upstream) or remove it (lose local feature). This is an architectural decision, not a text merge.

#### F2. Dedicated DB connection doesn't support set_writer_state — Likelihood: HIGH | Impact: MEDIUM

The bead's edge case section asks "Does the dedicated connection fully support set_writer_state?" but doesn't answer it. The dedicated connection opened via `open_db(path)` is a separate SQLite connection. `set_writer_state` writes to the shared DB. Since both connections operate on the same WAL-mode file, the writes are visible, but the *semantics* differ: the dedicated watcher connection writes index data, while `set_writer_state` was designed for the shared connection. If the watcher uses the dedicated connection, it must also set writer state through that same connection, not the shared one.

**Consequence chain:**
- 1st order: Writer state is set on shared connection but watcher reads it from dedicated connection — stale reads possible under WAL
- 2nd order: Readiness signals become unreliable
- 3rd order: The entire readiness contract breaks

#### F3. Watcher constructor signature mismatch across beads — Likelihood: HIGH | Impact: HIGH

This bead says "ensure Watcher constructor takes both the dedicated db connection and writer_chunk_size." But bead qartez-mcp-latest-5m0 (watch.rs) describes merging `Watcher::with_prefix` (upstream) with `Watcher::with_prefix_with_chunk_size` (local). These two beads must produce compatible constructor signatures, but there's no explicit dependency or coordination mechanism between them.

**Consequence chain:**
- 1st order: Server/mod.rs creates a Watcher with one signature, watch.rs defines it with another
- 2nd order: Compilation fails; must rework both beads simultaneously
- 3rd order: Circular dependency between the two beads blocks progress

### Early Warning Signs

| Signal | Failure It Predicts | Check Frequency |
|--------|-------------------|-----------------|
| `dispatch_tool_call` no longer has readiness gate after merge | F1 | After merge |
| `set_writer_state` called on shared conn but watcher uses dedicated conn | F2 | During code review of merged result |
| `Watcher::with_prefix*` signature differs between server/mod and watch.rs | F3 | During compilation |

### Mitigations

| Failure | Mitigation | Effort | Reduces Risk By |
|---------|-----------|--------|-----------------|
| F1 | Before merging, decide explicitly: keep readiness gating in server (requiring re-addition of upstream-removed code) or remove readiness gating (accepting upstream's design) | Medium | 80% |
| F2 | Ensure `set_writer_state` is called on the same connection the watcher uses, or document that WAL visibility is sufficient | Low | 60% |
| F3 | Add explicit dependency: server/mod bead depends on watch.rs bead completing first, or merge them into one bead | Low | 70% |

### Inversion Check

**What would guarantee failure:**
1. Treating this as an additive merge when upstream actually removes significant code
2. Not coordinating the Watcher constructor signature with the watch.rs bead
3. Assuming dedicated DB connection and shared DB connection have identical WAL visibility guarantees without verifying

**Do any exist now?** YES — all three. The bead description frames the conflict as additive, there's no dependency link to the watch.rs bead, and the edge case about `set_writer_state` is posed as a question rather than investigated.

---

## 2. Code Review Findings

### P1 — Bead misrepresents the conflict as additive when it's actually subtractive

**File:** Bead description, "Context Summary" and "Implementation Plan"  
**Issue:** The bead says "Local added writer_chunk_size, upstream added db_path." In reality, upstream **removed** `writer_chunk_size`, the `ReadinessState` import, the readiness gate in `dispatch_tool_call`, and all readiness state initialization in test fixtures (126 lines removed, 32 added). The plan to "include both" doesn't account for the readiness system removal.  
**Autofix class:** gated_auto  
**Suggested fix:** Rewrite context summary to accurately reflect that upstream replaces `writer_chunk_size` with `db_path` AND removes the readiness gating system. Add a decision point: "Does the merge keep readiness gating (re-adding upstream-removed code) or remove it (accepting upstream's design)?"

### P1 — No dependency on watch.rs bead despite tight coupling

**File:** Bead dependencies  
**Issue:** The bead says "Ensure Watcher constructor in src/watch.rs takes both the dedicated db connection and writer_chunk_size" but doesn't declare a dependency on the watch.rs bead (qartez-mcp-latest-5m0). The Watcher constructor signature must be consistent across both files.  
**Autofix class:** gated_auto  
**Suggested fix:** Add dependency: `qartez-mcp-latest-dzg` depends on `qartez-mcp-latest-5m0` (or vice versa, depending on merge order). Alternatively, merge the two beads.

### P2 — Edge case investigation left as question instead of answer

**File:** Bead description, "Edge Cases to Investigate"  
**Issue:** Two edge cases are listed but not investigated: (1) error bubbling from `open_db`, (2) dedicated connection supporting `set_writer_state`. These should be answered before implementation, not during.  
**Autofix class:** manual  
**Suggested fix:** Investigate and document findings for both edge cases before starting implementation. For (1): confirm `?` propagation is preserved. For (2): verify WAL visibility guarantees between connections.

### P3 — Acceptance criteria don't verify behavioral correctness

**File:** Bead description, "Acceptance criteria"  
**Issue:** Criteria only check compilation and that `attach_watcher` passes both arguments. They don't verify that the watcher actually uses the dedicated connection for writes, or that readiness signaling still works correctly.  
**Autofix class:** advisory  
**Suggested fix:** Add criterion: "When `db_path` is set, watcher write operations use the dedicated connection (verifiable by checking that shared mutex is not held during reindexing)."

---

## 3. Assessment

| Dimension | Rating | Notes |
|-----------|--------|-------|
| **Comprehensive** | 4/10 | Misses the subtractive nature of upstream's changes (readiness removal) |
| **Complete** | 5/10 | Plan covers struct + constructor merge but not the dispatch_tool_call readiness gate |
| **Coherent** | 6/10 | Plan is logically consistent within its framing, but the framing is wrong |
| **Staged** | 5/10 | No dependency on watch.rs bead despite tight constructor coupling |
| **Scoped** | 6/10 | Scope is too narrow — the conflict extends beyond struct/constructor to readiness gating |
| **Happy paths** | 6/10 | Happy path described but incomplete (doesn't address readiness) |
| **Edge cases** | 4/10 | Good questions raised, but left unanswered; critical F3 (constructor mismatch) not identified |

**Overall:** This bead has the most significant issues of the four. It frames a **subtractive** upstream change as **additive**, which will lead to an incorrect merge strategy. The missing dependency on the watch.rs bead and the unanswered edge cases about `set_writer_state` create high risk of compilation failures and behavioral regressions.

---

## 4. Recommendations

1. **Critical: Reframe the conflict** — Acknowledge that upstream removes the readiness system and replaces `writer_chunk_size` with `db_path`. The merge decision is not "combine both" but "which local features to preserve against upstream's removal."
2. **Add dependency on watch.rs bead** — The Watcher constructor signature must be coordinated. Resolve watch.rs first, then adapt server/mod.rs to match.
3. **Answer edge cases before implementing** — Specifically: does `set_writer_state` work correctly when called on a different connection than the one the watcher uses?
4. **Expand scope** — The readiness gate in `dispatch_tool_call` is part of this conflict. Either explicitly include it in scope or create a separate bead for readiness system reconciliation.
