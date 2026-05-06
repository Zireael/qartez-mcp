# Beads workflow for Qartez implementation agents

This project uses Beads (`bd`) as a dependency-aware task graph for coding work.
Use it as your persistent memory and sequencing system.

## Why Beads here

The Qartez indexing/runtime improvements are interdependent:
- parser changes unblock parallel indexing
- DB split unblocks watcher fairness and startup truthfulness
- DB lifecycle work depends on the new connection model

A plain checklist is not enough. Use the graph.

## Standard session loop

### 1. Load current context
```bash
bd prime
bd ready --json --quiet
```

### 2. Inspect the selected task
```bash
bd show <id> --json
```

### 3. Claim it atomically
```bash
bd update <id> --claim --json --quiet
```

### 4. Work the task
- Read the linked docs and relevant Allium section.
- Keep changes scoped.
- Run targeted tests as you go.

### 5. Track discovered work
If you find missing tests, follow-up cleanups, or hidden bugs:
```bash
bd create "Add stress test for watcher rename bursts" -t task -p 2 --deps discovered-from:<parent-id> --json
```

### 6. Finish or block explicitly
Complete:
```bash
bd close <id> --reason "Completed"
```

Blocked:
```bash
bd update <id> --status blocked --notes "Blocked on <other-id> / design decision / failing dependency" --json --quiet
```

## Coordination rules

- Always claim before changing code.
- Prefer one issue per branch unless the graph says otherwise.
- Pull/rebase before checking ready work in multi-agent environments.
- Never leave discovered work undocumented.

## For this program

Recommended top-level sequencing:
1. baseline/acceptance harness ✅
2. readiness contract ✅
3. DB split ✅
4. watcher parity + chunking ✅
5. parser workers ✅
6. parallel full-index ✅
7. hot-file incremental reparse ✅
8. shared DB lifecycle ✅
9. rollout docs + regressions ✅ (i9sy in progress)

All 6 implementation lanes are complete. See `docs/agents/high-roi-implementation-playbook.md`
for per-lane status, validation rules, and operator documentation.
