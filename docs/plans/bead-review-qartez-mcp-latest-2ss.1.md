# Bead Review: qartez-mcp-latest-2ss.1

## Bead
**ID**: qartez-mcp-latest-2ss.1
**Title**: Bug: Give watcher its own dedicated DB connection
**Priority**: P0 (Critical)
**Issue Type**: bug
**Labels**: critical, db-connection, mutex-contention

## the-fool Pre-mortem Analysis

### Steelmanned Thesis
> "Opening a second database connection for the file watcher eliminates the `Arc<Mutex<Connection>>` contention that currently blocks all server queries during reindex. SQLite WAL mode inherently supports concurrent reads from multiple connections, and the existing `storage::open_db()` handles connection setup. Each connection manages its own transaction lifecycle. The server queries proceed uninterrupted while the watcher reindexes on its own connection."

### Ranked Failure Narratives

#### 1. In-Memory Test Divergence — Likelihood: HIGH | Impact: CRITICAL
**Scenario**: The CI pipeline is green. A developer adds an integration test for reindex correctness. The test uses `Connection::open_in_memory()` for the server. The watcher calls `storage::open_db()` which also opens `:memory:` — creating a **completely independent, empty, second database**. The watcher's reindex writes to database B; the server reads from database A. Tests either pass non-deterministically or hang waiting for data that never appears.

**Consequence chain**:
- 1st order: Integration tests requiring cross-connection data visibility are unreliable
- 2nd order: Developers add polling/sleep workarounds, tests become slow and flaky
- 3rd order: A regression in production reindex correctness passes CI undetected

**Mitigation**:
- `storage::open_db()` must accept an optional pre-opened `Connection` for test injection
- Switch integration tests to `tempfile::NamedTempFile` backed databases
- Use the URI filename trick: `file::memory:?cache=shared` so multiple connections share the same in-memory database
- **Action item**: Add this to the bead's scope before implementation

#### 2. Unhandled `SQLITE_BUSY` Between Two Writers — Likelihood: HIGH | Impact: HIGH
**Scenario**: A repository with 50K files is added. The watcher's full reindex takes 45 seconds. During this, a server operation needs to write the readiness/writer state. The server tries to write on its connection but the watcher's write transaction is still active. SQLite WAL mode supports one writer at a time. The server's write fails with `SQLITE_BUSY`.

If `busy_timeout` (5s) elapses before the watcher finishes, the server gets an error. If caught, the state write is silently dropped. The orchestrator sees no state transition, triggers recovery, spawns a **second reindex**. Two reindexes compete for the write lock.

**Consequence chain**:
- 1st order: Server's state write fails or blocks during watcher reindex
- 2nd order: Orchestrator detects missing state, triggers duplicate reindex
- 3rd order: Two concurrent reindexes thrash for the write lock, CPU spikes, queries stall — **worse than the original problem**

**Mitigation**:
- Set `busy_timeout = 5000` on **both** connections explicitly in `storage::open_db()`
- Implement a write-retry layer with exponential backoff and a maximum retry count
- Watcher should batch writes and minimize transaction duration — commit every N files, not one giant transaction
- Consider a **soft application-level write lock** (e.g., `AtomicBool` with try_lock) so the server defers state writes instead of blocking

#### 3. PRAGMA / Connection Configuration Drift — Likelihood: MEDIUM | Impact: HIGH
**Scenario**: The server connection is fully configured at startup via `storage::open_db()`: WAL journal mode, `busy_timeout`, `foreign_keys = ON`, cache size, mmap size. The watcher calls the same `storage::open_db()` — or does it? If the watcher opens via a different code path (e.g., inline `Connection::open()`), it misses a PRAGMA.

For example, if `foreign_keys = ON` is set on the server but not the watcher, the reindex inserts rows violating FK constraints. The index now has orphan rows. The server's queries against the index silently return unexpected results — no crash, no error. Data corruption that's invisible until a user reports the wrong file showing up in search results.

**Consequence chain**:
- 1st order: Watcher connection missing a PRAGMA or function registration
- 2nd order: Incorrect index data written (orphans, wrong rankings, missing entries)
- 3rd order: Users see wrong or stale file search results; trust in the tool erodes

**Mitigation**:
- `storage::open_db()` must be the **single, shared code path** for opening any database connection
- Extract connection configuration into a builder/initializer function called for every new connection
- Add a health-check query that runs on both connections at startup: `SELECT * FROM pragma_journal_mode`, `SELECT * FROM pragma_foreign_keys` — log a warning if they diverge

#### 4. Schema Migration Race — Likelihood: MEDIUM | Impact: MEDIUM
**Scenario**: The server runs schema migrations at startup. The watcher opens its connection **before** migrations complete. The watcher sees an incomplete schema. While SQLite's `SQLITE_SCHEMA` + retry mechanism handles most cases, there's a window where:
- `ALTER TABLE ADD COLUMN` — retry path might re-prepare and re-execute a multi-statement batch differently
- If the migration runs on the **watcher** connection too (e.g., because the watcher checks `schema_version` and decides to migrate), both connections run migrations. If not idempotent, one fails with "table already exists"

