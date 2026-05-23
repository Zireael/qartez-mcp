# Bead Review: qartez-mcp-latest-2ss.9

## Bead
**ID**: qartez-mcp-latest-2ss.9
**Title**: Chore: Restore comprehensive gitignore handling
**Priority**: P1
**Issue Type**: chore
**Labels**: gitignore, ignore-patterns, upstream-regression, indexing

## Manual Review (Subagents unavailable — self-analysis)

### Problem Analysis

Upstream merge removed gitignore-based file eligibility, leaving only `.qartezignore`. This causes build artifacts, IDE files, and OS files to be indexed unnecessarily.

### Failure Modes (Self-identified)

#### #1 (HIGH): `node_modules/`, `target/`, `.git/` etc. indexed without exclusion
**Scenario**: `.gitignore` has exclusions but only `.qartezignore` is checked.

**Consequence**: Massive index bloat, slower queries, noisy search results.

**Mitigation**: Restore gitignore precedence chain.

#### #2 (MEDIUM): `.qartezignore` vs `.gitignore` precedence not documented
**Scenario**: User expects `target/` in `.gitignore` to be honored, but it isn't. They must duplicate in `.qartezignore`.

**Consequence**: User confusion, duplicate maintenance.

**Mitigation**: Document precedence chain and allow `.qartezignore` to override `.gitignore`.

#### #3 (LOW): System-level `.gitignore` (e.g., `core.excludesFile`) not loaded
**Scenario**: Global gitignore at `~/.config/git/ignore` is ignored.

**Consequence**: OS-specific ignores (e.g., `.DS_Store`) leak into index.

**Mitigation**: Load `core.excludesFile` from `git config`.

### Recommended Solution

1. **Priority chain** (highest to lowest):
   1. `.qartezignore` (explicit user override)
   2. `.git/info/exclude` (repo-local)
   3. `.gitignore` (per-directory and root)
   4. `core.excludesFile` (global git config)
   5. XDG `~/.config/git/ignore`

2. **Implementation**:
   - Use `git2` library to parse `.gitignore` and `.git/info/exclude`.
   - Read `core.excludesFile` via `git config`.
   - Read XDG via dirs crate.
   - Merge all into a single ignore matcher.

3. **Cache**: Cache ignore patterns per-directory to avoid re-parsing.

### Updated Acceptance Criteria
- [ ] `.gitignore` files are loaded and respected for file eligibility.
- [ ] `.git/info/exclude` is respected.
- [ ] `core.excludesFile` is respected.
- [ ] XDG `~/.config/git/ignore` is respected.
- [ ] `.qartezignore` overrides `.gitignore` for the same path.
- [ ] Index does NOT include files ignored by any of the above sources.
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.
