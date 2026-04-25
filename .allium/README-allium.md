# Qartez Allium Improvement Specs

This package contains Allium specs for the highest-ROI Qartez improvements identified in the review.

## Included files

- `allium/qartez-indexing-improvements.allium`
  - Reader/writer DB split
  - Explicit readiness signaling
  - Thread-local, lazy-loaded parser subsystem
  - Parallel full-index parse/extract
  - Hot-file incremental reparsing
  - Shared DB lifecycle management and pruning
  - Watcher parity and chunked writes

## Suggested placement

Drop the `allium/` directory into your repository root, or into whatever spec directory you use for Allium.

## How to use

This spec is intentionally behavioral. It captures what Qartez should do, not how Rust code should be structured.

Use it to:

1. align implementation across startup, indexing, watcher, and DB maintenance flows
2. drive drift checks between intent and code
3. generate follow-up implementation tasks and integration tests
4. pressure-test ambiguous decisions before changing code

## Mapping to current Qartez modules

- `src/index/parser.rs`
- `src/index/mod.rs`
- `src/watch.rs`
- `src/storage/schema.rs`
- `src/storage/mod.rs`
- `src/main.rs`
- `src/server/mod.rs`

## Notes

- The Allium syntax here follows the public language reference and examples from JUXT's docs as of April 2026.
- I could not run an `allium` CLI validator in this environment, so treat these as high-confidence draft specs that should be validated in your local toolchain if you have the CLI installed.
- Open questions are captured at the end of the spec instead of being silently decided.
