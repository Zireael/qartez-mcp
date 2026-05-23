# Epic Review: qartez-mcp-latest-i9sy — Merge upstream changes (4 conflict files)

**Reviewed:** 2026-05-20
**Mode:** Pre-mortem (the-fool) + Code Review (ce-code-review, report-only)
**Reviewer:** Hephaestus

---

## Cross-Bead Analysis

### Systemic Issue: All 4 beads frame subtractive upstream changes as additive

Every bead describes the conflict as "local added X, upstream added Y — combine both." In reality, upstream **removed** significant code in 3 of 4 files:

| Bead | Local Added | Upstream Actually | Bead's Framing | Reality |
|------|-------------|-------------------|---------------|---------|
| igf (Cargo.toml) | rayon, writer_chunk_size | Replaced writer_chunk_size with db_path | Additive | Additive (correct) |
| dzg (server/mod.rs) | writer_chunk_size, readiness gating | Removed readiness gating, added db_path | Additive | **Subtractive** |
| na0 (storage/read.rs) | has_hot_tree/tree_cache, prepare_cached | Removed has_hot_tree/tree_cache from all queries, removed readiness reads | Additive | **Subtractive** |
| 5m0 (watch.rs) | with_prefix_with_chunk_size | Rewrote entire file with debouncer-full, cadence, .gitignore | Additive | **Rewrite** |

This framing mismatch is the **root cause** of most issues found in the individual reviews. Beads 2-4 will fail if implemented as described because the "combine both" strategy produces incoherent code.

### Cross-Bead Dependency Graph (undeclared)

The beads have tight coupling but no declared dependencies:

```
igf (Cargo.toml) ──→ dzg (server/mod.rs) ──→ 5m0 (watch.rs)
                              │                      │
                              └──────┬───────────────┘
                                     ↓
                              na0 (storage/read.rs)
```

- **igf → dzg**: Cargo.toml must resolve before compilation is possible
- **dzg ↔ 5m0**: Watcher constructor signature must be consistent (circular dependency risk)
- **dzg + 5m0 → na0**: Readiness system in storage/read.rs depends on whether server/mod.rs keeps it

### The Readiness System Decision

The single most important architectural decision is: **keep or drop the readiness system?**

- Upstream **removes** it entirely (no `ReadinessState`, no `get_readiness`, no readiness gating in `dispatch_tool_call`)
- Local HEAD **added** it (readiness state, writer state, gating of query tools)

**If keeping readiness:**
- Must re-add upstream-removed code in server/mod.rs, storage/read.rs
- Must adapt the readiness system to work with the dedicated watcher DB connection
- Diverges from upstream, making future merges harder

**If dropping readiness:**
- Accept upstream's design (no query gating during cold start)
- Simpler merge — take upstream as base for most files
- Local readiness feature is lost; need alternative (e.g., client-side retry)

