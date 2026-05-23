# Bead Review: qartez-mcp-latest-2ss.10

## Bead
**ID**: qartez-mcp-latest-2ss.10
**Title**: Feature: Make retry_after state-dependent and configurable
**Priority**: P2
**Issue Type**: feature
**Labels**: retry, after, state-dependent, configuration

## Manual Review (Subagents unavailable — self-analysis)

### Problem Analysis

The deferred readiness response currently returns a fixed `retry_after_secs: 2`, regardless of state. This causes:
- Unnecessary client re-polling during ColdStart (which takes minutes)
- Too aggressive re-polling during Failed state (wastes resources when reindex is needed)
- Not configurable for different deployment contexts (CI vs. dev box)

### Failure Modes (Self-identified)

#### #1 (MEDIUM): Client retry storms during ColdStart
**Scenario**: Client retries every 2s during a 5-minute ColdStart. That's 150 requests for the same deferred response.

**Consequence**: Unnecessary CPU and I/O overhead. Especially problematic for CI environments.

**Mitigation**: State-dependent retry intervals (5s for ColdStart).

#### #2 (LOW): Config not exposed as CLI flag
**Scenario**: User wants to tune retry intervals for their environment (slow CI runners vs fast dev boxes).

**Consequence**: Must rebuild from source to change defaults.

**Mitigation**: Add `--retry-cold-start 5s`, `--retry-indexing 2s`, etc. optional CLI args to `qartez` binary.

#### #3 (LOW): Client ignores retry_after
**Scenario**: MCP clients are supposed to respect retry_after, but the protocol doesn't strictly enforce it.

**Consequence**: Even if we return a state-dependent value, some clients may ignore it and poll at their own rate.

**Mitigation**: Document the behavior. The server cannot force clients to respect retry_after.

### Recommended Solution

1. Update `retry_after_secs()`:
```rust
pub fn retry_after_secs(state: ReadinessState) -> u64 {
    match state {
        ReadinessState::ColdStart => 5,
        ReadinessState::Indexing => 2,
        ReadinessState::Maintenance => 10,
        ReadinessState::Failed => 5,
        ReadinessState::PartialReindex => 2,
        ReadinessState::Ready => 0, // Should never be deferred
    }
}
```

2. Add CLI config:
```rust
// In cli.rs / config.rs
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct RetryConfig {
    cold_start: u64,    // default: 5
    indexing: u64,      // default: 2
    maintenance: u64,    // default: 10
    failed: u64,        // default: 5
    partial_reindex: u64, // default: 2
}
```

3. Add `--retry.*` flags (matching config) in `cli.rs`.

### Updated Acceptance Criteria
- [ ] `retry_after_secs()` returns state-dependent values.
- [ ] Default values: ColdStart=5s, Indexing=2s, Maintenance=10s, Failed=5s, PartialReindex=2s.
- [ ] Values are configurable via config file and CLI flags.
- [ ] `server/mod.rs` uses the state-dependent value in deferred responses.
- [ ] Full validation passes: `cargo fmt`, `cargo clippy`, `cargo build --release`, `cargo test --release`.
