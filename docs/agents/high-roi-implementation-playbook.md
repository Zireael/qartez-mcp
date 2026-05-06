# High-ROI implementation playbook

This file tells agents where to work, what to avoid, and how to validate each lane.

## Implementation status

All 6 lanes are **implemented and tested**. This playbook now serves as
operator documentation and onboarding reference.

| Lane | Status | Key files | Beads |
|------|--------|-----------|-------|
| 1 — Readiness + DB split | ✅ Complete | `src/server/mod.rs`, `src/storage/` | svve, n2xb, r5y3, bxmp |
| 2 — Parser workers | ✅ Complete | `src/index/parser.rs`, `src/index/parser_workers.rs` | k22b, qtef |
| 3 — Parallel full-index | ✅ Complete | `src/index/mod.rs` | jxqk, ogj1 |
| 4 — Hot-file incremental reparsing | ✅ Complete | `src/index/mod.rs`, `src/index/parser_workers.rs` | 45ps, kf5, j6d |
| 5 — DB lifecycle | ✅ Complete | `src/storage/schema.rs`, `src/storage/write.rs` | m2te, p7t3 |
| 6 — Watcher parity + chunking | ✅ Complete | `src/watch.rs` | ajvr, tub6 |

Validation baseline: 24 acceptance tests, ~1485 lib tests (release mode).

## Lane 1 — Reader/writer DB split + readiness signalling

Primary hotspots:
- startup/index orchestration
- server DB wiring
- watcher/index writer path
- tool-handler read path

Goals:
- one writer path for indexing, watcher, prune, compact
- read pool for tool handlers
- explicit readiness states
- no silent empty results during cold start

Anti-goals:
- do not add mandatory external DB services
- do not hide transient not-ready conditions behind empty results

Validation:
- cold-start query before index completion returns explicit not-ready/deferred status
- heavy writer activity does not starve read requests

## Lane 2 — Thread-local, lazy-loaded parser subsystem

Primary hotspots:
- `src/index/parser.rs`
- parser call sites in `src/index/mod.rs`

Goals:
- remove single `Mutex<Parser>` bottleneck
- create parser state lazily by language
- Rust-only repos should not pay meaningful cost for absent languages

Anti-goals:
- do not eagerly initialize all supported grammars
- do not introduce non-deterministic parser sharing bugs

Validation:
- parser metrics show per-language lazy activation
- parallel parse workers do not deadlock or serialize on one parser lock

## Lane 3 — Parallel full-index parse/extract

Primary hotspot:
- `src/index/mod.rs`

Goals:
- parallelize file parse/extract
- keep DB writes serialized or batched deterministically

Anti-goals:
- do not write to SQLite directly from every worker
- do not change output ordering in a way that destabilizes deterministic downstream behaviour

Validation:
- full-index wall-clock time drops materially
- result sets remain stable across runs

## Lane 4 — Hot-file incremental reparsing

Primary hotspots:
- parser/edit path
- watcher-driven reindex path

Goals:
- reuse recent `Tree` state for changed files
- fall back cleanly to cold parse on cache miss or invalid edit info

Implementation details:
- **ChangeSet** (`src/index/mod.rs`): `has_byte_edit` field controls tree reuse.
  `changed()` defaults to true (conservative: assume bytes changed).
  `metadata_only()` sets false (e.g. chmod/touch without content change).
- **Conditional invalidation**: In `incremental_index_batch`:
  - `has_byte_edit=true` + hot tree in pool → preserve (incremental parse via `Tree.edit()`)
  - `has_byte_edit=true` + no hot tree → invalidate (cold parse fallback)
  - `has_byte_edit=false` → preserve (metadata-only, tree still valid)
- **TreeCacheState** (`src/index/parser_workers.rs`): Enum with DB↔enum mapping.
  States: `Absent`, `Hot`, `Invalidated`, `Evicted`.
  Legacy DB value "cold" normalized to `Invalidated` via `from_db_str()` and schema migration.
- **evict_tree_cache()**: Two-phase (mark Evicted then retain) to prevent
  use-after-evict races in the in-memory cache.
- **DB column**: `files.tree_cache` tracks persistent state;
  `files.has_hot_tree` tracks in-memory cache presence.

Anti-goals:
- do not try to persist syntax trees for the entire repo yet
- do not risk stale-tree correctness bugs for marginal speedups

Validation:
- repeated edits on hot files reindex faster than cold parse baseline
- cache misses remain safe and correct
- `rule_hot_files_prefer_incremental_reparse` acceptance test
- `rule_incremental_change_falls_back_to_cold_parse` acceptance test
- `invariant_incremental_tasks_are_always_classified` acceptance test
- `rule_tree_cache_state_roundtrip` acceptance test

## Lane 5 — Shared DB lifecycle and pruning

Primary hotspots:
- DB schema/open path
- CLI/admin surfacing
- project/root metadata

Goals:
- make shared `--db-path` usage visible and manageable
- track root lifecycle metadata
- support prune/stats/compact operations
- keep normal startup cheap

Anti-goals:
- do not vacuum/compact unexpectedly on normal startup
- do not prune active roots or live data silently

Validation:
- stale roots can be pruned safely
- stats surface DB size/root counts/maintenance timestamps

## Lane 6 — Watcher parity + chunked writes

Primary hotspots:
- `src/watch.rs`
- supported-file logic shared with indexer

Goals:
- watcher and indexer agree on supported files
- heavy watcher batches are chunked and fair to readers

Anti-goals:
- do not rely on extension-only heuristics
- do not let branch switches monopolize the DB for long stretches

Validation:
- special filenames stay fresh
- read latency under large watcher bursts remains bounded
