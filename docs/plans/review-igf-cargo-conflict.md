# Bead Review: qartez-mcp-latest-igf — Resolve Cargo merge conflict

**Reviewed:** 2026-05-20  
**Mode:** Pre-mortem (the-fool) + Code Review (ce-code-review, report-only)  
**Reviewer:** Hephaestus

---

## 1. Pre-Mortem Analysis

**Timeframe:** 2 weeks from now (after attempting to resolve the merge conflict)

### Failure Narratives

#### F1. Cargo.lock version conflicts after automated resolution — Likelihood: Medium | Impact: High

After resolving `Cargo.toml` by including rayon + notify-debouncer-full + qartez-dashboard, running `cargo update --workspace` produces version conflicts between rayon's transitive requirements (crossbeam-deque, etc.) and upstream's dependency tree. The build fails with "failed to select a version for the requirement" errors.

**Consequence chain:**
- 1st order: `cargo update` or `cargo build` fails with version resolution errors
- 2nd order: Need to inspect `cargo tree --duplicates` and add explicit version constraints or patch versions
- 3rd order: Delay cascades to other conflict resolution beads that depend on a compilable Cargo.toml

#### F2. qartez-dashboard dependency pulls incompatible transitive deps — Likelihood: Low | Impact: Medium

`qartez-dashboard` (from upstream) may share transitive dependencies with existing crates but at incompatible semver ranges. Since this is a project-internal crate, its version constraints may not be published or well-documented.

**Consequence chain:**
- 1st order: `cargo check` fails on duplicate semver-incompatible versions of shared deps
- 2nd order: Need to align the internal crate's Cargo.toml with the workspace
- 3rd order: Upstream sync may require changes in the dashboard crate itself, expanding scope

#### F3. Acceptance criteria satisfied but runtime behavior broken — Likelihood: Low | Impact: High

`cargo check` passes, `Cargo.lock` is clean, all three dependencies present — but the actual code that uses rayon (parallel indexing) has a latent issue with upstream's dedicated watcher DB connection (added in server/mod.rs). The conflict is behavioral, not textual, and only surfaces at runtime under concurrent load.

**Consequence chain:**
- 1st order: Race conditions or DB lock contention in production
- 2nd order: Hard-to-reproduce bugs, flaky tests
- 3rd order: Need to re-architect the rayon usage or DB access pattern

### Early Warning Signs

| Signal | Failure It Predicts | Check Frequency |
|--------|-------------------|-----------------|
| `cargo update` emits "failed to select a version" | F1 | During resolution |
| `cargo tree --duplicates` shows duplicate semver-incompatible versions | F1, F2 | After resolution |
| Tests pass but flake under parallel load | F3 | After full test suite |

### Mitigations

| Failure | Mitigation | Effort | Reduces Risk By |
|---------|-----------|--------|-----------------|
| F1 | Run `cargo tree --duplicates` after resolution; add explicit version constraints if needed | Low | 60% |
| F2 | Verify qartez-dashboard is a path dependency and its Cargo.toml is workspace-compatible | Low | 50% |
| F3 | Run `cargo test --release` with multiple threads after all 4 conflicts are resolved | Medium | 70% |

### Inversion Check

**What would guarantee failure:**
1. Running `cargo update` without first checking that the three added dependencies share compatible semver ranges
2. Accepting the bead as complete based on `cargo check` alone without running tests
3. Not verifying that `Cargo.lock` is tracked and committed (if gitignored, the lockfile drifts)

**Do any exist now?** Partially — the acceptance criteria do not require running tests, only `cargo check`. The bead is scoped to Cargo files only, so runtime validation is deferred to the epic level. This is acceptable *if* the epic's validation step is actually run after all beads complete.

---

## 2. Code Review Findings

### P2 — Acceptance criteria omit test execution

**File:** Bead description, "Acceptance criteria" section  
**Issue:** The criteria verify manifest correctness (`cargo check`) and lockfile cleanliness, but do not require any test execution. The parent epic requires "Code compiles, passes tests, and matches upstream requirements," but this bead doesn't gate on test passing.  
**Autofix class:** advisory  
**Suggested fix:** Add acceptance criterion: "After all 4 conflict beads are resolved, `cargo test --release` passes."

### P2 — No rollback plan if cargo update fails

**File:** Bead description, "Implementation Plan" step 2  
**Issue:** If `cargo update --workspace` fails to produce a valid lockfile, the bead provides no fallback.  
**Autofix class:** advisory  
**Suggested fix:** Add step: "If `cargo update --workspace` fails, inspect `cargo tree --duplicates` and resolve by adding explicit version constraints in `Cargo.toml`."

### P3 — "Even if it hits compiler errors" is misleading

**File:** Bead description, "Implementation Plan" step 3  
**Issue:** The phrasing "verify that the build process can proceed to the compilation stage (even if it hits compiler errors due to other unmerged Rust files)" is ambiguous. Does "proceed to compilation" mean cargo resolves the manifest and starts compiling (then fails on source errors), or that compilation succeeds? The former is what's meant, but it reads like accepting broken compilation.  
**Autofix class:** advisory  
**Suggested fix:** Rephrase: "Verify that `cargo check` resolves the dependency graph and begins compilation. Source-level compiler errors from other unmerged files are expected at this stage and do not block this bead."

### P3 — qartez-dashboard origin not specified

**File:** Bead description, "Context Summary"  
**Issue:** `qartez-dashboard` is listed as an upstream dependency but it's unclear whether it's a crates.io dependency or a path/git dependency. This matters for how cargo resolves it.  
**Autofix class:** advisory  
**Suggested fix:** Note whether `qartez-dashboard` is a path dependency, git dependency, or crates.io crate in the context summary.

---

## 3. Assessment

| Dimension | Rating | Notes |
|-----------|--------|-------|
| **Comprehensive** | 7/10 | Covers the mechanical merge well; missing dependency validation depth |
| **Complete** | 7/10 | Acceptance criteria cover Cargo.toml and Cargo.lock but not test execution |
| **Coherent** | 9/10 | Plan is clear and logically ordered |
| **Staged** | 8/10 | Appropriately scoped as a dependency-first resolution; correct ordering |
| **Scoped** | 9/10 | Rightly excludes source code conflicts; focused on Cargo files only |
| **Happy paths** | 9/10 | Happy path (all deps compatible) is well-described |
| **Edge cases** | 5/10 | No edge cases documented — version conflicts, transitive dep incompatibility, and behavioral interactions are unaddressed |

**Overall:** The bead is well-structured for the straightforward case. Its main weakness is the lack of edge-case coverage and the acceptance criteria gap (no tests). These are mitigated by the parent epic's validation step, but only if that step is actually executed.

---

## 4. Recommendations

1. **Add edge cases section** — What if `cargo update` fails? What if transitive deps conflict?
2. **Strengthen acceptance criteria** — Add `cargo tree --duplicates` check; note that full test validation happens at epic level
3. **Clarify qartez-dashboard origin** — Path dep vs. crates.io affects resolution strategy
4. **Rephrase step 3** — Remove ambiguity about accepting compiler errors
