# Readiness Signalling and Partial-Result Contracts Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add explicit readiness signalling and partial-result contracts to the qartez-mcp server, ensuring queries are deferred until the index is usable and clients receive structured retry information.

**Architecture:** A new `ReadinessState` enum tracks project state (ColdStart → Indexing → Ready → PartialReindex). State is persisted in the SQLite `meta` table and surfaced via `.qartez/status.json`. The `QartezServer::call_tool_by_name` dispatcher gates query tools behind readiness checks, returning deferred responses with `retry_after` when not ready.

**Tech Stack:** Rust, rusqlite, tokio, serde_json, rmcp (MCP framework)

---

## File Structure

| File | Responsibility |
|------|--------------|
| `src/readiness.rs` (NEW) | `ReadinessState` enum, state transitions, meta table persistence |
| `src/storage/read.rs` | Add `get_readiness()` helper (wraps `get_meta`) |
| `src/storage/write.rs` | Add `set_readiness()` helper (wraps `set_meta`) |
| `src/server/mod.rs` | Add readiness check to `call_tool_by_name`, deferred response format |
| `src/main.rs` | Set ColdStart→Indexing at startup, write `.qartez/status.json` |
| `src/index/mod.rs` | Set Indexing→Ready after `full_index_root`, Ready→PartialReindex→Ready in watcher paths |
| `src/watch.rs` | Set PartialReindex before incremental reindex, Ready after completion |
| `src/acceptance.rs` | Update existing test, add new tests for deferred responses |

---

## Task 1: Create ReadinessState Enum and Core Module

**Files:**
- Create: `src/readiness.rs`
- Modify: `src/lib.rs` (add module declaration)

**Allium Rules Covered:**
- `InitialIndexMovesProjectIntoIndexing`
- `ProjectReturnsToReadyWhenPendingWorkDrains`
- `WatcherMovesReadyProjectsIntoPartialReindex`

### Step 1.1: Write the failing test

Create `src/readiness.rs` with just the enum and a failing test:

```rust
//! Readiness state management for the qartez-mcp server.
//!
//! Tracks the project's indexing lifecycle and gates queries accordingly.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The readiness state of a project index.
///
/// Transitions:
/// - ColdStart → Indexing (when initial index begins)
/// - Indexing → Ready (when initial index completes)
/// - Ready → PartialReindex (when watcher detects changes)
/// - PartialReindex → Ready (when incremental index completes)
/// - Any → Failed (on unrecoverable error)
/// - Any → Maintenance (during manual maintenance)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    /// Project just opened, no index exists yet
    ColdStart,
    /// Initial full index in progress
    Indexing,
    /// Index is complete and usable
    Ready,
    /// Incremental reindex in progress (index is still usable)
    PartialReindex,
    /// Manual maintenance mode (queries deferred)
    Maintenance,
    /// Unrecoverable error state
    Failed,
}

impl fmt::Display for ReadinessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadinessState::ColdStart => write!(f, "cold_start"),
            ReadinessState::Indexing => write!(f, "indexing"),
            ReadinessState::Ready => write!(f, "ready"),
            ReadinessState::PartialReindex => write!(f, "partial_reindex"),
            ReadinessState::Maintenance => write!(f, "maintenance"),
            ReadinessState::Failed => write!(f, "failed"),
        }
    }
}

impl ReadinessState {
    /// Returns true if queries can be served from this state.
    ///
    /// Per Allium: `QueryServesFromReadyOrPartialIndex` - queries are served
    /// when readiness is Ready or PartialReindex.
    pub fn is_queryable(&self) -> bool {
        matches!(self, ReadinessState::Ready | ReadinessState::PartialReindex)
    }

    /// Returns true if queries should be deferred.
    ///
    /// Per Allium: `QueryDefersUntilIndexIsUsable` - queries are deferred
    /// when readiness is ColdStart, Indexing, or Maintenance.
    pub fn should_defer(&self) -> bool {
        matches!(
            self,
            ReadinessState::ColdStart | ReadinessState::Indexing | ReadinessState::Maintenance
        )
    }

    /// Returns the retry_after duration in seconds for deferred responses.
    pub fn retry_after_secs(&self) -> u64 {
        // Per Allium config: readiness_retry_after = 2.seconds
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_readiness_state_is_queryable() {
        assert!(!ReadinessState::ColdStart.is_queryable());
        assert!(!ReadinessState::Indexing.is_queryable());
        assert!(ReadinessState::Ready.is_queryable());
        assert!(ReadinessState::PartialReindex.is_queryable());
        assert!(!ReadinessState::Maintenance.is_queryable());
        assert!(!ReadinessState::Failed.is_queryable());
    }

    #[test]
    fn test_readiness_state_should_defer() {
        assert!(ReadinessState::ColdStart.should_defer());
        assert!(ReadinessState::Indexing.should_defer());
        assert!(!ReadinessState::Ready.should_defer());
        assert!(!ReadinessState::PartialReindex.should_defer());
        assert!(ReadinessState::Maintenance.should_defer());
        assert!(!ReadinessState::Failed.should_defer()); // Failed is not deferrable, it's an error
    }

    #[test]
    fn test_readiness_state_display() {
        assert_eq!(ReadinessState::ColdStart.to_string(), "cold_start");
        assert_eq!(ReadinessState::Indexing.to_string(), "indexing");
        assert_eq!(ReadinessState::Ready.to_string(), "ready");
        assert_eq!(ReadinessState::PartialReindex.to_string(), "partial_reindex");
        assert_eq!(ReadinessState::Maintenance.to_string(), "maintenance");
        assert_eq!(ReadinessState::Failed.to_string(), "failed");
    }

    #[test]
    fn test_readiness_state_serde() {
        // Test serialization round-trip
        let states = vec![
            ReadinessState::ColdStart,
            ReadinessState::Indexing,
            ReadinessState::Ready,
            ReadinessState::PartialReindex,
            ReadinessState::Maintenance,
            ReadinessState::Failed,
        ];

        for state in states {
            let json = serde_json::to_string(&state).unwrap();
            let deserialized: ReadinessState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, deserialized);
        }
    }
}
```

