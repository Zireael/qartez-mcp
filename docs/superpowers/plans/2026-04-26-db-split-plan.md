# DB Split Implementation Plan: Writer State Visibility

## Overview

Make the SQLite writer state visible to queries by extending the existing readiness signalling. Background indexing already uses a separate connection - what's needed is exposing writer_state in queries.

## Current State

1. **Background indexing** already uses separate connection (main.rs L125)
2. **Readiness signalling** implemented - ColdStart → Indexing → Ready transitions
3. **Query deferral** works - queries return deferred during Indexing/ColdStart
4. **QartezServer** holds single `db: Arc<Mutex<Connection>>`

## What's Needed

- Add `writer_state` to meta table tracking
- Return writer_state in deferred/proxy responses
- Ensure queries see state even during background writes

## Implementation Units

- [ ] U1. Add writer_state column to meta table

**Files:**
- Modify: `src/storage/schema.rs` (add `writer_state` column)
- Modify: `src/storage/write.rs` (add `set_writer_state`)
- Modify: `src/storage/read.rs` (add `get_writer_state`)

**Test scenarios:**
- Happy path: writer_state transitions from idle → full_indexing → idle

- [ ] U2. Include writer_state in deferred response

**Files:**
- Modify: `src/server/mod.rs` (build_deferred_response includes writer_state)
- Modify: `src/acceptance.rs` (add test for writer_state in response)

**Test scenarios:**
- Happy path: deferred response contains writer_state field

- [ ] U3. Set writer_state during background indexing

**Files:**
- Modify: `src/main.rs` (set writer_state at index start/complete)

**Test scenarios:**
- Happy path: writer_state is "full_indexing" during background index

- [ ] U4. Set writer_state during watcher writes

**Files:**
- Modify: `src/watch.rs` (set writer_state on batch start/complete)

**Test scenarios:**
- Happy path: writer_state is "incremental_indexing" during watch batch

## Key Decisions

- **Use meta table, not new table**: Already have key-value pattern for readiness
- **writer_state values**: idle | full_indexing | incremental_indexing | pruning | compacting | blocked
- **Visibility**: Returned in deferred responses, readable via get_meta

## Verification

- cargo fmt, clippy, release build pass
- 1430+ tests pass
- Acceptance test verifies writer_state in deferred response