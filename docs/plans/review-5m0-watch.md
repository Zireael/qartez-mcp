# Bead Review: qartez-mcp-latest-5m0 — Resolve src/watch.rs merge conflict

**Reviewed:** 2026-05-20
**Mode:** Pre-mortem (the-fool) + Code Review (ce-code-review, report-only)
**Reviewer:** Hephaestus

---

## 1. Pre-Mortem Analysis

**Timeframe:** 3 weeks from now

### Failure Narratives

#### F1. Upstream is a near-complete rewrite — "combine both" produces an incoherent hybrid — Likelihood: HIGH | Impact: HIGH

The upstream watch.rs diff is 927 lines. It replaces: (a) manual `notify` with `notify-debouncer-full`, (b) `writer_chunk_size` with `WatcherCadence` (PageRank/WAL checkpoint cadence), (c) `.qartezignore`-only with `.qartezignore` + `.gitignore` + global git excludes, (d) single-shot event processing with batch-drain windowing, (e) `DEFAULT_WRITER_CHUNK_SIZE` constant with `PAGERANK_MIN_INTERVAL_MS` / `BATCH_DRAIN_MS` / `WAL_TRUNCATE_MIN_INTERVAL_MS` / `IGNORE_REFRESH_MIN_INTERVAL_MS` constants. The bead's plan to "keep Watcher::with_prefix_with_chunk_size and add with_db_path" is fundamentally incompatible with upstream's architecture.

**Consequence chain:**
- 1st order: Hybrid code has both `writer_chunk_size` and `WatcherCadence`, both manual debounce and debouncer-full, both `.qartezignore`-only and `.gitignore`-aware ignore building
- 2nd order: Code is internally contradictory — two debounce systems, two chunking strategies, two ignore mechanisms
- 3rd order: Must choose one architecture; combining produces unmaintainable code

#### F2. Constructor signature mismatch with server/mod.rs bead — Likelihood: HIGH | Impact: HIGH

This bead creates `with_prefix_with_db_path` (adding db_path). The server/mod.rs bead creates `attach_watcher` with a different constructor call. Since both beads modify different ends of the same API, they must produce consistent signatures.

**Consequence chain:**
- 1st order: `server/mod.rs::attach_watcher` calls `Watcher::with_prefix_with_chunk_size` but watch.rs now has `with_prefix_with_db_path`
- 2nd order: Compilation fails
- 3rd order: Must rework both beads simultaneously

#### F3. notify-debouncer-full API fundamentally different from raw notify — Likelihood: HIGH | Impact: MEDIUM

Upstream replaces `notify::RecommendedWatcher` with `notify_debouncer_full::Debouncer<RecommendedWatcher, RecommendedCache>`. The event handling, error handling, and watcher lifecycle are completely different. The bead's plan doesn't address this.

**Consequence chain:**
- 1st order: Event callback signature changes from `Fn(Event)` to `Fn(DebounceEventResult)`
- 2nd order: Error handling changes from `notify::Error` to `notify_debouncer_full::Error`
- 3rd order: Test mocking must change; existing test patterns won't compile

### Early Warning Signs

| Signal | Failure It Predicts | Check Frequency |
|--------|-------------------|-----------------|
| Both `writer_chunk_size` and `WatcherCadence` exist in merged code | F1 | After merge |
| `Watcher::with_prefix_with_chunk_size` called but doesn't exist | F2 | During compilation |
| `notify::RecommendedWatcher` and `Debouncer<_, _>` both referenced | F3 | During compilation |

### Mitigations

| Failure | Mitigation | Effort | Reduces Risk By |
|---------|-----------|--------|-----------------|
| F1 | Accept upstream's architecture entirely; re-add only the .qartezignore support on top | High | 80% |
| F2 | Coordinate constructor signatures between server/mod.rs and watch.rs beads; merge as one unit | Medium | 70% |
| F3 | Update all test code for debouncer-full API | Medium | 60% |

### Inversion Check

**What would guarantee failure:**
1. Trying to combine the old manual debounce with the new debouncer-full
2. Keeping `writer_chunk_size` alongside `WatcherCadence`
3. Implementing this bead independently of the server/mod.rs bead

**Do any exist now?** YES — all three. The bead explicitly plans to keep the old constructor alongside the new one.

---

## 2. Code Review Findings

### P0 — "Combine both" strategy is architecturally incoherent for this file

