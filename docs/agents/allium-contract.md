# Allium contract workflow

The Allium spec is the behavioural acceptance contract for this implementation program.

Spec file:
- `allium/qartez-indexing-improvements.allium`

## How to work from the spec

1. Identify the task you claimed in Beads.
2. Find the matching behavioural section in the Allium file.
3. Translate that section into code changes and tests.
4. When behaviour changes, update the spec in the same patch.

## Mapping tasks to spec areas

- **Readiness + DB split**
  - startup truthfulness
  - explicit not-ready / maintenance semantics
  - read/write isolation

- **Parser workers + lazy loading**
  - parser startup behaviour
  - language-on-demand behaviour
  - no unnecessary grammar cost for absent languages

- **Parallel full index**
  - faster full-index completion without result drift
  - deterministic outputs remain stable

- **Hot-file incremental reparsing**
  - lower latency on watcher-driven edits
  - correct fallback when old-tree reuse is not available

- **Shared DB lifecycle**
  - visible DB stats
  - stale-root pruning
  - explicit compact/maintenance entry points

- **Watcher parity + chunking**
  - same supported-file rules as the indexer
  - read fairness during large write bursts

## Update policy

Update the Allium file if an operator, caller, or test harness can observe a new behaviour.
Do not update it for internal refactors alone.

## Review questions

Before you close an issue, answer these:
- What observable behaviour changed?
- Is it already described in the spec?
- If not, did you update the spec?
- Is there a test that exercises the behavioural claim?
