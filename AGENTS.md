# AGENTS.md

Qartez is a **local-first, agent-first code intelligence server**. Preserve that.

The project already has a behavioural contract and a task graph. Agents must use
both during implementation:

1. **Beads issue graph** = task truth, dependency truth, handoff truth.
2. **Allium spec** = behavioural truth, acceptance truth, rollout truth.
3. **Rust compiler + tests + CI commands** = implementation truth.

Do not work from memory alone. Start from the graph and the spec.

Read these files before making changes:
- `@.allium/qartez-indexing-improvements.allium`
- `@docs/agents/beads-workflow.md`
- `@docs/agents/allium-contract.md`
- `@docs/agents/high-roi-implementation-playbook.md`
- `@docs/agents/validation-and-handoff.md`

## Core rules

- Preserve **local-first** behaviour. Do not introduce a mandatory external service.
- Prefer **explicit readiness/error states** over silent partial results.
- Keep changes **minimal and lane-focused**. Do not mix structural rewrites with behaviour changes unless the issue explicitly calls for both.
- Never hand-edit Beads runtime state under `.beads/embeddeddolt/`, `.beads/dolt/`, `backup/`, or `tmp/`.
- Do not silently change observable behaviour without updating the Allium spec when the contract changes.
- Add or update tests for every behaviour change.
- Keep output deterministic where the current system depends on it.

## Start-of-session workflow

1. Initialize Beads if needed:
   - `bd init --quiet`
2. Load graph context:
   - `bd prime`
   - `bd ready --json --quiet`
3. Select or confirm the issue.
4. Read the issue details:
   - `bd show <id> --json`
5. Claim the issue atomically before coding:
   - `bd update <id> --claim --json --quiet`
6. Map the issue to the relevant section(s) of `.allium/qartez-indexing-improvements.allium`.
7. Read the lane notes in `docs/agents/high-roi-implementation-playbook.md`.

If no issue is specified, start with `bd ready --json --quiet` and choose the highest-priority unblocked work.

## During implementation

- Keep one branch/PR scoped to one issue or one tightly related chain of issues.
- If you discover new work, create it immediately and link it with `discovered-from`:
  - `bd create "..." -t task -p 2 --deps discovered-from:<parent-id> --json`
- If you are blocked, mark it explicitly and explain why:
  - `bd update <id> --status blocked --notes "..." --json --quiet`
- Update status as you move:
  - `bd update <id> --status in_progress --json --quiet`
- Keep the Allium spec open while implementing. It is the contract.

## Validation commands

Use targeted commands while iterating, then run the full validation set before closing the issue.

### Targeted Rust loop
- `cargo test <targeted_test_name>`
- `cargo test --lib <module_name>`
- `cargo test --release <targeted_test_name>`

### Full validation set
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo build --release`
- `cargo test --release --no-fail-fast`

### Installer checks
Only if you touched installer or cross-platform bootstrap code:
- POSIX: `bash ./tests/test-install.sh`
- Windows: `./tests/test-install.ps1`

## Completion protocol

Before closing an issue:
1. Re-read the relevant Allium sections and verify the behaviour matches.
2. Run the full validation set.
3. Update docs/spec if observable behaviour changed.
4. File any discovered follow-up work in Beads.
5. Close the issue with a concrete completion reason:
   - `bd close <id> --reason "Completed"`
6. Pull/rebase and push your branch.

Do not end a session with hidden state in your head. Put it in Beads.

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
