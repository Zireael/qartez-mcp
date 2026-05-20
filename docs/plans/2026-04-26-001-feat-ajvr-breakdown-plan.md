---
title: ajvr watcher chunking breakdown + priority implementation plan
type: feat
status: active
date: 2026-04-26
---

# ajvr Watcher Chunking Breakdown + Priority Implementation Plan

## Overview

Break down ajvr "Full implement" into atomic beads, then create priority order for all remaining open implementation beads.

---

## Problem Frame

The Allium spec lists 5 major indexing/runtime improvements. Prior session confirmed eligibility is already unified (watcher uses shared languages module). Remaining work: chunking config + chunked reindex loop + reader yield.

The original ajvr bead has 3 sub-parts:

1. **Eligibility check** - VERIFIED working (watcher uses languages module, 9 tests pass)
2. **Write chunking** - NEEDS: add writer_chunk_size config + chunked reindex loop
3. **Yield points** - NEEDS: tokio::task::yield_now between chunks

One ajvr contains too much work for a single bead. Need to split into atomic pieces with proper dependencies.

---

## Bead Priority Order

Determined from dependency analysis + Allium spec success criteria:

| Priority | Bead ID | Title | Dependencies | Notes |
|----------|---------|-------|--------------|-------|
| 1 | k22b | Parser workers | (none - foundational) | Lazy-loaded, avoids global bottleneck |
| 2 | ajvr-chunk | Write chunking | k22b | writer_chunk_size config + chunk loop |
| 3 | ajvr-yield | Reader yield | ajvr-chunk | tokio::task::yield_now |
| 4 | ajvr-elig | Watcher eligibility | (none - already done) | VERIFIED - pending close |
| 5 | 45ps | Hot-file reparsing | ajvr-(chunk+yield) | tree caching + incremental parse |
| 6 | jxqk | Parallel index | (none - already done) | ac7386d in prior session |
| 7 | i9sy | Finalize + docs | (all above) | Last - after all impl |

---

## ajvr Sub-Beads Creation

### ajvr-elig (IN PROGRESS → COMPLETE)

**Goal:** Verify watcher eligibility matches indexer

**Status:** COMPLETE - Analysis confirmed watcher uses shared languages module at runtime. 9 watch tests pass. Close bead with completion note.

**Dependencies:** None

**Files:** None (existing tests verify)

**Verification:** 9 watch eligibility tests pass

---

### ajvr-chunk (NEW - PRIORITY 2)

**Goal:** Add writer_chunk_size config + implement chunked reindex loop in watcher

**Requirements:**

- R1: Write chunks bounded by config.writer_chunk_size (50 from acceptance.rs L420)
- R2: Ready parse tasks join chunks until size limit reached
- R3: Each chunk committed separately

**Dependencies:** k22b (parser workers provide parsing)

**Files:**

- Modify: `src/config.rs` - add writer_chunk_size field
- Modify: `src/watch.rs` - Chunk struct + chunked reindex loop
- Test: `src/watch.rs` - chunk creation/assignment tests

**Approach:**

1. Add `writer_chunk_size: Option<usize>` to Config (default 50)
2. Create `WriteChunk` struct with file batch + metadata
3. Modify watcher event handling to batch into chunks
4. Add chunk commit loop in reindex flow

**Test scenarios:**

- Happy path: 50-file batch -> exactly 1 chunk committed
- Happy path: 100-file batch -> 2 chunks (50+50)
- Edge case: 1-file batch -> 1 chunk committed
- Edge case: empty batch -> no commit
- Error path: chunk commit failure -> retry or propagate

**Verification:** Chunk commits match configured size, no unbounded batches

---

### ajvr-yield (NEW - PRIORITY 3)

**Goal:** Add reader yield points between chunk commits

**Requirements:**

- R1: Yield to readers between chunk commits
- R2: Watch batches move through states: queued → preparing → parsing → writing → completed | failed

**Dependencies:** ajvr-chunk

**Files:**

- Modify: `src/watch.rs` - add tokio::task::yield_now between commits
- Test: concurrent read during watch indexing test

**Approach:**

1. Add `tokio::task::yield_now()` after each chunk commit
2. Track batch state transitions (queued → preparing → parsing → writing → completed)
3. Verify readers can observe intermediate states

**Test scenarios:**

- Happy path: reader sees partial results during large batch index
- Integration: concurrent tool read during chunked write

**Verification:** Reader queries unblock during chunked indexing

---

## Implementation Units

- [ ] U1. **Close ajvr-elig** - Mark eligibility sub-bead complete

**Goal:** Close eligibility verification as done

**Dependencies:** None

**Verification:** Close bead with completion note

---

- [ ] U2. **Create ajvr-chunk sub-bead** - writer_chunk_size config + chunked loop

**Goal:** Add bounded write chunks to watcher

**Requirements:** R1, R2, R3 (see above)

**Dependencies:** k22b (parser workers)

**Files:** config.rs, watch.rs modifications + tests

**Verification:** Chunk commits respect configured size

---

- [ ] U3. **Create ajvr-yield sub-bead** - yield between commits

**Goal:** Yield reader access between chunk commits

**Requirements:** R1, R2 (see above)

**Dependencies:** U2

**Files:** watch.rs modifications + concurrent tests

**Verification:** Concurrent reads unblock during indexing

---

- [ ] U4. **Create 45ps atomic sub-bead** - hot-file incremental reparsing

**Goal:** Cache Tree state for hot files, incremental parse on edits

**Dependencies:** U3 (needs watcher chunking first)

**Files:** SourceFile tracking, incremental parse logic

**Verification:** Hot files reindex faster, cold fallback works

---

## Key Technical Decisions

- **writer_chunk_size default 50**: From acceptance.rs L420 spec
- **State machine for batches**: Explicit states enable debugging + yield points
- **tokio yield over sleep(0)**: More efficient CPU yield, same semantic

---

## Sources & References

- Origin bead: qartez-mcp-latest-ajvr
- Acceptance spec: acceptance.rs L420 (WRITER_CHUNK_SIZE = 50)
- Watch module: src/watch.rs (L130+ reindex flow)