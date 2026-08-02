//! GUI-only presentation conversion for the shared mode runner.
//!
//! This module deliberately consumes the runner's immutable snapshot.  It
//! neither owns a Session nor performs a diagnostic request; the conversion is
//! therefore safe to call from a Tauri command at any cadence.

use obd2_dash::mode_runner::{
    CapabilityPersistence, CapabilityVerification, DiagnosticDtc, DiagnosticDtcOrigin, ModeState,
    RunnerSnapshot,
};
use serde::Serialize;

const GUI_POLL_MS: u16 = 500;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StatusValue {
    pub label: String,
    pub value: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DtcSnapshot {
    pub code: String,
    pub module: String,
    pub status: String,
    pub description: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ModuleScan {
    pub module: String,
    pub standard: String,
    pub gm_all: String,
    pub gm_active: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SignalSnapshot {
    pub key: String,
    pub label: String,
    pub category: String,
    pub module: String,
    pub unit: String,
    pub value: Option<f64>,
    pub state: String,
    pub confidence: String,
    pub provenance: Vec<String>,
    pub source_fields: Option<SignalSourceFields>,
    pub request: Option<String>,
    pub decoder_id: Option<String>,
    pub evidence_policy: String,
    pub failure_policy: String,
    pub preferred_over: Option<String>,
    pub evidence: Option<SignalEvidence>,
    pub composition: SignalComposition,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignalSourceFields {
    pub txd: String,
    pub rxf: Option<String>,
    pub rxd: Option<SignalRxdSource>,
    pub raw_mth: Option<String>,
    pub source_ref: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignalRxdSource {
    pub raw: String,
    pub bit_width: Option<u8>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SignalEvidence {
    pub key: String,
    pub label: String,
    pub module: String,
    pub node: String,
    pub request: String,
    pub source: String,
    pub confidence: String,
    pub status: String,
    pub unit: String,
    pub value: Option<f64>,
    pub response: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalComposition {
    Scalar,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilitySection {
    pub id: String,
    pub category: String,
    pub label: String,
    pub signal_keys: Vec<String>,
    pub active_test_keys: Vec<String>,
    pub diagnostic_service_keys: Vec<String>,
    pub visible: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunnerModeDto {
    Connecting,
    Discovering {
        origin: String,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilityStateDto {
    pub persistence: String,
    pub verification: String,
    pub remaining: Option<usize>,
}

/// Wire-compatible shell used by the existing React client.  Fields whose
/// source used to be inline-backend-only remain present but empty until their
/// runner-owned equivalents are added; no command handler synthesizes serial
/// data to fill them.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct DiagnosticSnapshot {
    pub mode: RunnerModeDto,
    pub capability_state: CapabilityStateDto,
    /// The runner has no completed foreground-result record yet.  Keep this
    /// field explicit and null rather than fabricating a result from mode
    /// transitions; later runner support can fill it without another JSON
    /// shape change.
    pub foreground_result: Option<serde_json::Value>,
    pub vehicle: String,
    pub vin: String,
    pub protocol: String,
    pub connection: String,
    pub voltage: f64,
    pub rpm: u16,
    pub speed_mph: u16,
    pub poll_ms: u16,
    pub units: String,
    pub statuses: Vec<StatusValue>,
    pub alerts: Vec<String>,
    pub dtcs: Vec<DtcSnapshot>,
    pub modules: Vec<ModuleScan>,
    pub source_confidence: Vec<SignalEvidence>,
    pub signals: Vec<SignalSnapshot>,
    pub capability_sections: Vec<CapabilitySection>,
    pub active_tests_v2: Vec<serde_json::Value>,
}

impl From<&RunnerSnapshot> for DiagnosticSnapshot {
    fn from(snapshot: &RunnerSnapshot) -> Self {
        let dtcs = snapshot
            .diagnostic
            .standard_dtcs
            .iter()
            .chain(snapshot.diagnostic.profile_dtcs.iter())
            .map(dtc_snapshot)
            .collect::<Vec<_>>();
        let signals = snapshot
            .signals
            .iter()
            .map(|(key, value)| standard_signal(key, *value))
            .collect::<Vec<_>>();
        let signal_keys = signals.iter().map(|signal| signal.key.clone()).collect();
        let alerts = alerts(snapshot);

        Self {
            mode: mode_dto(&snapshot.mode),
            capability_state: capability_state_dto(snapshot),
            foreground_result: None,
            // Identity/protocol presentation will move onto RunnerSnapshot as
            // part of the GUI connector bootstrap.  These stable placeholders
            // are preferable to re-reading a Session from the Tauri thread.
            vehicle: "Live OBD-II".to_string(),
            vin: "--".to_string(),
            protocol: "--".to_string(),
            connection: connection_label(&snapshot.mode),
            voltage: snapshot.signals.get("0142").copied().unwrap_or_default(),
            rpm: rounded_signal(snapshot, "010C"),
            speed_mph: speed_mph(snapshot),
            poll_ms: GUI_POLL_MS,
            units: "US".to_string(),
            statuses: vec![
                StatusValue {
                    label: "DTCs".to_string(),
                    value: dtcs.len().to_string(),
                    state: if dtcs.is_empty() { "ok" } else { "warn" }.to_string(),
                },
                StatusValue {
                    label: "Runner".to_string(),
                    value: mode_label(&snapshot.mode),
                    state: mode_status(&snapshot.mode).to_string(),
                },
                StatusValue {
                    label: "Capability".to_string(),
                    value: capability_label(snapshot),
                    state: capability_status(snapshot).to_string(),
                },
            ],
            alerts,
            dtcs,
            modules: Vec::new(),
            source_confidence: Vec::new(),
            signals,
            capability_sections: vec![CapabilitySection {
                id: "powertrain".to_string(),
                category: "Powertrain".to_string(),
                label: "Powertrain".to_string(),
                signal_keys,
                active_test_keys: Vec::new(),
                diagnostic_service_keys: Vec::new(),
                visible: true,
            }],
            active_tests_v2: Vec::new(),
        }
    }
}

fn mode_dto(mode: &ModeState) -> RunnerModeDto {
    match mode {
        ModeState::Connecting => RunnerModeDto::Connecting,
        ModeState::Discovering {
            origin,
            step,
            total,
        } => RunnerModeDto::Discovering {
            origin: match origin {
                obd2_dash::mode_runner::snapshot::DiscoveryOrigin::Initial => "initial",
                obd2_dash::mode_runner::snapshot::DiscoveryOrigin::Rescan => "rescan",
            }
            .to_string(),
            step: *step,
            total: *total,
        },
        ModeState::Telemetry => RunnerModeDto::Telemetry,
        ModeState::Diagnostic {
            phase,
            phase_total,
            step,
            total,
        } => RunnerModeDto::Diagnostic {
            phase: *phase,
            phase_total: *phase_total,
            step: *step,
            total: *total,
        },
        ModeState::Reconnecting { attempt } => RunnerModeDto::Reconnecting { attempt: *attempt },
        ModeState::ShuttingDown => RunnerModeDto::ShuttingDown,
    }
}

fn capability_state_dto(snapshot: &RunnerSnapshot) -> CapabilityStateDto {
    let (verification, remaining) = match &snapshot.capability.verification {
        CapabilityVerification::Ready => ("ready", None),
        CapabilityVerification::Verifying { remaining } => ("verifying", Some(*remaining)),
        CapabilityVerification::Degraded { unresolved } => ("degraded", Some(*unresolved)),
        CapabilityVerification::ConservativeFallback => ("conservative_fallback", None),
    };
    let persistence = match snapshot.capability.persistence {
        CapabilityPersistence::Cached => "cached",
        CapabilityPersistence::Pending => "pending",
        CapabilityPersistence::SessionOnlyNoVin => "session_only_no_vin",
        CapabilityPersistence::SessionOnlyStoreError => "session_only_store_error",
    };
    CapabilityStateDto {
        persistence: persistence.to_string(),
        verification: verification.to_string(),
        remaining,
    }
}

fn dtc_snapshot(dtc: &DiagnosticDtc) -> DtcSnapshot {
    let origin = match &dtc.origin {
        DiagnosticDtcOrigin::Standard => "standard",
        DiagnosticDtcOrigin::Profile { profile_id } => profile_id.as_str(),
    };
    DtcSnapshot {
        code: dtc.key.code.clone(),
        module: dtc
            .key
            .module
            .clone()
            .unwrap_or_else(|| "broadcast".to_string()),
        status: format!("{:?} ({origin})", dtc.key.status).to_lowercase(),
        description: dtc.description.clone(),
        notes: dtc.notes.clone(),
    }
}

fn standard_signal(key: &str, value: f64) -> SignalSnapshot {
    let (label, unit) = match key {
        "010C" => ("Engine RPM", "rpm"),
        "010D" => ("Vehicle speed", "mph"),
        "0105" => ("Coolant temperature", "F"),
        "010B" => ("Intake manifold pressure", "kPa"),
        "010F" => ("Intake air temperature", "F"),
        "0110" => ("Mass air flow", "g/s"),
        "0142" => ("Control module voltage", "V"),
        _ => ("OBD-II signal", ""),
    };
    let value = display_value(key, value);
    SignalSnapshot {
        key: key.to_string(),
        label: label.to_string(),
        category: "Powertrain".to_string(),
        module: "broadcast".to_string(),
        unit: unit.to_string(),
        value: Some(value),
        state: "ok".to_string(),
        confidence: "LiveObserved".to_string(),
        provenance: vec!["shared mode runner".to_string()],
        source_fields: None,
        request: Some(key.to_string()),
        decoder_id: None,
        evidence_policy: "normal".to_string(),
        failure_policy: "retain".to_string(),
        preferred_over: None,
        evidence: None,
        composition: SignalComposition::Scalar,
    }
}

fn rounded_signal(snapshot: &RunnerSnapshot, key: &str) -> u16 {
    snapshot
        .signals
        .get(key)
        .copied()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| value.round().min(f64::from(u16::MAX)) as u16)
        .unwrap_or_default()
}

fn speed_mph(snapshot: &RunnerSnapshot) -> u16 {
    snapshot
        .signals
        .get("010D")
        .copied()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|kph| (kph * 0.621_371).round().min(f64::from(u16::MAX)) as u16)
        .unwrap_or_default()
}

/// `Session::read_pid` reports these standard PID values in their protocol
/// units.  The existing GUI contract is US display units, so convert only at
/// the presentation boundary; the runner snapshot remains lossless SI.
fn display_value(key: &str, value: f64) -> f64 {
    match key {
        "010D" => value * 0.621_371,
        "0105" | "010F" => value * 9.0 / 5.0 + 32.0,
        _ => value,
    }
}

fn connection_label(mode: &ModeState) -> String {
    match mode {
        ModeState::Telemetry => "runner telemetry".to_string(),
        ModeState::Connecting => "runner connecting".to_string(),
        ModeState::Discovering { .. } => "runner discovering".to_string(),
        ModeState::Diagnostic { .. } => "runner diagnostic".to_string(),
        ModeState::Reconnecting { attempt } => format!("runner reconnecting (attempt {attempt})"),
        ModeState::ShuttingDown => "runner shutting down".to_string(),
    }
}

fn mode_label(mode: &ModeState) -> String {
    match mode {
        ModeState::Telemetry => "telemetry".to_string(),
        ModeState::Connecting => "connecting".to_string(),
        ModeState::Discovering {
            origin,
            step,
            total,
        } => format!("discovering {origin:?} {step}/{total}"),
        ModeState::Diagnostic {
            phase,
            phase_total,
            step,
            total,
        } => format!("diagnostic {phase}/{phase_total}, step {step}/{total}"),
        ModeState::Reconnecting { attempt } => format!("reconnecting {attempt}"),
        ModeState::ShuttingDown => "shutting down".to_string(),
    }
}

fn mode_status(mode: &ModeState) -> &'static str {
    match mode {
        ModeState::Telemetry => "ok",
        ModeState::Connecting | ModeState::Discovering { .. } | ModeState::Diagnostic { .. } => {
            "warn"
        }
        ModeState::Reconnecting { .. } | ModeState::ShuttingDown => "muted",
    }
}

fn capability_label(snapshot: &RunnerSnapshot) -> String {
    match &snapshot.capability.verification {
        CapabilityVerification::Ready => "ready".to_string(),
        CapabilityVerification::Verifying { remaining } => format!("verifying ({remaining})"),
        CapabilityVerification::Degraded { unresolved } => format!("degraded ({unresolved})"),
        CapabilityVerification::ConservativeFallback => "conservative fallback".to_string(),
    }
}

fn capability_status(snapshot: &RunnerSnapshot) -> &'static str {
    match &snapshot.capability.verification {
        CapabilityVerification::Ready => "ok",
        CapabilityVerification::Verifying { .. } => "warn",
        CapabilityVerification::Degraded { .. } | CapabilityVerification::ConservativeFallback => {
            "muted"
        }
    }
}

fn alerts(snapshot: &RunnerSnapshot) -> Vec<String> {
    let mut alerts = Vec::new();
    if matches!(
        &snapshot.capability.persistence,
        CapabilityPersistence::SessionOnlyStoreError
    ) {
        alerts.push("capability persistence is unavailable for this session".to_string());
    }
    if matches!(
        &snapshot.capability.verification,
        CapabilityVerification::Degraded { .. }
    ) {
        alerts.push("some capabilities remain unverified".to_string());
    }
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_dash::mode_runner::{
        snapshot::{DiagnosticDtcKey, DiagnosticDtcStatus, DiagnosticResult},
        CapabilityState, CapabilityVerification,
    };
    use std::{collections::BTreeMap, sync::Arc};

    #[test]
    fn runner_snapshot_conversion_retains_typed_dtc_details() {
        let mut snapshot = RunnerSnapshot::empty();
        snapshot.mode = ModeState::Telemetry;
        snapshot.signals = Arc::new(BTreeMap::from([
            ("010C".to_string(), 742.0),
            ("010D".to_string(), 64.4),
        ]));
        snapshot.diagnostic = Arc::new(DiagnosticResult {
            standard_dtcs: vec![DiagnosticDtc {
                key: DiagnosticDtcKey {
                    code: "P0101".to_string(),
                    status: DiagnosticDtcStatus::Stored,
                    module: Some("ecm".to_string()),
                },
                origin: DiagnosticDtcOrigin::Standard,
                description: Some("MAF range/performance".to_string()),
                notes: None,
                severity: None,
                status_raw: None,
                status_flags: Vec::new(),
                raw: vec![0x01, 0x01],
            }],
            ..DiagnosticResult::default()
        });
        snapshot.capability = CapabilityState {
            persistence: CapabilityPersistence::Cached,
            verification: CapabilityVerification::Ready,
        };

        let dto = DiagnosticSnapshot::from(&snapshot);

        assert_eq!(dto.rpm, 742);
        assert_eq!(dto.speed_mph, 40);
        assert_eq!(dto.dtcs[0].code, "P0101");
        assert_eq!(dto.dtcs[0].module, "ecm");
        assert_eq!(dto.signals[0].key, "010C");
        assert_eq!(dto.statuses[0].value, "1");
        let json = serde_json::to_value(dto).unwrap();
        assert!(json.get("active_tests_v2").is_some());
        assert_eq!(json["mode"]["state"], "telemetry");
        assert_eq!(json["capability_state"]["persistence"], "cached");
        assert!(json["foreground_result"].is_null());
    }
}
