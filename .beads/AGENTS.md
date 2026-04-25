# AGENTS.md for .beads/

This directory contains durable task-planning assets and, after initialization,
Beads runtime state.

## What is safe to edit

Safe to edit:
- `.beads/formulas/`
- `.beads/seeds/`
- `.beads/scripts/`
- `.beads/README.md`
- this `AGENTS.md`

Never hand-edit runtime/database state:
- `.beads/embeddeddolt/`
- `.beads/dolt/`
- `.beads/backup/`
- `.beads/tmp/`
- generated logs

## Agent workflow

Use Beads as the source of task truth.

Preferred commands for agents:
- `bd ready --json --quiet`
- `bd show <id> --json`
- `bd update <id> --claim --json --quiet`
- `bd create "..." -t task -p <n> --json`
- `bd dep add <child> <parent>`
- `bd close <id> --reason "Completed"`
- `bd prime`

Avoid interactive flows in automated sessions.
Do not use commands that open an editor.

## Dependency semantics

Use dependency types intentionally:
- `blocks` for hard sequencing
- `related` for soft linkage
- `parent-child` for epic/subtask structure
- `discovered-from` for work found during implementation

Only `blocks` should affect the ready queue.

## For this Qartez improvement program

The key implementation lanes are:
- readiness + DB split
- parser workers + lazy language loading
- parallel full-index parse/extract
- hot-file incremental reparsing
- shared DB lifecycle/pruning
- watcher parity + chunked writes

If you discover new work inside one lane, link it back to that lane's issue with `discovered-from`.