**File:** Bead description, "Implementation Plan"
**Issue:** The upstream watch.rs is a near-complete rewrite (927-line diff). The bead plans to keep `with_prefix_with_chunk_size` and add `with_prefix_with_db_path`, but upstream replaces the entire chunking model with `WatcherCadence`. Combining both produces code that has two contradictory systems: chunk-based yielding and cadence-based gating.
**Autofix class:** gated_auto
**Suggested fix:** Accept upstream's architecture entirely. The correct merge strategy is: (1) Take upstream's watch.rs as the base, (2) Re-add `.qartezignore` support if it was dropped (verify — upstream may already handle it), (3) Ensure the dedicated DB connection (`db_path`) is threaded through correctly. Do NOT keep `writer_chunk_size` or the manual debounce loop.

### P1 — Missing .gitignore support analysis

**File:** Bead description
**Issue:** Upstream adds full `.gitignore` support (reading `.gitignore`, global git excludes via `excludesfile_from_git_config`, `build_local_ignore` with `GitignoreBuilder`). The bead doesn't mention this at all. If local HEAD only supports `.qartezignore`, merging must decide whether to keep upstream's `.gitignore` support.
**Autofix class:** gated_auto
**Suggested fix:** Document that upstream adds `.gitignore` support. This is a feature gain — accept it unless there's a specific reason not to.

### P1 — DEFAULT_WRITER_CHUNK_SIZE removed; local code may reference it

**File:** Bead description, "Edge Cases to Investigate"
**Issue:** The bead correctly notes that `DEFAULT_WRITER_CHUNK_SIZE` is referenced in server/mod.rs. But upstream removes it entirely and replaces it with `WatcherCadence` constants. The bead says "add a compatibility wrapper" but this perpetuates the architectural conflict rather than resolving it.
**Autofix class:** gated_auto
**Suggested fix:** Remove `DEFAULT_WRITER_CHUNK_SIZE` from all code paths. Replace with `WatcherCadence`-based cadence. Update server/mod.rs's `attach_watcher` to not pass chunk_size.

### P2 — Missing WatcherCadence/PageRank cadence test coverage

**File:** Bead description, "Acceptance criteria"
**Issue:** Acceptance criteria only verify "Watcher struct compiles" and "dedicated DB connection is used." They don't verify the new PageRank cadence, WAL checkpoint cadence, or batch-drain windowing behavior.
**Autofix class:** manual
**Suggested fix:** Add criterion: "Existing watcher tests pass with the new architecture (may require test rewrites for debouncer-full API)."

---

## 3. Assessment

| Dimension | Rating | Notes |
|-----------|--------|-------|
| **Comprehensive** | 3/10 | Misses the rewrite nature; focuses on constructor signature only |
| **Complete** | 3/10 | Plan addresses struct fields but not debounce, ignore, cadence, or event handling |
| **Coherent** | 4/10 | Internally consistent but wrong framing (additive vs rewrite) |
| **Staged** | 4/10 | No dependency on server/mod.rs despite tight coupling |
| **Scoped** | 3/10 | Too narrow — the conflict is architectural, not just constructor |
| **Happy paths** | 4/10 | Happy path for constructor merge but not for the full rewrite |
| **Edge cases** | 5/10 | Correctly identifies chunk_size compatibility concern but misses the architectural ones |

**Overall:** This bead has the second-worst scope mismatch (after storage/read.rs). The upstream watch.rs is a near-complete rewrite, not an additive change. The "combine both" strategy produces an incoherent hybrid. The correct approach is to accept upstream's architecture and re-add only the local features that don't conflict.

---

## 4. Recommendations

1. **Critical: Accept upstream's architecture** — Take upstream's watch.rs as the base. The upstream rewrite is strictly better (debouncer-full, PageRank cadence, .gitignore support, WAL checkpoint cadence). Re-add `.qartezignore` support only if upstream dropped it.
2. **Merge with server/mod.rs bead** — These two beads are so tightly coupled they should be one unit of work. At minimum, add an explicit dependency.
3. **Remove `DEFAULT_WRITER_CHUNK_SIZE`** — It's replaced by `WatcherCadence`. Don't add a compatibility wrapper.
4. **Expect test rewrites** — The debouncer-full API change requires updating all watcher tests. Account for this in effort estimates.
5. **Accept .gitignore support** — This is a feature gain from upstream. Don't fight it.
