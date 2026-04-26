//! Readiness state management for the qartez-mcp server.
//!
//! Tracks the project's indexing lifecycle and gates queries accordingly.
//! See `.allium/qartez-indexing-improvements.allium` for the behavioural
//! contract that governs these states and transitions.

use serde::{Deserialize, Serialize};
use std::fmt;

/// The readiness state of a project index.
///
/// Transitions (per Allium rules):
/// - ColdStart → Indexing (when initial index begins)
/// - Indexing → Ready (when initial index completes and pending work drains)
/// - Ready → PartialReindex (when watcher detects changes)
/// - PartialReindex → Ready (when incremental index completes and pending work drains)
/// - Any → Failed (on unrecoverable error)
/// - Ready → Maintenance → Ready (during manual maintenance)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessState {
    /// Project just opened, no index exists yet
    ColdStart,
    /// Initial full index in progress
    Indexing,
    /// Index is complete and usable
    Ready,
    /// Incremental reindex in progress (index is still usable)
    PartialReindex,
    /// Manual maintenance mode (queries deferred)
    Maintenance,
    /// Unrecoverable error state
    Failed,
}

impl fmt::Display for ReadinessState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadinessState::ColdStart => write!(f, "cold_start"),
            ReadinessState::Indexing => write!(f, "indexing"),
            ReadinessState::Ready => write!(f, "ready"),
            ReadinessState::PartialReindex => write!(f, "partial_reindex"),
            ReadinessState::Maintenance => write!(f, "maintenance"),
            ReadinessState::Failed => write!(f, "failed"),
        }
    }
}

impl ReadinessState {
    /// Parse from the string stored in the meta table.
    pub fn from_meta(value: &str) -> Option<Self> {
        match value {
            "cold_start" => Some(ReadinessState::ColdStart),
            "indexing" => Some(ReadinessState::Indexing),
            "ready" => Some(ReadinessState::Ready),
            "partial_reindex" => Some(ReadinessState::PartialReindex),
            "maintenance" => Some(ReadinessState::Maintenance),
            "failed" => Some(ReadinessState::Failed),
            _ => None,
        }
    }

    /// Returns true if queries can be served from this state.
    ///
    /// Per Allium `QueryServesFromReadyOrPartialIndex`: queries are served
    /// when readiness is `Ready` or `PartialReindex`.
    pub fn is_queryable(&self) -> bool {
        matches!(self, ReadinessState::Ready | ReadinessState::PartialReindex)
    }

    /// Returns true if queries should be deferred with a retry hint.
    ///
    /// Per Allium `QueryDefersUntilIndexIsUsable`: queries are deferred
    /// when readiness is `ColdStart`, `Indexing`, or `Maintenance`.
    pub fn should_defer(&self) -> bool {
        matches!(
            self,
            ReadinessState::ColdStart | ReadinessState::Indexing | ReadinessState::Maintenance
        )
    }

    /// Returns the retry_after duration in seconds for deferred responses.
    ///
    /// Per Allium config: `readiness_retry_after = 2.seconds`
    pub fn retry_after_secs(&self) -> u64 {
        2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readiness_state_is_queryable() {
        assert!(!ReadinessState::ColdStart.is_queryable());
        assert!(!ReadinessState::Indexing.is_queryable());
        assert!(ReadinessState::Ready.is_queryable());
        assert!(ReadinessState::PartialReindex.is_queryable());
        assert!(!ReadinessState::Maintenance.is_queryable());
        assert!(!ReadinessState::Failed.is_queryable());
    }

    #[test]
    fn readiness_state_should_defer() {
        assert!(ReadinessState::ColdStart.should_defer());
        assert!(ReadinessState::Indexing.should_defer());
        assert!(!ReadinessState::Ready.should_defer());
        assert!(!ReadinessState::PartialReindex.should_defer());
        assert!(ReadinessState::Maintenance.should_defer());
        // Failed is not deferrable — it's an error, not a temporary state
        assert!(!ReadinessState::Failed.should_defer());
    }

    #[test]
    fn readiness_state_display_roundtrip() {
        for state in [
            ReadinessState::ColdStart,
            ReadinessState::Indexing,
            ReadinessState::Ready,
            ReadinessState::PartialReindex,
            ReadinessState::Maintenance,
            ReadinessState::Failed,
        ] {
            let s = state.to_string();
            assert_eq!(ReadinessState::from_meta(&s), Some(state));
        }
    }

    #[test]
    fn readiness_state_from_meta_rejects_unknown() {
        assert_eq!(ReadinessState::from_meta("unknown"), None);
        assert_eq!(ReadinessState::from_meta(""), None);
        assert_eq!(ReadinessState::from_meta("READY"), None); // case-sensitive
    }

    #[test]
    fn readiness_state_serde_roundtrip() {
        for state in [
            ReadinessState::ColdStart,
            ReadinessState::Indexing,
            ReadinessState::Ready,
            ReadinessState::PartialReindex,
            ReadinessState::Maintenance,
            ReadinessState::Failed,
        ] {
            let json = serde_json::to_string(&state).unwrap();
            let back: ReadinessState = serde_json::from_str(&json).unwrap();
            assert_eq!(state, back);
        }
    }
}