Add to `src/lib.rs` (find the existing mod declarations around line 20-30):

```rust
pub mod readiness;
```

### Step 1.2: Run test to verify it fails

```bash
cargo test --lib readiness::tests -- --nocapture
```

**Expected:** FAIL - module not found or tests not found

### Step 1.3: Verify module is included

Check `src/lib.rs` has the module declaration. The enum and tests are already complete in the file above.

### Step 1.4: Run test to verify it passes

```bash
cargo test --lib readiness::tests -- --nocapture
```

**Expected:** PASS - 4 tests pass

### Step 1.5: Commit

```bash
git add src/readiness.rs src/lib.rs
git commit -m "feat: add ReadinessState enum with queryable/defer logic"
```

---

## Task 2: Add Storage Helpers for Readiness Persistence

**Files:**
- Modify: `src/storage/read.rs:828-835` (after `get_meta`)
- Modify: `src/storage/write.rs:441-448` (after `set_meta`)

**Allium Rules Covered:**
- State persistence via meta table

### Step 2.1: Write the failing test

Add to `src/storage/read.rs` after line 835:

```rust
/// Get the current readiness state from the meta table.
/// Returns ColdStart if no readiness key exists.
pub fn get_readiness(conn: &Connection) -> Result<crate::readiness::ReadinessState> {
    match get_meta(conn, "readiness")? {
        Some(value) => {
            let state: crate::readiness::ReadinessState = serde_json::from_str(&value)
                .map_err(|e| crate::storage::StorageError::Other(format!("invalid readiness state: {e}")))?;
            Ok(state)
        }
        None => Ok(crate::readiness::ReadinessState::ColdStart),
    }
}
```

Add to `src/storage/write.rs` after line 448:

```rust
/// Persist the readiness state to the meta table.
pub fn set_readiness(conn: &Connection, state: &crate::readiness::ReadinessState) -> Result<()> {
    let value = serde_json::to_string(state)
        .map_err(|e| crate::storage::StorageError::Other(format!("failed to serialize readiness: {e}")))?;
    set_meta(conn, "readiness", &value)
}
```

Add test to `src/acceptance.rs` (after line 158):

```rust
/// Maps to **Rule: InitialIndexMovesProjectIntoIndexing**
///
/// When a project is first opened, readiness should be ColdStart.
/// After indexing begins, it should transition to Indexing.
#[test]
fn rule_initial_readiness_is_cold_start() {
    let conn = test_db();
    
    // Initially, readiness should be ColdStart (no entry in meta)
    let readiness = read::get_readiness(&conn).unwrap();
    assert_eq!(readiness, qartez_mcp::readiness::ReadinessState::ColdStart);
}
```

### Step 2.2: Run test to verify it fails

```bash
cargo test --lib acceptance::rule_initial_readiness_is_cold_start -- --nocapture
```

**Expected:** FAIL - function `get_readiness` not found

### Step 2.3: Write minimal implementation

The code snippets above are the full implementation. Ensure imports are correct:

In `src/storage/read.rs`, add to the top (check existing imports):
```rust
use crate::readiness::ReadinessState;
```

In `src/storage/write.rs`, add to the top:
```rust
use crate::readiness::ReadinessState;
```

### Step 2.4: Run test to verify it passes

```bash
cargo test --lib acceptance::rule_initial_readiness_is_cold_start -- --nocapture
```