**Consequence chain**:
- 1st order: Schema mismatch between connections
- 2nd order: Watcher's queries fail with `SQLITE_SCHEMA`, rusqlite retries — performance hit on first query
- 3rd order: If migration is not idempotent, watcher crashes with table-already-exists error

**Mitigation**:
- Run **all** migrations on the watcher's connection too, or defer watcher startup until the server signals "migrations complete" via a startup barrier
- Make all DDL statements idempotent (`CREATE TABLE IF NOT EXISTS`, etc.)
- Open the watcher connection **after** server startup phase is complete

#### 5. Connection Lifetime & Panic Safety — Likelihood: LOW | Impact: MEDIUM
**Scenario**: Root directory removed during reindex → watcher must abort. If watcher panics while holding a write transaction, Rust's `Drop` runs, rolling back the transaction. On **Windows**, the `.shm` file can become stale. On next startup, SQLite may fail to open WAL mode ("directory is locked").

**Consequence chain**:
- 1st order: Watcher panic → transaction rollback → partial reindex lost
- 2nd order: Incomplete reindex means the next reindex starts from scratch, doubling the work
- 3rd order: Repeated panics cause an infinite reindex loop

**Mitigation**:
- Ensure `Connection` is dropped on a controlled thread (not the OS file-watcher callback)
- On Windows, after a crash, attempt to clear stale `.shm` files on startup before opening the database
- Wrap the watcher's reindex loop in `catch_unwind`-style error handling
- Test: simulate a panic during write and verify the connection is cleanly closed

### Early Warning Signs

| Signal | Failure Predicted | Check Frequency |
|--------|-------------------|-----------------|
| Integration tests for reindex are flaky or use sleeps | #1 (in-memory test divergence) | Per PR / CI run |
| `SQLITE_BUSY` appearing in logs | #2 (two-writer conflict) | Per deploy, monitor in production |
| Watcher recover/reset events firing repeatedly | #2 (orchestrator thrashing) | Per production run |
| PRAGMA journal_mode differs between connections in health-check | #3 (config drift) | Per startup |
| Migration errors or `SQLITE_SCHEMA` on first watcher query | #4 (schema timing) | Per startup |
| Windows users reporting "database is locked" on restart | #5 (stale SHM file) | Per Windows issue report |

### Inversion Check

**What would guarantee failure?**
1. `storage::open_db()` opens `:memory:` in test mode without awareness that two connections to `:memory:` are different databases.
2. The watcher and server both try to **write simultaneously without a busy_timeout or retry layer**.
3. The watcher registers custom functions/PRAGMAs via a **different code path** than the server — missing any one causes silent data corruption.

**Do any exist now?**
- `storage::open_db()` currently opens based on `db_path` — tests need to be checked
- `busy_timeout` is set in `open_db()` per the code comment at `src/storage/mod.rs`
- Need to verify whether custom functions are registered in `open_db()` vs elsewhere

---

## ce-code-review Report-only Analysis

### Verdict: Ready with fixes — design direction is sound, 2 P0 blockers and 3 P1 risks need resolution before implementation.

### P0 — Critical

| # | Issue | Reviewer | Confidence |
|---|-------|----------|------------|
| 1 | **In-memory test compatibility is broken** — `open_in_memory()` cannot be used with the proposed design | correctness, testing | 100 |
| 2 | **No connection-lifetime contract for task abort** — replacing a watcher mid-reindex leaks its connection | correctness | 100 |

**#1 — In-memory test compatibility** (P0)
The design says watcher opens its own connection via `storage::open_db(&db_path)`. But `open_db()` opens a *file* on disk. In-memory tests create in-memory connections and pass them as `Arc<Mutex<Connection>>`. Each `Connection::open_in_memory()` creates an *independent* in-memory database.

The existing comment at `src/watch.rs:L44-45` confirms this test pattern:
```rust
/// When `None`, the watcher writes without coordination (used by tests that
/// drive indexing through an in-memory connection only).
```

**Fix**: Add a `Connection`-accepting constructor alongside the new `db_path` constructor:
- `Watcher::with_connection(conn: Connection, ...)` for tests
- `Watcher::with_db_path(db_path, ...)` for production
- `reindex()` uses whichever is available

**#2 — No connection-lifetime contract for task abort** (P0)
In `src/server/mod.rs:L275-279`, when a second `attach_watcher` fires for the same root, the old `JoinHandle` is replaced. The old handle is simply dropped without `abort()`. If the watcher has its own `Connection` (not `Arc<Mutex<Connection>>`), dropping the join handle without aborting means the spawned task's `Watcher` struct (and its `Connection`) lives until the task naturally finishes its current `reindex()` call — potentially seconds or minutes.

**Fix**: The `attach_watcher` code must explicitly `abort()` the old handle, and `Watcher::run()` must handle cancellation to close the connection promptly.

### P1 — High

| # | Issue | Reviewer | Confidence |
|---|-------|----------|------------|
| 3 | `open_db()` calls `schema::create_schema` on every open — no correctness issue but wasted I/O | maintainability | 100 |
| 4 | `reindex()` has no fallback if watcher's connection fails — skipped batch with stale state | correctness | 75 |
| 5 | WAL visibility delay — server reads may briefly lag behind watcher writes | correctness | 75 |

