## Objective
Resolve the merge conflict in `src/storage/read.rs` to support hot-file cache metadata while using optimized query caching.

## Scope
- `src/storage/read.rs`, specifically `get_all_files`.

## Context Summary
- Local (`HEAD`): Modified `get_all_files` SQL string to fetch `has_hot_tree` and `tree_cache` columns.
- Upstream (`upstream/main`): Changed `conn.prepare` to `conn.prepare_cached` for performance optimization.

## Implementation Plan
1. Use `conn.prepare_cached` as introduced by upstream.
2. Update the SQL string inside `prepare_cached` to include `has_hot_tree` and `tree_cache` columns as introduced by local `HEAD`.

## Acceptance criteria
- `get_all_files` executes `conn.prepare_cached(...)` with a SQL query retrieving `has_hot_tree` and `tree_cache`.
- File reads compile and tests verifying local caching pass successfully.