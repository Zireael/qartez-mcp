# Project Instructions for AI Agents

This file provides instructions and context for AI coding agents working on this project.

<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->


## Build & Test

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo test --release --no-fail-fast
```

Installer checks (only if installer/bootstrap files changed):
- Windows: `.\tests\test-install.ps1`
- POSIX: `bash ./tests/test-install.sh`

## Architecture Overview

Qartez is a **local-first, agent-first code intelligence MCP server** written in Rust.
It provides AST-based code intelligence (indexing, parsing, search, dependency analysis)
to AI coding agents via the MCP protocol.

Key binaries:
- `qartez` — main MCP server
- `qartez-guard` — edit guard sidecar
- `qartez-setup` — bootstrap/setup utility
- `benchmark` — performance benchmarking

## Code Exploration

**Use Qartez MCP tools for all source-code exploration.** Do not use raw grep/read for code when Qartez tools are available.

| Task | Tool |
|---|---|
| Project structure | `qartez_map` |
| Symbol/text search | `qartez_grep` |
| Jump to definition | `qartez_find` |
| Read source code | `qartez_read` |
| Related files | `qartez_context` |
| Blast radius before editing | `qartez_impact` |
| Call hierarchy | `qartez_calls` |
| Dependencies | `qartez_deps` |
| Symbol references | `qartez_refs` |
| Run tests/build | `qartez_project` |

**Before editing load-bearing files** (high PageRank or large blast radius), run `qartez_impact` first.

## Conventions & Patterns

- Rust 2024 edition, Rust 1.88+
- Local-first: never introduce mandatory external services
- Explicit readiness/error states over silent partial results
- Keep changes minimal and lane-focused
- Update `.allium/qartez-indexing-improvements.allium` when observable behaviour changes
- Use Beads (`bd`) for all task tracking — not TodoWrite or markdown TODOs
