# Bead Review: qartez-mcp-latest-na0 — Resolve src/storage/read.rs merge conflict

**Reviewed:** 2026-05-20
**Mode:** Pre-mortem (the-fool) + Code Review (ce-code-review, report-only)
**Reviewer:** Hephaestus

---

## 1. Pre-Mortem Analysis

**Timeframe:** 3 weeks from now

### Failure Narratives

#### F1. Upstream removes has_hot_tree/tree_cache entirely — bead assumes they're preserved — Likelihood: HIGH | Impact: HIGH

The bead description says "use `prepare_cached` + include `has_hot_tree` and `tree_cache` columns." But upstream **removes** `has_hot_tree` and `tree_cache` from ALL SQL queries, ALL row-mapping functions (`row_to_file`, `row_to_file_joined`), ALL join column constants, and ALL manual column-index readers (like `get_cochanges` and `get_most_imported_files`). This is not a simple "combine both" — it's a fundamental design conflict: local HEAD added hot-file caching columns, upstream removed them.

**Consequence chain:**
- 1st order: If we add back `has_hot_tree`/`tree_cache` to the SQL but upstream removed them from `row_to_file`, the code won't compile
- 2nd order: Must either re-add the column mapping to every row function (large scope expansion) or drop hot-file caching
- 3rd order: Dropping hot-file caching invalidates the local feature that depends on it (incremental reparsing)

#### F2. Upstream removes readiness read functions — not addressed in bead — Likelihood: HIGH | Impact: MEDIUM

Upstream removes `get_readiness()`, `get_writer_state()`, and their test `test_get_readiness_roundtrip`. These are part of the local readiness feature. The bead's scope says "specifically `get_all_files`" but the conflict actually spans the entire file.

**Consequence chain:**
- 1st order: `get_readiness`/`get_writer_state` are gone; callers elsewhere in the codebase break
- 2nd order: Must re-add these functions or remove all readiness-reading code paths
- 3rd order: Cross-file breakage not captured by the bead's narrow scope

#### F3. Upstream removes several public API functions — Likelihood: MEDIUM | Impact: MEDIUM

Upstream removes `clones_get_all_ordered_groups`, `hierarchy_direct_subtypes`, `hierarchy_direct_supertypes`. These are public functions that may have callers in other modules. The bead doesn't address this.

**Consequence chain:**
- 1st order: Compilation fails wherever these removed functions are called
- 2nd order: Must either re-add these functions or update all callers
- 3rd order: Scope expands well beyond `get_all_files`

### Early Warning Signs

| Signal | Failure It Predicts | Check Frequency |
|--------|-------------------|-----------------|
| `has_hot_tree`/`tree_cache` missing from `row_to_file` after merge | F1 | During merge |
| `get_readiness`/`get_writer_state` removed | F2 | During merge |
| Public functions (`clones_get_all_ordered_groups`, `hierarchy_direct_*`) missing | F3 | After merge |

### Mitigations