**Expected:** PASS

### Step 2.5: Commit

```bash
git add src/storage/read.rs src/storage/write.rs src/acceptance.rs
git commit -m "feat: add get_readiness/set_readiness storage helpers"
```

---

## Task 3: Add Readiness Check to Server Tool Dispatch

**Files:**
- Modify: `src/server/mod.rs:259-311` (`call_tool_by_name`)

**Allium Rules Covered:**
- `QueryDefersUntilIndexIsUsable`
- `QueryServesFromReadyOrPartialIndex`

### Step 3.1: Write the failing test

Add to `src/acceptance.rs` (after the previous test):

```rust
/// Maps to **Rule: QueryDefersUntilIndexIsUsable**
///
/// When readiness is ColdStart, queries should be deferred with retry_after.
#[test]
fn rule_query_deferred_when_cold_start() {
    let conn = test_db();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    
    // Create server without indexing
    let server = server::QartezServer::new(conn, root.clone(), 10);
    
    // Query should be deferred
    let result = server.call_tool_by_name("qartez_map", serde_json::json!({}));
    
    // Should return a deferred response, not an error
    assert!(result.is_ok(), "query should return Ok with deferred status");
    let response = result.unwrap();
    assert!(response.contains("deferred"), "response should indicate deferral: {response}");
    assert!(response.contains("retry_after"), "response should include retry_after: {response}");
}
```

### Step 3.2: Run test to verify it fails

```bash
cargo test --lib acceptance::rule_query_deferred_when_cold_start -- --nocapture
```

**Expected:** FAIL - test may pass if server doesn't check readiness yet (we'll verify)

### Step 3.3: Implement readiness check in server

Modify `src/server/mod.rs`:

First, add import at the top of the file (around line 1-20):
```rust
use crate::readiness::{ReadinessState, self};
```

Add a helper method to `QartezServer` impl (after line 109, before `tool_router`):

```rust
    /// Check the current readiness state by reading from the database.
    fn get_readiness(&self) -> Result<ReadinessState, String> {
        let conn = self.db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
        crate::storage::read::get_readiness(&conn)
            .map_err(|e| format!("failed to read readiness: {e}"))
    }

    /// Build a deferred response JSON for when queries cannot be served.
    fn build_deferred_response(&self, state: ReadinessState) -> String {
        serde_json::json!({
            "status": "deferred",
            "readiness": state,
            "retry_after": state.retry_after_secs(),
            "message": format!("Index not ready: {}. Retry after {} seconds.", state, state.retry_after_secs()),
        }).to_string()
    }
```

Now modify `call_tool_by_name` (around line 259-311). The current function returns `Result<String, String>`. We need to add a readiness check at the start. Replace the function body:

```rust
    pub fn call_tool_by_name(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> std::result::Result<String, String> {
        let args = if args.is_null() {
            serde_json::json!({})
        } else {
            args
        };

        // Check readiness before dispatching query tools
        // Per Allium: QueryDefersUntilIndexIsUsable
        let query_tools = [
            "qartez_map", "qartez_find", "qartez_read", "qartez_grep",
            "qartez_refs", "qartez_calls", "qartez_deps", "qartez_impact",
            "qartez_diff_impact", "qartez_cochange", "qartez_unused",
            "qartez_outline", "qartez_stats", "qartez_context",
            "qartez_wiki", "qartez_hotspots", "qartez_clones",
            "qartez_smells", "qartez_health", "qartez_refactor_plan",
            "qartez_test_gaps", "qartez_boundaries", "qartez_hierarchy",
            "qartez_trend", "qartez_security", "qartez_semantic",
            "qartez_knowledge", "qartez_rename", "qartez_move",
            "qartez_rename_file", "qartez_replace_symbol",
            "qartez_insert_before_symbol", "qartez_insert_after_symbol",
            "qartez_safe_delete",
        ];

        if query_tools.contains(&name) {
            match self.get_readiness() {
                Ok(state) if state.should_defer() => {
                    return Ok(self.build_deferred_response(state));
                }
                Ok(_) => {
                    // Ready or PartialReindex - proceed with query
                }
                Err(e) => {
                    return Err(format!("readiness check failed: {e}"));
                }
            }
        }

        dispatch_tool_call!(self, name, args,
            // ... rest of existing dispatch_tool_call! macro invocation
```

The full replacement for `call_tool_by_name`:

```rust
    pub fn call_tool_by_name(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> std::result::Result<String, String> {
        let args = if args.is_null() {
            serde_json::json!({})
        } else {
            args
        };

        // Check readiness before dispatching query tools
        // Per Allium: QueryDefersUntilIndexIsUsable
        let query_tools = [
            "qartez_map", "qartez_find", "qartez_read", "qartez_grep",
            "qartez_refs", "qartez_calls", "qartez_deps", "qartez_impact",
            "qartez_diff_impact", "qartez_cochange", "qartez_unused",
            "qartez_outline", "qartez_stats", "qartez_context",
            "qartez_wiki", "qartez_hotspots", "qartez_clones",
            "qartez_smells", "qartez_health", "qartez_refactor_plan",
            "qartez_test_gaps", "qartez_boundaries", "qartez_hierarchy",
            "qartez_trend", "qartez_security", "qartez_semantic",
            "qartez_knowledge", "qartez_rename", "qartez_move",
            "qartez_rename_file", "qartez_replace_symbol",
            "qartez_insert_before_symbol", "qartez_insert_after_symbol",
            "qartez_safe_delete",
        ];

        if query_tools.contains(&name) {
            match self.get_readiness() {
                Ok(state) if state.should_defer() => {
                    return Ok(self.build_deferred_response(state));
                }
                Ok(_) => {
                    // Ready or PartialReindex - proceed with query
                }
                Err(e) => {
                    return Err(format!("readiness check failed: {e}"));
                }
            }
        }

        dispatch_tool_call!(self, name, args,
            infallible {
            }
            fallible {
                "qartez_map" => qartez_map: QartezParams,
                "qartez_find" => qartez_find: SoulFindParams,
                "qartez_workspace" => qartez_workspace: SoulWorkspaceParams,
                "qartez_read" => qartez_read: SoulReadParams,
                "qartez_impact" => qartez_impact: SoulImpactParams,
                "qartez_diff_impact" => qartez_diff_impact: SoulDiffImpactParams,
                "qartez_cochange" => qartez_cochange: SoulCochangeParams,
                "qartez_grep" => qartez_grep: SoulGrepParams,
                "qartez_unused" => qartez_unused: SoulUnusedParams,
                "qartez_refs" => qartez_refs: SoulRefsParams,
                "qartez_rename" => qartez_rename: SoulRenameParams,
                "qartez_project" => qartez_project: SoulProjectParams,
                "qartez_move" => qartez_move: SoulMoveParams,
                "qartez_rename_file" => qartez_rename_file: SoulRenameFileParams,
                "qartez_outline" => qartez_outline: SoulOutlineParams,
                "qartez_deps" => qartez_deps: SoulDepsParams,
                "qartez_stats" => qartez_stats: SoulStatsParams,
                "qartez_calls" => qartez_calls: SoulCallsParams,
                "qartez_context" => qartez_context: SoulContextParams,
                "qartez_wiki" => qartez_wiki: SoulWikiParams,
                "qartez_hotspots" => qartez_hotspots: SoulHotspotsParams,
                "qartez_clones" => qartez_clones: SoulClonesParams,
                "qartez_smells" => qartez_smells: SoulSmellsParams,
                "qartez_health" => qartez_health: SoulHealthParams,
                "qartez_refactor_plan" => qartez_refactor_plan: SoulRefactorPlanParams,
                "qartez_test_gaps" => qartez_test_gaps: SoulTestGapsParams,
                "qartez_boundaries" => qartez_boundaries: SoulBoundariesParams,
                "qartez_hierarchy" => qartez_hierarchy: SoulHierarchyParams,
                "qartez_trend" => qartez_trend: SoulTrendParams,
                "qartez_security" => qartez_security: SoulSecurityParams,
                "qartez_semantic" => qartez_semantic: SemanticParams,
                "qartez_knowledge" => qartez_knowledge: SoulKnowledgeParams,
                "qartez_replace_symbol" => qartez_replace_symbol: SoulReplaceSymbolParams,
                "qartez_insert_before_symbol" => qartez_insert_before_symbol: SoulInsertSymbolParams,
                "qartez_insert_after_symbol" => qartez_insert_after_symbol: SoulInsertSymbolParams,
                "qartez_safe_delete" => qartez_safe_delete: SoulSafeDeleteParams,
            }
        )
    }
```

### Step 3.4: Run test to verify it passes

```bash
cargo test --lib acceptance::rule_query_deferred_when_cold_start -- --nocapture
```

**Expected:** PASS

### Step 3.5: Commit

```bash
git add src/server/mod.rs src/acceptance.rs
git commit -m "feat: gate query tools behind readiness check with deferred responses"
```

---

## Task 4: Set Readiness Transitions in Main.rs

**Files:**
- Modify: `src/main.rs:104-163` (startup and indexing flow)

**Allium Rules Covered:**
- `InitialIndexMovesProjectIntoIndexing`
- Status file writing

### Step 4.1: Write the failing test

Add to `src/acceptance.rs`:

