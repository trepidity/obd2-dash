use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use crate::profiles::ProfileId;

/// One DTC as presented to a snapshot consumer.  This intentionally does
/// not expose core's mutable protocol type: the runner owns a stable,
/// transport-independent diagnostic result after the scan completes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiagnosticDtcKey {
    pub code: String,
    pub status: DiagnosticDtcStatus,
    pub module: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DiagnosticDtcStatus {
    Stored,
    Pending,
    Permanent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticDtcOrigin {
    Standard,
    Profile { profile_id: String },
}

/// Decoded DTC data retained from an operator-initiated diagnostic pass.
/// `raw` remains available for evidence/export, while the remaining fields
/// are directly consumable by a GUI without re-decoding OBD payload bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticDtc {
    pub key: DiagnosticDtcKey,
    pub origin: DiagnosticDtcOrigin,
    pub description: Option<String>,
    pub notes: Option<String>,
    pub severity: Option<String>,
    pub status_raw: Option<u8>,
    pub status_flags: Vec<String>,
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticResultErrorKind {
    NoData,
    Unsupported,
    Other,
}

/// A single PID sample from a Mode-02 freeze frame.  A failure is retained
/// beside the PID rather than dropping the whole frame: sparse freeze-frame
/// support is normal on older ECUs.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticFreezeFrameReading {
    pub pid: u8,
    pub value: Option<f64>,
    pub unit: Option<String>,
    pub raw: Vec<u8>,
    pub error: Option<DiagnosticResultError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticResultError {
    pub kind: DiagnosticResultErrorKind,
    pub detail: String,
}

/// Freeze-frame data is keyed to the exact decoded DTC (code, lifecycle
/// status, and module), not merely to the scan that happened to request it.
#[derive(Debug, Clone, PartialEq)]
pub struct DiagnosticFreezeFrame {
    pub dtc: DiagnosticDtcKey,
    pub readings: Vec<DiagnosticFreezeFrameReading>,
}

/// The last completed or partially completed operator diagnostic pass.
/// Standard and profile DTCs remain separate so callers cannot mistake an
/// OEM decoder result for a generic Mode-03 response.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DiagnosticResult {
    /// True only after an operator-initiated diagnostic pass reached its
    /// normal completion. An empty pre-scan result is not a clean scan.
    pub completed: bool,
    pub standard_dtcs: Vec<DiagnosticDtc>,
    pub profile_dtcs: Vec<DiagnosticDtc>,
    pub freeze_frames: Vec<DiagnosticFreezeFrame>,
    /// Dispatch evidence from profile DTC routing.  The GUI recording worker
    /// consumes these records without re-entering ProfileRuntime or Session.
    pub profile_evidence: Vec<crate::profiles::ProfileEvidenceRecord>,
}

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

/// Connection metadata that the runner has observed without exposing its
/// session or adapter to snapshot consumers. Missing values are meaningful:
/// an older vehicle may provide standard telemetry without answering Mode 09.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConnectionMetadata {
    pub vin: Option<String>,
    pub protocol: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RunnerSnapshot {
    pub mode: ModeState,
    pub capability: CapabilityState,
    pub connection: ConnectionMetadata,
    /// Profile selected by the runner. A manually confirmed profile remains
    /// explicit so presentation consumers do not infer it from a VIN string.
    pub selected_profile: Option<ProfileId>,
    pub profile_manually_confirmed: bool,
    /// Adapter-reported supply voltage from `ATRV`, not SAE PID 01 42.
    pub adapter_voltage: Option<f64>,
    pub signals: Arc<BTreeMap<String, f64>>,
    pub diagnostic: Arc<DiagnosticResult>,
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
            connection: ConnectionMetadata::default(),
            selected_profile: None,
            profile_manually_confirmed: false,
            adapter_voltage: None,
            signals: Arc::new(BTreeMap::new()),
            diagnostic: Arc::new(DiagnosticResult::default()),
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

    #[test]
    fn empty_snapshot_has_an_empty_typed_diagnostic_result() {
        let snapshot = RunnerSnapshot::empty();
        assert!(snapshot.diagnostic.standard_dtcs.is_empty());
        assert!(snapshot.diagnostic.profile_dtcs.is_empty());
        assert!(snapshot.diagnostic.freeze_frames.is_empty());
    }

    #[test]
    fn empty_snapshot_does_not_invent_connection_metadata_or_voltage() {
        let snapshot = RunnerSnapshot::empty();
        assert_eq!(snapshot.connection, ConnectionMetadata::default());
        assert_eq!(snapshot.adapter_voltage, None);
    }
}
