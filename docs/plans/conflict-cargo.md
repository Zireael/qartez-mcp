## Objective
Plan and execute the resolution of the `Cargo.toml` and `Cargo.lock` merge conflict between the local main branch and `upstream/main`.

## Scope
- `Cargo.toml` dependencies block.
- `Cargo.lock` file.
- Out of scope: Other source code conflicts.

## Context Summary
- Local (`HEAD`) added `rayon` as a dependency for parallel indexing.
- Upstream (`upstream/main`) added `notify-debouncer-full` and `qartez-dashboard`.
- Both branches modified the dependency list, creating a Git conflict.

## Implementation Plan
1. Manually resolve the conflict in `Cargo.toml` by including both `rayon` (from HEAD) and `notify-debouncer-full` + `qartez-dashboard` (from upstream).
2. Do not attempt to manually merge `Cargo.lock`. Instead, delete the conflict markers or the entire file, and run `cargo update --workspace` or `cargo build` to let cargo resolve the dependency tree automatically.
3. Verify that the build process can proceed to the compilation stage (even if it hits compiler errors due to other unmerged Rust files).

## Acceptance criteria
- `Cargo.toml` contains `rayon`, `notify-debouncer-full`, and `qartez-dashboard`.
- `cargo check` parses the manifest and resolves the dependency graph successfully without parse errors.
- `Cargo.lock` is clean and contains no Git conflict markers.