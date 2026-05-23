# Bead Review: qartez-mcp-latest-2ss.3

## Bead
**ID**: qartez-mcp-latest-2ss.3
**Title**: Bug: Handle readiness write errors instead of silently ignoring
**Priority**: P0 (Critical)
**Issue Type**: bug
**Labels**: critical, error-handling, readiness

## the-fool Pre-mortem Analysis

### Failure Mode #1: Retry loop retries non-transient errors (HIGH)
**What**: Writes fail with `ENOSPC` (disk full), `EACCES` (permission denied), or `EROFS` (read-only filesystem). The retry loop burns through 3 attempts × 100ms = 300ms of wall time, each attempt also triggering a log write (see FM#3), then still fails — but the delay pushes the system past a realtime deadline.

**Mitigation**: Classify error types before retrying. Transient: `SQLITE_BUSY`, `SQLITE_LOCKED`, `SQLITE_IOERR` (some subcodes). Non-transient: `SQLITE_FULL`, `SQLITE_PERM`, `SQLITE_READONLY`, `SQLITE_CORRUPT`. Retry only transient family. Use `sqlite3_errcode()` to differentiate. Better: a `is_retryable(err) -> bool` function, with retry count = 1 for non-transient (log once, move on).

### Failure Mode #2: Logging inside `_safe` helper causes re-entrant failure / deadlock (HIGH)
**What**: The existing `set_readiness` call writes to DB. If the DB write fails and the `_safe` helper attempts to log the error via the same DB connection (e.g., writing to a log table or querying diagnostics), it re-enters the same connection/schema that just failed. On `SQLITE_LOCKED` or `SQLITE_BUSY`, this makes the state worse — the log write itself could panic or hang.

**Mitigation**: The `_safe` helper must log **without touching the DB**. Use `eprintln!`, `tracing::warn!`, or `log::error!` — something that goes to stderr or a ring buffer, not the same SQLite connection. If the application already uses a file-based logger, ensure the logger's I/O is on a different fd than the DB.

### Failure Mode #3: Control-flow change from `let _ =` to explicit handling alters retry/success semantics for critical transitions (MEDIUM)
**What**: In the current code, `let _ = set_readiness(&conn, Ready)` discards the error and **unconditionally continues**. The proposed change wraps in a `_safe` helper that logs and potentially retries. But some callers may depend on the unconditional-continue behavior — e.g., a startup sequence that must proceed even if readiness-marking fails. Under `_safe`, a non-retryable error logs but continues (fine). Under `_safe` with retry, a transient error delays 300ms and then continues (changed latency characteristic). If the caller sits inside a hot path or a lock scope, that delay could cascade into a timeout upstream.

**Mitigation**: Audit every call site before wrapping. Distinguish:
- **Critical**: readiness state must be persisted before proceeding. Use `_safe` + retry.
- **Best-effort**: caller is about to exit, or the state is advisory. Use `_safe` **without** retry (log once, continue).
- Make this a parameter: `set_readiness_safe(conn, state, retry: RetryPolicy)` where `RetryPolicy::{None, Transient(3, 100ms)}`.

### Failure Mode #4: `set_` functions panic internally — `let _ =` previously suppressed the unwind boundary (CRITICAL if present)
**What**: If `set_readiness` or `set_writer_state` calls `unwrap()`, `expect()`, `assert!()`, or `panic!()` internally, the current `let _ =` code catches the panic at the `Result` boundary only if the function returns `Result`. If the function panics instead, the `_safe` helper never even runs. The new logging catches the `Err` cases but the panic cases remain unlogged and crash the thread.

**Mitigation**: 
1. Audit `set_readiness` and `set_writer_state` for any `unwrap()`/`expect()` calls. Eliminate them.
2. Install a `std::panic::set_hook` that logs panics through the same non-DB logger before aborting.
3. In the `_safe` helper, wrap the call in `std::panic::catch_unwind` to convert panics into logged errors. Only do this if the functions are `UnwindSafe`.

### Failure Mode #5: Error-type mismatch — `_safe` helper expects a concrete error but `Box<dyn Error>` wraps something unexpected (LOW-MEDIUM)
**What**: If the functions return `Result<(), Box<dyn std::error::Error>>`, a `_safe` helper that pattern-matches on `ErrorKind` or tries `error.downcast_ref::<SqliteError>()` will miss the error if it's wrapped in a different layer (e.g., `std::io::Error` wrapping `sqlite3` error). If the downcast returns `None`, the helper falls through to a default "unknown error" branch — which might log it but might also **retry** a non-retryable unknown error, or **not retry** a retryable one.

**Mitigation**: 
1. Don't pattern-match on downcasted error types for retry logic. Instead, use `sqlite3_errcode()` directly on the connection handle after the failed call — the connection retains the error code regardless of how it was boxed.
2. If direct errcode access is not possible, write a trait `IsRetryable` with a blanket impl for `Box<dyn Error>` that checks `source()` chain recursively. Test with all known error paths.

## Summary Table

| # | Failure | Severity | Key Mitigation |
|---|---------|----------|----------------|
| 1 | Retry on non-transient (disk full) | HIGH | Classify error codes; skip retry on `SQLITE_FULL` etc. |
| 2 | Log re-enters failing DB connection | HIGH | Log to stderr/tracing, not DB |
| 3 | Control-flow change breaks caller assumptions | MED | Parameterize retry vs. no-retry per call site; audit all callers |
| 4 | Panic inside `set_` functions | CRITICAL | Audit unwraps; install panic hook; optional catch_unwind |
| 5 | Error downcast mismatch for retry logic | LOW-MED | Use `sqlite3_errcode()` on connection, not downcast |

## Synthesis & Action Items for Bead #3

### Must-Fix Before Implementation
1. **CRITICAL (if present)**: Audit `set_readiness` and `set_writer_state` for any `unwrap()`/`expect()` calls. Eliminate them. If a function does panic, wrap the call in `catch_unwind` and convert to a logged `Err` in the fallback path.
2. **HIGH**: Implement `is_retryable(errcode: i32) -> bool` that checks `sqlite3_errcode()` against transient vs. non-transient codes. Do NOT retry on `SQLITE_FULL`, `SQLITE_PERM`, `SQLITE_READONLY`, `SQLITE_CORRUPT`.
3. **HIGH**: Log errors only via `tracing::warn!` or `eprintln!`, not by writing to the DB. Never allow `set_readiness_safe` to make its own DB writes.

### Should-Fix During Implementation
4. **MEDIUM**: Parameterize retry policy per call site. Use `RetryPolicy::{None, Transient(3, 100ms)}`. Critical call sites (readiness transitions that gate query dispatch) use `Transient`. Best-effort call sites (startup, shutdown) use `None`.
5. **MEDIUM**: Audit all call sites of `set_readiness` and `set_writer_state` to understand which ones are critical vs. best-effort. Document the classification in comments.
6. **LOW-MEDIUM**: If using a `Box<dyn Error>` return type, implement an `IsRetryable` trait that checks the `source()` chain recursively for known error types. Avoid relying on `downcast_ref` on the top-level error.

### Updated Acceptance Criteria
- [ ] No `let _ = set_readiness(...)` or `let _ = set_writer_state(...)` remains in `src/main.rs` or `src/watch.rs`.
- [ ] All readiness/writer_state write errors are logged at `warn!` level or higher.
- [ ] **Retry only on transient errors** (`SQLITE_BUSY`, `SQLITE_LOCKED`, `SQLITE_IOERR` subcodes). **Never retry on non-transient** (`SQLITE_FULL`, `SQLITE_PERM`, `SQLITE_READONLY`, `SQLITE_CORRUPT`).
- [ ] For critical transitions (Indexing→Ready, FullIndexing→Idle), retry up to 3 times with 100ms backoff on transient errors only.
- [ ] For non-critical transitions, log once and continue without retry.
- [ ] System never panics on readiness write failure (use `catch_unwind` if functions may panic).
- [ ] Log output from `_safe` helpers is written to stderr/tracing, NOT to the database.
- [ ] Unit tests verify error logging and retry behavior (inject transient vs. non-transient errors).
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.

## Additional Edge Cases Not Addressed in Bead
1. **Cancellation during retry**: If a signal (e.g., SIGTERM) arrives during the retry loop, the retry must cancel promptly (use `tokio::select!` or a cancellation token). A blind 300ms loop could delay graceful shutdown.
2. **Backpressure on failed writes**: If a write fails and is retried, is there any mechanism to rate-limit further attempts? If a persistent error (e.g., disk full) causes every `set_readiness` to fail, the retry + log spam could overwhelm the system.
3. **Thread safety of `sqlite3_errcode`**: If `sqlite3_errcode()` is called on the connection after a failed write, but another thread is already using the same connection (via `Arc<Mutex<Connection>>` locker), the errcode might have been overwritten by a different operation. This is only relevant if the same connection is used concurrently, but with the `Mutex` it's serialized.
