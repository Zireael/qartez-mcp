# Bead Review: qartez-mcp-latest-2ss.5

## Bead
**ID**: qartez-mcp-latest-2ss.5
**Title**: Bug: Auto-generate or test query_tools list completeness
**Priority**: P1 (High)
**Issue Type**: bug
**Labels**: bug, correctness, compile-time, query-tools, gating

## Manual Review (Subagents unavailable — self-analysis)

### Problem Analysis

`server/mod.rs:452` maintains a manually-managed `const` array `query_tools` that lists all tools gated by readiness. When a new query tool is added, it must be added to both the tool dispatch AND this array. There is currently no compile-time or test-time check that the two are in sync.

### Failure Modes (Self-identified)

#### #1 (HIGH): New query tool bypasses readiness gating
**Scenario**: Developer adds `qartez_new_query_tool` to the tool dispatch (`src/server/tools/mod.rs`) but forgets to add it to `query_tools()`.

**Consequence**: During `ColdStart` or `Indexing`, a client calls the new tool. It passes the readiness gate (not in `query_tools()` → treated as non-query), executes against an incomplete/stale index, returns wrong results, client acts on it.

**Detection**: Integration test would need to exhaustively call every tool during indexing and verify it's deferred. Currently no such test exists.

#### #2 (MEDIUM): Non-query tool accidentally added to `query_tools()`
**Scenario**: A mutation tool (e.g., `qartez_rename_file`) is accidentally added to `query_tools()`.

**Consequence**: During indexing, a client calls `qartez_rename_file`. It should proceed regardless of readiness (readiness gate should not block mutation tools, as they don't need an index). But if it's in `query_tools()`, it gets deferred.

**Mitigation**: The `query_tools()` list is manually curated in code. If `qartez_rename_file` is incorrectly added, someone reviewing the PR would notice. Not a high risk, but a possibility.

#### #3 (MEDIUM): Duplicate entries in `query_tools()`
**Scenario**: `qartez_map` appears twice in the list (copy-paste error).

**Consequence**: None functionally — `const` array with duplicates still compiles and works. But it confuses code review and makes the list harder to maintain.

**Mitigation**: A compile-time check using `const_assert!` or a macro that deduplicates.

### Root Cause

The `query_tools` list is a static compile-time constant that doesn't derive from the tool registration system. Rust's traits can't easily introspect on all registered tools at compile time without a procedural macro or build script.

### Review of Proposed Solutions

**Option A (Auto-generate from dispatch table)**: Would need a build script that parses `src/server/tools/mod.rs` and generates the list. Overly complex for a small list. Also, the dispatch table doesn't distinguish "query" vs "mutation" — that semantic is in `query_tools()` itself.

**Option B (Compile-time or integration test)**: Feasible. A unit test can iterate over all tool names and assert that any tool that reads from the DB is in `query_tools()`. But how does the test know which tools read from the DB? We'd need to metadata-tag each tool.

**Option C (Minimal `#[cfg(test)]` function)**: A `#[cfg(test)]` function that calls every tool during `ColdStart` and asserts none bypass gating. This is an integration-level test.

### Preferred Approach (My recommendation)

A **minimal procedural macro** or **build script** that derives `query_tools` from a trait marker:

1. Define a trait `QueryTool` for tools that are gated by readiness.
2. Add `impl QueryTool` to each query tool struct.
3. Use a procedural macro `#[derive(QueryToolRegistry)]` that collects all `impl QueryTool` types and generates the `query_tools()` function.

This is complex but ensures correctness at compile time.

Alternatively, a **simpler approach**:
1. Move the `query_tools` list to a central config file (e.g., `query_tools.json`).
2. Generate both the Rust code and the test from this file using a build script.
3. This way, tools.toml is the single source of truth.

Both approaches are over-engineered for this bead. The **most pragmatic approach** is:

1. Deprecate the `query_tools()` function entirely.
2. Add a `requires_ready_state` method to the `ToolHandler` trait:
   ```rust
   trait ToolHandler {
       fn requires_ready_state(&self) -> bool {
           true // default: yes
       }
   }
   ```
3. For mutation tools, return `false`.
4. In `call_tool`, query each tool's `requires_ready_state` dynamically.

This is cleaner and avoids manual list maintenance entirely.

### Acceptance Criteria (Updated)
- [ ] No manually-maintained `query_tools` list exists in code.
- [ ] Each tool's `requires_ready_state` is declared at point of registration (single source of truth).
- [ ] A compile-time test ensures all tools that query the DB have `requires_ready_state = true`.
- [ ] If a tool is added without setting `requires_ready_state`, the compiler or test catches it.
- [ ] All existing tests pass.