**#3 — Schema creation is idempotent but unnecessary on existing DBs** (P1)
`storage::mod.rs:L71` — `open_db()` always calls `schema::create_schema(&conn)`. On a watcher connection to an existing DB, this is redundant but harmless (uses `CREATE TABLE IF NOT EXISTS`). Consider a `storage::open_db_for_writer()` variant that skips schema creation, or just accept the small overhead.

**#4 — `reindex()` has no fallback if watcher's connection fails** (P1)
If the watcher opens its own connection and `open_db()` fails (permission denied, disk full, lock contention), `reindex()` should fall back rather than crash or silently skip.

Precedent from `main.rs:L193-199`:
```rust
let conn = match storage::open_db(&db_path) {
    Ok(c) => c,
    Err(e) => {
        tracing::error!("background indexer: open_db failed: {e}");
        return;
    }
};
```

The watcher should log and skip the batch, letting the next file change retry. **Do NOT fall back to the server's shared connection** — that would defeat the purpose and reintroduce blocking.

**#5 — WAL visibility: server may briefly lag watcher writes** (P1)
In WAL mode, each connection manages its own read snapshot. When the watcher commits, the server won't see that data until it starts a *new* read transaction (on the next `lock()` → query → `unlock()` cycle).

For `writer_state` transitions, a one-batch lag is acceptable. For indexed data, the lag is at most one chunk (50 files). **Audit needed**: Check if any server tool handler caches the `Connection` across calls (the `MutexGuard` pattern). If so, that would stall the snapshot and potentially show stale data.

### P2 — Medium

**#6 — `set_writer_state` error is silently ignored** (P2)
`src/watch.rs:L219`: `let _ = crate::storage::write::set_writer_state(&conn, Idle);`
If the watcher's connection is in a broken state, the writer_state stays as `IncrementalIndexing` permanently. Fix: Log at WARN level instead of silently ignoring.

### P3 — Low

**#7 — Missing acceptance criteria** (P3)
Add to the bead's AC:
- [ ] In-memory test connections still work (via constructor overload)
- [ ] Watcher connection open failure logs and skips the batch
- [ ] Writer_state is reset to `Idle` even if watcher connection errors during indexing
- [ ] Watcher connection is closed promptly when watcher task is replaced or server shuts down
- [ ] New integration test: issue N server queries during a watcher reindex, verify zero are blocked

### Summary

| Area | Assessment |
|------|------------|
| **Problem solved?** | Yes, completely. Moving from `Arc<Mutex<Connection>>` to a dedicated watcher connection is the right solution. The background indexer in `main.rs:L193` already proves this pattern works. |
| **Correctness** | Sound for production. WAL mode supports concurrent writer+readers. Need a constructor fallback for in-memory tests. |
| **Safety** | Task-replacement connection lifecycle needs explicit `abort()` to avoid leaked connections. |
| **Completeness** | Missing fallback for watcher `open_db` failure, missing error logging on `set_writer_state`. |
| **Testability** | **P0 concern**: in-memory tests cannot use `open_db()`. Needs a dual-constructor approach. |

---

## Synthesis & Action Items for Bead #1

### Must-Fix Before Implementation
1. **Add in-memory test constructor** (P0): Add `Watcher::with_connection(conn: Connection, ...)` alongside `Watcher::with_db_path(db_path, ...)`
2. **Add task-replacement abort contract** (P0): Ensure `attach_watcher` calls `abort()` on old handles and `Watcher::run()` handles cancellation via `tokio::select!` or cancellation token
3. **Add fall-open fallback for watcher's connection** (P1): If `open_db()` fails, log and skip the batch (do not fall back to server connection)

### Should-Fix During Implementation
4. **Log `set_writer_state` errors** (P2): Change `let _ =` to `if let Err(e) = ... { tracing::warn!(...) }`
5. **Add busy_timeout retry layer** (P1): Implement exponential backoff for `SQLITE_BUSY` (or rely on existing `busy_timeout=5000` but document this assumption)
6. **Populate `db_path` in `Watcher::new`** (P1): Ensure tests that currently pass `Arc<Mutex<Connection>>` can provide a `Connection` or a `db_path`

### Updated Acceptance Criteria
- [x] Watcher has its own dedicated connection **(verified: correct direction)**
- [x] Server queries NOT blocked during reindex **(verified: correct, but needs integration test)**
- [x] Readiness/writer_state transitions still work **(verified: correct, but needs error handling)**
+ [ ]-In-memory test connections still work (via constructor overload)
+ [ ]-Watcher connection open failure logs and skips the batch
+ [ ]-Writer_state is reset to `Idle` even if watcher connection errors
+ [ ]-Watcher connection is closed promptly when task is replaced
+ [ ]-New integration test: N server queries during reindex, zero blocked
+ [ ]-PRAGMA health check at startup verifies both connections match
+ [ ]-Busy_timeout set on both connections (documented / tested)
