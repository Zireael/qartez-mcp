# AGENTS.md for allium/

The `.allium` file in this directory is the behavioural contract for the highest-ROI
Qartez indexing improvements.

## How to use it

- Treat `allium/qartez-indexing-improvements.allium` as the source of truth for
  externally visible behaviour.
- Read the relevant section before changing code.
- Re-read it before closing the issue.

## When to update the spec

Update the Allium spec when the **observable behaviour** changes, for example:
- a new readiness state is added or renamed
- a tool now returns an explicit deferred/result-unavailable status
- watcher fairness changes externally observable timing/semantics
- shared DB pruning adds or changes operator-visible commands or behaviour

Do **not** update the spec for pure refactors that do not change observable behaviour.

## Writing rules

- Keep statements behavioural, not implementation-prescriptive.
- Prefer: "when the initial index is incomplete, tools return an explicit not-ready status"
- Avoid: "use tokio::sync::watch with enum ReadyState"
- Each statement should be testable by an agent or operator.

## Completion rule

If code changes violate or extend the spec, update the spec in the same change set.
