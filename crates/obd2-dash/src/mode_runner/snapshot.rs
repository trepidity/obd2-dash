use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModeState {
    Connecting,
    Discovering {
        origin: DiscoveryOrigin,
        step: u32,
        total: u32,
    },
    Telemetry,
    Diagnostic {
        phase: u8,
        phase_total: u8,
        step: u32,
        total: u32,
    },
    Reconnecting {
        attempt: u32,
    },
    ShuttingDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryOrigin {
    Initial,
    Rescan,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityPersistence {
    Cached,
    Pending,
    SessionOnlyNoVin,
    SessionOnlyStoreError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapabilityVerification {
    Ready,
    Verifying { remaining: usize },
    Degraded { unresolved: usize },
    ConservativeFallback,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityState {
    pub persistence: CapabilityPersistence,
    pub verification: CapabilityVerification,
}

#[derive(Debug, Clone)]
pub struct RunnerSnapshot {
    pub mode: ModeState,
    pub capability: CapabilityState,
    pub signals: Arc<BTreeMap<String, f64>>,
    pub sample_at: Option<Instant>,
}

impl RunnerSnapshot {
    pub fn empty() -> Self {
        Self {
            mode: ModeState::Connecting,
            capability: CapabilityState {
                persistence: CapabilityPersistence::Pending,
                verification: CapabilityVerification::Verifying { remaining: 0 },
            },
            signals: Arc::new(BTreeMap::new()),
            sample_at: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistence_and_verification_are_independent() {
        let state = CapabilityState {
            persistence: CapabilityPersistence::SessionOnlyStoreError,
            verification: CapabilityVerification::Ready,
        };
        assert_eq!(
            state.persistence,
            CapabilityPersistence::SessionOnlyStoreError
        );
        assert_eq!(state.verification, CapabilityVerification::Ready);
    }

    #[test]
    fn snapshots_share_unchanged_signal_collections() {
        let snapshot = RunnerSnapshot::empty();
        let signals = Arc::clone(&snapshot.signals);
        assert_eq!(Arc::strong_count(&snapshot.signals), 2);
        assert!(signals.is_empty());
    }
}