| Failure | Mitigation | Effort | Reduces Risk By |
|---------|-----------|--------|-----------------|
| F1 | Decide: keep hot-file columns (re-add to all row functions, ~8 sites) or drop hot-file caching (accept upstream's design) | High | 90% |
| F2 | Add readiness read functions back as part of this bead or create a separate bead for readiness reconciliation | Medium | 70% |
| F3 | Check callers of removed functions; if used, re-add or update callers | Medium | 60% |

### Inversion Check

**What would guarantee failure:**
1. Treating this as a `prepare` → `prepare_cached` swap when upstream removes 250+ lines
2. Scoping to `get_all_files` only when the conflict affects the entire file
3. Not coordinating with server/mod.rs bead about readiness system

**Do any exist now?** YES — all three. The bead frames this as a trivial swap but the actual diff removes `has_hot_tree`/`tree_cache` from every query, removes readiness functions, and removes public API functions.

---

## 2. Code Review Findings

### P0 — Bead fundamentally misrepresents the conflict scope and severity

**File:** Bead description
**Issue:** The bead says "Scope: `src/storage/read.rs`, specifically `get_all_files`." The actual diff touches: `row_to_file`, `row_to_file_joined`, `SYMBOL_FILE_JOIN_COLS`, `get_file_by_path`, `get_all_files`, `get_files_ranked`, `get_all_files_ranked`, `get_all_symbols_with_path`, `get_file_by_id`, `get_cochanges`, `get_most_imported_files`, `get_subtypes`, `get_supertypes`, `get_symbol_references` (×2), `get_stale_files`, `boundaries_all_files`, `count_unused_exports` (adds materialize call), `get_unused_exports_page` (adds materialize call), plus removes `get_readiness`, `get_writer_state`, `clones_get_all_ordered_groups`, `hierarchy_direct_subtypes/supertypes`, and the readiness test. This is **not** a "specifically `get_all_files`" change.
**Autofix class:** gated_auto
**Suggested fix:** Expand scope to cover the entire file. Rewrite the implementation plan as: (1) Decide whether to keep `has_hot_tree`/`tree_cache` columns, (2) Apply `prepare_cached` to all query sites, (3) Decide whether to keep readiness read functions, (4) Decide whether to keep removed public API functions.

### P1 — "Combine both" strategy doesn't work for has_hot_tree/tree_cache

**File:** Bead description, "Implementation Plan" step 2
**Issue:** The plan says "update SQL string inside `prepare_cached` to include `has_hot_tree` and `tree_cache`." But upstream also removed these columns from `row_to_file` and `row_to_file_joined`. Just adding them back to the SQL without adding them back to the row-mapping functions causes a type mismatch (query returns columns that the row mapper doesn't read).
**Autofix class:** gated_auto
**Suggested fix:** If keeping hot-file columns, must also restore them in `row_to_file`, `row_to_file_joined`, `SYMBOL_FILE_JOIN_COLS`, and every manual column-index reader. This is ~8 sites across the file.

### P2 — New upstream behavior (materialize_unused_exports_if_dirty) not mentioned

**File:** Bead description
**Issue:** Upstream adds `materialize_unused_exports_if_dirty(conn)?` calls in `count_unused_exports` and `get_unused_exports_page`. This is a behavioral change that's not addressed in the bead.
**Autofix class:** manual
**Suggested fix:** Document this change in the implementation plan. Decide whether to keep it (it's an upstream improvement for deferred recomputation).

### P2 — Duplicate column references in local HEAD

**File:** Local `src/storage/read.rs`
**Issue:** The local HEAD has duplicate column references like `f.has_hot_tree AS f_has_hot_tree, f.tree_cache AS f_tree_cache, f.has_hot_tree AS f_has_hot_tree, f.tree_cache AS f_tree_cache` (repeated 2-4 times in some queries). This suggests a prior merge error or copy-paste issue.
**Autofix class:** advisory
**Suggested fix:** Whether keeping or removing these columns, ensure the final version has no duplicate references.

---

## 3. Assessment

| Dimension | Rating | Notes |
|-----------|--------|-------|
| **Comprehensive** | 2/10 | Covers only 1 of ~15 affected functions; misses readiness, API removals, new upstream behavior |
| **Complete** | 3/10 | Plan addresses `get_all_files` only; massive gap in scope |
| **Coherent** | 5/10 | Internal logic is sound for the described scope, but the scope is wrong |
| **Staged** | 4/10 | No dependency on readiness-related beads despite removing readiness functions |
| **Scoped** | 2/10 | "Specifically `get_all_files`" is far too narrow for a file-wide conflict |
| **Happy paths** | 4/10 | Happy path described for `get_all_files` but not for the rest |
| **Edge cases** | 2/10 | No edge cases identified; doesn't address the fundamental design conflict (keep vs drop hot-file columns) |

**Overall:** This is the most severely under-scoped bead of the four. The conflict affects 15+ functions, removes the readiness system from this file, removes public API functions, and introduces new upstream behavior — yet the bead describes it as a simple `prepare` → `prepare_cached` swap plus column addition. The fundamental design decision (keep or drop hot-file caching) is not addressed.

---

## 4. Recommendations

1. **Critical: Expand scope to entire file** — The conflict is file-wide, not limited to `get_all_files`.
2. **Critical: Make the design decision** — Keep `has_hot_tree`/`tree_cache` (requires restoring in ~8 sites) or drop them (accept upstream's removal). This is the central architectural question.
3. **Coordinate with server/mod.rs bead** — Both beads affect the readiness system; they must be resolved together.
4. **Document the new upstream behavior** — `materialize_unused_exports_if_dirty` is an upstream improvement that should be kept.
5. **Add removed public functions to scope** — Decide whether to keep `clones_get_all_ordered_groups`, `hierarchy_direct_subtypes/supertypes`, or accept their removal.