**Recommendation:** Drop the readiness system. It adds complexity to the merge and the dedicated DB connection pattern (upstream's approach) solves the same problem differently — the watcher no longer blocks the shared mutex, so tool dispatch isn't delayed during reindexing.

---

## Pre-Mortem Analysis (Epic Level)

### F1. Merge order produces circular blocker — Likelihood: MEDIUM | Impact: HIGH

If dzg (server/mod.rs) is implemented before 5m0 (watch.rs), the Watcher constructor call won't compile because watch.rs still has the old signature. If 5m0 is implemented before dzg, the `attach_watcher` method still calls the old constructor. Either order produces a compilation failure until both are done.

**Mitigation:** Implement dzg and 5m0 together in one working session, or implement 5m0 first and dzg second (since server/mod.rs calls into watch.rs, not the other way around).

### F2. Cargo.toml resolution passes but later beads reveal incompatible dep versions — Likelihood: LOW | Impact: MEDIUM

The Cargo.toml bead (igf) resolves first. But watch.rs needs `notify-debouncer-full` which may have semver conflicts with the existing `notify` crate. If the Cargo.toml bead already merged, adding `notify-debouncer-full` requires revisiting Cargo.toml.

**Mitigation:** Ensure Cargo.toml bead includes `notify-debouncer-full` in its dependency list (it currently doesn't — only rayon, notify-debouncer-full, qartez-dashboard).

### F3. Test suite doesn't pass after all 4 beads are resolved — Likelihood: HIGH | Impact: HIGH

Even if each bead compiles individually, the integration may not work. The readiness system removal affects test fixtures across the codebase. The debouncer-full API change requires test rewrites. The `has_hot_tree`/`tree_cache` removal affects any test that relies on those columns.

**Mitigation:** Run full validation (`cargo test --release`) only after ALL 4 beads are resolved. Account for significant test fixup effort.

---

## Code Review Findings

### P1 — Cargo.toml bead missing notify-debouncer-full dependency

**File:** qartez-mcp-latest-igf description
**Issue:** The bead lists rayon, notify-debouncer-full, and qartez-dashboard as the three dependencies to add. But `notify-debouncer-full` is only needed by watch.rs (bead 5m0). If the Cargo bead is implemented first without knowing the exact version requirements from watch.rs, it may add an incompatible version.
**Autofix class:** manual
**Suggested fix:** Ensure the Cargo.toml bead specifies the same `notify-debouncer-full` version that upstream's Cargo.toml uses.

### P1 — No integration validation step after all 4 beads complete

**File:** Epic description
**Issue:** The epic's success criteria include "Code compiles, passes tests, and matches upstream requirements" but no bead is responsible for running the full integration test. Each bead validates locally only.
**Autofix class:** manual
**Suggested fix:** Add a 5th bead or epilogue step: "After all 4 conflict beads are resolved, run `cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings && cargo build --release && cargo test --release` and fix any remaining issues."

### P2 — Bead ordering doesn't respect actual dependencies

**File:** Epic description
**Issue:** The epic doesn't specify implementation order. The natural order is: (1) Cargo.toml first (all others depend on compilable deps), (2) watch.rs second (defines the Watcher API), (3) server/mod.rs third (consumes the Watcher API), (4) storage/read.rs last (independent of watcher changes but dependent on readiness decision).
**Autofix class:** advisory
**Suggested fix:** Specify implementation order: igf → 5m0 → dzg → na0.

---

## Assessment (Epic Level)

| Dimension | Rating | Notes |
|-----------|--------|-------|
| **Comprehensive** | 5/10 | Covers 4 conflict files but underrepresents the conflict severity |
| **Complete** | 4/10 | Missing integration validation, missing readiness system decision |
| **Coherent** | 6/10 | Consistent but based on incorrect additive framing |
| **Staged** | 4/10 | No bead ordering; no cross-bead dependencies declared |
| **Scoped** | 5/10 | File-level scope is right but the actual conflicts are broader |
| **Happy paths** | 6/10 | Happy path described at epic level but not per-bead |
| **Edge cases** | 3/10 | No cross-bead edge cases identified |

---

## Recommendations

### Critical (must fix before implementation)

1. **Make the readiness system decision first** — This is the architectural keystone. Recommend: drop readiness (accept upstream's design) since the dedicated DB connection pattern solves the same problem.

2. **Reframe beads 2-4 as subtractive/rewrite conflicts** — The "combine both" strategy only works for bead 1 (Cargo.toml). For beads 2-4, the correct strategy is "take upstream as base, re-add only non-conflicting local features."

3. **Specify implementation order** — igf → 5m0 → dzg → na0

4. **Add integration validation step** — After all 4 beads, run the full validation suite.

### High Priority

5. **Merge beads dzg and 5m0** — Or add explicit dependency between them. The Watcher constructor coupling makes independent implementation risky.

6. **Expand na0 scope** — storage/read.rs conflict affects 15+ functions, not just `get_all_files`.

### Medium Priority

7. **Decide on has_hot_tree/tree_cache** — If dropping readiness, also consider dropping hot-file caching (upstream removed it). Re-adding it to every query and row mapper is high effort.

8. **Update Cargo.toml bead** — Ensure notify-debouncer-full version matches upstream exactly.

---

## Revised Merge Strategy

Given the analysis, here's the recommended approach:

1. **Cargo.toml (igf)** — Take upstream's Cargo.toml as base. It already has notify-debouncer-full. Add rayon if local HEAD needs it. Accept upstream's dependency versions.

2. **watch.rs (5m0)** — Take upstream's watch.rs entirely. It's a strictly better implementation (debouncer-full, cadence, .gitignore, dedicated DB). Verify `.qartezignore` support is preserved (upstream may have kept it alongside `.gitignore`).

3. **server/mod.rs (dzg)** — Take upstream as base. Remove `writer_chunk_size`, add `db_path`. Remove readiness gating from `dispatch_tool_call`. Update `attach_watcher` to use the dedicated DB connection.

4. **storage/read.rs (na0)** — Take upstream as base. Apply `prepare_cached` to any query sites that still use `prepare` (if upstream didn't already). Remove `has_hot_tree`/`tree_cache` from all queries and row mappers (matching upstream). Remove readiness read functions.

5. **Integration validation** — Run full test suite. Fix any remaining test failures from the readiness system removal.