```rust
/// Maps to **Rule: InitialIndexMovesProjectIntoIndexing**
///
/// After indexing completes, readiness should be Ready.
#[test]
fn rule_readiness_ready_after_full_index() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);
    
    // Readiness should be Ready after full index
    let readiness = read::get_readiness(&conn).unwrap();
    assert_eq!(readiness, qartez_mcp::readiness::ReadinessState::Ready);
}
```

### Step 4.2: Run test to verify it fails

```bash
cargo test --lib acceptance::rule_readiness_ready_after_full_index -- --nocapture
```

**Expected:** FAIL - readiness is still ColdStart (indexer doesn't set it yet)

### Step 4.3: Implement readiness transitions in main.rs

Modify `src/main.rs`:

Add import at the top:
```rust
use qartez_mcp::readiness::ReadinessState;
use qartez_mcp::storage::write::set_readiness;
```

After line 104 (before the background task spawn), add:

```rust
        // Set initial readiness to ColdStart, then transition to Indexing
        // Per Allium: InitialIndexMovesProjectIntoIndexing
        if let Err(e) = set_readiness(&conn, &ReadinessState::ColdStart) {
            tracing::warn!("failed to set initial readiness: {e}");
        }
```

Inside the `spawn_blocking` closure (after line 117, before indexing starts), add:

```rust
            // Transition to Indexing state
            if let Err(e) = set_readiness(&conn, &ReadinessState::Indexing) {
                tracing::warn!("background indexer: failed to set indexing readiness: {e}");
            }
```

After the indexing completes (after line 150, before the closing `});`), add:

```rust
            // Transition to Ready state
            if let Err(e) = set_readiness(&conn, &ReadinessState::Ready) {
                tracing::warn!("background indexer: failed to set ready readiness: {e}");
            }
```

### Step 4.4: Run test to verify it passes

```bash
cargo test --lib acceptance::rule_readiness_ready_after_full_index -- --nocapture
```

**Expected:** PASS

### Step 4.5: Commit

```bash
git add src/main.rs src/acceptance.rs
git commit -m "feat: set ColdStart→Indexing→Ready transitions in main.rs"
```

---

## Task 5: Set Readiness Transitions in Index Module

**Files:**
- Modify: `src/index/mod.rs:580-605` (full_index_root completion)
- Modify: `src/index/mod.rs:1825-1851` (incremental_index_with_prefix)

**Allium Rules Covered:**
- `ProjectReturnsToReadyWhenPendingWorkDrains`

### Step 5.1: Write the failing test

The acceptance test from Task 4 already covers this. We'll verify the indexer sets readiness.

### Step 5.2: Run test to verify current state

```bash
cargo test --lib acceptance::rule_readiness_ready_after_full_index -- --nocapture
```

**Expected:** Currently PASS (from main.rs changes), but we need the indexer to also set it

### Step 5.3: Implement readiness in full_index_root

Modify `src/index/mod.rs` at line 591 (after `set_meta` for last_index):

```rust
        // Set readiness to Ready after full index completes
        // Per Allium: ProjectReturnsToReadyWhenPendingWorkDrains
        write::set_readiness(&tx, &crate::readiness::ReadinessState::Ready)?;
```

### Step 5.4: Implement readiness in incremental_index_with_prefix

Modify `src/index/mod.rs` at line 1834 (after `set_meta` for last_index):

```rust
        // Set readiness to Ready after incremental index completes
        // Per Allium: ProjectReturnsToReadyWhenPendingWorkDrains
        write::set_readiness(&tx, &crate::readiness::ReadinessState::Ready)?;
```

### Step 5.5: Run test to verify it passes

```bash
cargo test --lib acceptance::rule_readiness_ready_after_full_index -- --nocapture
```

**Expected:** PASS

### Step 5.6: Commit

```bash
git add src/index/mod.rs
git commit -m "feat: set Ready readiness in full_index and incremental_index"
```

---

## Task 6: Set Readiness Transitions in Watcher

**Files:**
- Modify: `src/watch.rs:114-139` (reindex method)

**Allium Rules Covered:**
- `WatcherMovesReadyProjectsIntoPartialReindex`
- `ProjectReturnsToReadyWhenPendingWorkDrains`

### Step 6.1: Write the failing test

Add to `src/acceptance.rs`:

```rust
/// Maps to **Rule: WatcherMovesReadyProjectsIntoPartialReindex**
///
/// When a watch event triggers reindex, readiness should transition
/// to PartialReindex and back to Ready when complete.
#[test]
fn rule_watcher_transitions_to_partial_reindex() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);
    
    // Verify initial state is Ready
    let readiness = read::get_readiness(&conn).unwrap();
    assert_eq!(readiness, qartez_mcp::readiness::ReadinessState::Ready);
    
    // Simulate what the watcher does: set PartialReindex before incremental
    write::set_readiness(&conn, &qartez_mcp::readiness::ReadinessState::PartialReindex).unwrap();
    let readiness = read::get_readiness(&conn).unwrap();
    assert_eq!(readiness, qartez_mcp::readiness::ReadinessState::PartialReindex);
    
    // After incremental index completes, should be Ready again
    // (This is tested by the incremental_index function itself)
}
```

### Step 6.2: Run test to verify it passes

```bash
cargo test --lib acceptance::rule_watcher_transitions_to_partial_reindex -- --nocapture
```

**Expected:** PASS (tests the storage helpers, not watcher logic yet)

### Step 6.3: Implement readiness in watcher

Modify `src/watch.rs`:

Add import at the top:
```rust
use crate::readiness::ReadinessState;
use crate::storage::write::set_readiness;
```

Modify the `reindex` method (around line 114-139):

```rust
    fn reindex(&self, changed: &[PathBuf], deleted: &[PathBuf]) -> anyhow::Result<()> {
        // Mirror the `into_inner()` recovery already used by the ignore-cache
        // lock at start_notify_watcher: a poisoned db mutex means a prior
        // indexing operation panicked mid-way, but the Connection is still
        // usable (sqlite rolls the open transaction back when the guard drops).
        // Panicking here would kill the watcher task for the rest of the
        // session - a long-running background loop should recover from a
        // one-off parse or encode panic instead of going silent.
        let conn = match self.db.lock() {
            Ok(g) => g,
            Err(poisoned) => {
                tracing::warn!("watcher db mutex was poisoned; recovering");
                poisoned.into_inner()
            }
        };
        
        // Transition to PartialReindex before starting
        // Per Allium: WatcherMovesReadyProjectsIntoPartialReindex
        if let Err(e) = set_readiness(&conn, &ReadinessState::PartialReindex) {
            tracing::warn!("watcher: failed to set partial_reindex readiness: {e}");
        }
        
        let result = index::incremental_index_with_prefix(
            &conn,
            &self.project_root,
            &self.path_prefix,
            changed,
            deleted,
        );
        
        // incremental_index_with_prefix sets readiness back to Ready
        // Per Allium: ProjectReturnsToReadyWhenPendingWorkDrains
        if let Err(e) = result {
            // On error, try to set Failed state
            let _ = set_readiness(&conn, &ReadinessState::Failed);
            return Err(e.into());
        }
        
        graph::pagerank::compute_pagerank(&conn, &Default::default())?;
        graph::pagerank::compute_symbol_pagerank(&conn, &Default::default())?;
        Ok(())
    }
```

### Step 6.4: Run test to verify it passes

```bash
cargo test --lib acceptance::rule_watcher_transitions_to_partial_reindex -- --nocapture
```

**Expected:** PASS

### Step 6.5: Commit

```bash
git add src/watch.rs src/acceptance.rs
git commit -m "feat: set PartialReindex→Ready transitions in file watcher"
```

---

## Task 7: Write Status File from Rust

**Files:**
- Modify: `src/main.rs` (add status file writing)
- Create: Helper function for status file

**Allium Rules Covered:**
- External status visibility

### Step 7.1: Write the failing test

Add to `src/acceptance.rs`:

```rust
/// Verify that status file can be written and read back.
#[test]
fn test_status_file_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let status_path = tmp.path().join("status.json");
    
    let status = qartez_mcp::readiness::StatusFile {
        readiness: qartez_mcp::readiness::ReadinessState::Ready,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    };
    
    // Write status file
    let json = serde_json::to_string_pretty(&status).unwrap();
    std::fs::write(&status_path, json).unwrap();
    
    // Read it back
    let content = std::fs::read_to_string(&status_path).unwrap();
    let parsed: qartez_mcp::readiness::StatusFile = serde_json::from_str(&content).unwrap();
    
    assert_eq!(parsed.readiness, status.readiness);
}
```

### Step 7.2: Run test to verify it fails

```bash
cargo test --lib acceptance::test_status_file_roundtrip -- --nocapture
```

**Expected:** FAIL - StatusFile struct doesn't exist

### Step 7.3: Implement StatusFile struct and helper

Add to `src/readiness.rs` (after the ReadinessState impl):

```rust
/// Status file format for external visibility.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusFile {
    pub readiness: ReadinessState,
    pub timestamp: u64,
}

impl StatusFile {
    /// Create a new status file with the current state.
    pub fn new(readiness: ReadinessState) -> Self {
        Self {
            readiness,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }

    /// Write the status file to the given path.
    pub fn write_to(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Read the status file from the given path.
    pub fn read_from(path: &std::path::Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let status = serde_json::from_str(&content)?;
        Ok(status)
    }
}
```

### Step 7.4: Run test to verify it passes

```bash
cargo test --lib acceptance::test_status_file_roundtrip -- --nocapture
```

**Expected:** PASS

### Step 7.5: Add status file writing to main.rs

Modify `src/main.rs`:

Add helper function (after `schedule_update_check`):

```rust
/// Write the status file to `.qartez/status.json`.
fn write_status_file(readiness: ReadinessState, db_path: &std::path::Path) -> anyhow::Result<()> {
    let qartez_dir = db_path.parent().ok_or_else(|| anyhow::anyhow!("db_path has no parent"))?;
    let status_path = qartez_dir.join("status.json");
    let status = qartez_mcp::readiness::StatusFile::new(readiness);
    status.write_to(&status_path)?;
    tracing::debug!("wrote status file: {:?}", status_path);
    Ok(())
}
```

Add calls to `write_status_file` at each readiness transition point:

1. After setting ColdStart (line ~105):
```rust
        if let Err(e) = set_readiness(&conn, &ReadinessState::ColdStart) {
            tracing::warn!("failed to set initial readiness: {e}");
        }
        if let Err(e) = write_status_file(ReadinessState::ColdStart, &config.db_path) {
            tracing::warn!("failed to write initial status file: {e}");
        }
```

2. Inside spawn_blocking, after setting Indexing:
```rust
            if let Err(e) = set_readiness(&conn, &ReadinessState::Indexing) {
                tracing::warn!("background indexer: failed to set indexing readiness: {e}");
            }
            // Note: status file can't be written here easily since we're in a blocking task
            // The external script will still write it, or we can use a channel
```

3. After setting Ready at the end of spawn_blocking:
```rust
            if let Err(e) = set_readiness(&conn, &ReadinessState::Ready) {
                tracing::warn!("background indexer: failed to set ready readiness: {e}");
            }
            // Write status file on completion
            if let Err(e) = write_status_file(ReadinessState::Ready, &db_path) {
                tracing::warn!("background indexer: failed to write ready status file: {e}");
            }
```

### Step 7.6: Commit

```bash
git add src/readiness.rs src/main.rs src/acceptance.rs
git commit -m "feat: add StatusFile struct and write status.json from Rust"
```

---

## Task 8: Update Acceptance Tests for Deferred Responses

**Files:**
- Modify: `src/acceptance.rs` (add comprehensive tests)

**Allium Rules Covered:**
- `QueryDefersUntilIndexIsUsable`
- `QueryServesFromReadyOrPartialIndex`

### Step 8.1: Write comprehensive acceptance tests

Add to `src/acceptance.rs`:

```rust
// ===========================================================================
// INVARIANT: QueryDefersUntilIndexIsUsable
// ===========================================================================
//
// Allium: "when readiness in [cold_start, indexing, maintenance], defer with retry_after=2s"

/// Maps to **Rule: QueryDefersUntilIndexIsUsable**
///
/// When readiness is Indexing, queries should be deferred.
#[test]
fn rule_query_deferred_when_indexing() {
    let conn = test_db();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    
    // Set readiness to Indexing
    write::set_readiness(&conn, &qartez_mcp::readiness::ReadinessState::Indexing).unwrap();
    
    // Create server
    let server = server::QartezServer::new(conn, root.clone(), 10);
    
    // Query should be deferred
    let result = server.call_tool_by_name("qartez_map", serde_json::json!({}));
    
    assert!(result.is_ok(), "query should return Ok with deferred status");
    let response = result.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["status"], "deferred");
    assert_eq!(parsed["retry_after"], 2);
}

/// Maps to **Rule: QueryServesFromReadyOrPartialIndex**
///
/// When readiness is Ready, queries should be served normally.
#[test]
fn rule_query_served_when_ready() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);
    
    // Verify readiness is Ready
    let readiness = read::get_readiness(&conn).unwrap();
    assert_eq!(readiness, qartez_mcp::readiness::ReadinessState::Ready);
    
    // Create server
    let server = server::QartezServer::new(conn, root.clone(), 10);
    
    // Query should be served (not deferred)
    let result = server.call_tool_by_name("qartez_map", serde_json::json!({}));
    
    assert!(result.is_ok(), "query should succeed");
    let response = result.unwrap();
    // Should NOT be a deferred response
    assert!(!response.contains("\"status\":\"deferred\""), "response should not be deferred: {response}");
    // Should contain actual results
    assert!(response.contains("files") || response.contains("symbols") || response.contains("ranked"), 
            "response should contain results: {response}");
}

/// Maps to **Rule: QueryServesFromReadyOrPartialIndex**
///
/// When readiness is PartialReindex, queries should still be served.
#[test]
fn rule_query_served_when_partial_reindex() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);
    
    // Manually set to PartialReindex
    write::set_readiness(&conn, &qartez_mcp::readiness::ReadinessState::PartialReindex).unwrap();
    
    // Create server
    let server = server::QartezServer::new(conn, root.clone(), 10);
    
    // Query should be served (not deferred)
    let result = server.call_tool_by_name("qartez_map", serde_json::json!({}));
    
    assert!(result.is_ok(), "query should succeed");
    let response = result.unwrap();
    // Should NOT be a deferred response
    assert!(!response.contains("\"status\":\"deferred\""), "response should not be deferred: {response}");
}

/// Maps to **Rule: QueryDefersUntilIndexIsUsable**
///
/// When readiness is Maintenance, queries should be deferred.
#[test]
fn rule_query_deferred_when_maintenance() {
    let conn = test_db();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    
    // Set readiness to Maintenance
    write::set_readiness(&conn, &qartez_mcp::readiness::ReadinessState::Maintenance).unwrap();
    
    // Create server
    let server = server::QartezServer::new(conn, root.clone(), 10);
    
    // Query should be deferred
    let result = server.call_tool_by_name("qartez_map", serde_json::json!({}));
    
    assert!(result.is_ok(), "query should return Ok with deferred status");
    let response = result.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert_eq!(parsed["status"], "deferred");
}
```

### Step 8.2: Run all new tests

```bash
cargo test --lib acceptance::rule_query_deferred_when_indexing acceptance::rule_query_served_when_ready acceptance::rule_query_served_when_partial_reindex acceptance::rule_query_deferred_when_maintenance -- --nocapture
```

**Expected:** All PASS

### Step 8.3: Commit

```bash
git add src/acceptance.rs
git commit -m "test: add comprehensive acceptance tests for readiness gating"
```

---

## Task 9: Update Existing Acceptance Test

**Files:**
- Modify: `src/acceptance.rs:106-135` (update `invariant_queries_only_served_from_usable_indexes_after_full_index`)

### Step 9.1: Update the existing test

Modify the existing test to also verify readiness state:

```rust
/// Maps to **Invariant: QueriesAreOnlyServedFromUsableIndexes**
///
/// After a full index completes, the DB must be in a usable state:
/// schema exists, files are present, and readiness is Ready.
#[test]
fn invariant_queries_only_served_from_usable_indexes_after_full_index() {
    let (_tmp, root) = rust_project();
    let conn = index_rust_project(&root);

    // Schema must exist
    let table_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(table_count >= 5, "DB must have core tables after indexing");

    // Files must exist
    let file_count = read::get_file_count(&conn).unwrap();
    assert!(
        file_count > 0,
        "DB must contain indexed files after full index"
    );

    // No stale files (files with zero mtime indicate they were never
    // actually written during the index pass)
    let stale = read::get_stale_files(&conn).unwrap();
    assert!(
        stale.is_empty(),
        "no files should be stale immediately after a fresh full index, found: {:?}",
        stale.iter().map(|f| &f.path).collect::<Vec<_>>()
    );
    
    // Readiness must be Ready
    let readiness = read::get_readiness(&conn).unwrap();
    assert!(
        readiness.is_queryable(),
        "readiness must be queryable after full index, got: {:?}",
        readiness
    );
}
```

### Step 9.2: Run the updated test

```bash
cargo test --lib acceptance::invariant_queries_only_served_from_usable_indexes_after_full_index -- --nocapture
```

**Expected:** PASS

### Step 9.3: Commit

```bash
git add src/acceptance.rs
git commit -m "test: update existing acceptance test to verify readiness state"
```

---

## Task 10: Full Validation

### Step 10.1: Run full test suite

```bash
cargo test --lib -- --nocapture
```

**Expected:** All tests pass

### Step 10.2: Run clippy

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

**Expected:** No warnings

### Step 10.3: Run fmt check

```bash
cargo fmt --all -- --check
```

**Expected:** No formatting issues

### Step 10.4: Build release

```bash
cargo build --release
```

**Expected:** Clean build

### Step 10.5: Final commit

```bash
git add -A
git commit -m "feat: complete readiness signalling and partial-result contracts"
```

---

## Summary

This implementation adds:

1. **ReadinessState enum** (`src/readiness.rs`) - Tracks ColdStart, Indexing, Ready, PartialReindex, Maintenance, Failed states
2. **Storage helpers** (`src/storage/read.rs`, `src/storage/write.rs`) - Persist/read readiness from meta table
3. **Query gating** (`src/server/mod.rs`) - Defers queries when not ready, returns structured response with retry_after
4. **Transition hooks** - Sets readiness at startup, after indexing, and during watcher reindex
5. **Status file** (`src/readiness.rs`, `src/main.rs`) - Writes `.qartez/status.json` from Rust
6. **Acceptance tests** - Comprehensive tests for all Allium rules

All changes are minimal and lane-focused, preserving local-first behavior and not introducing any mandatory external services.
