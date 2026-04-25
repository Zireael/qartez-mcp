# Validation and handoff checklist

Use this at the end of every implementation session.

## Minimum validation before close

Run the full validation set unless the issue is explicitly documentation-only.

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo build --release
cargo test --release --no-fail-fast
```

If you touched installer/bootstrap files, also run:
- POSIX: `bash ./tests/test-install.sh`
- Windows: `./tests/test-install.ps1`

## Behaviour review

Before closing the issue:
- Re-read the matching Allium section.
- Confirm the code and tests satisfy it.
- If behaviour changed, update the spec.

## Beads handoff protocol

### If complete
```bash
bd close <id> --reason "Completed"
```

### If partially complete
```bash
bd update <id> --status in_progress --notes "What is done, what remains, and what to run next" --json --quiet
```

### If blocked
```bash
bd update <id> --status blocked --notes "Why blocked, what unblocks it, linked issue if any" --json --quiet
```

### If new work was discovered
```bash
bd create "..." -t task -p 2 --deps discovered-from:<parent-id> --json
```

## Final session summary template

When handing off, record:
- issue id and title
- files touched
- tests run
- spec sections checked/updated
- discovered follow-up issues
- next best issue from `bd ready --json --quiet`
