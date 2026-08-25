//! GUI-only presentation conversion for the shared mode runner.
//!
//! This module deliberately consumes the runner's immutable snapshot.  It
//! neither owns a Session nor performs a diagnostic request; the conversion is
//! therefore safe to call from a Tauri command at any cadence.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use obd2_dash::mode_runner::{
    CapabilityPersistence, CapabilityVerification, DiagnosticDtc, DiagnosticDtcOrigin, ModeState,
    RunnerSnapshot,
};
use obd2_dash::profiles::{
    builtin_profile, PairRole, SignalCategory as ProfileSignalCategory,
    SignalComposition as ProfileSignalComposition, SignalDisplaySource, SignalRangeDefinition,
    SignalRangeEvaluation,
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
    pub operating_range: Option<SignalOperatingRange>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SignalOperatingRange {
    pub evaluation: String,
    pub desired_max: f64,
    pub caution_max: f64,
    pub desired_label: String,
    pub caution_label: String,
    pub outside_label: String,
    pub conditions: String,
    pub source_ref: String,
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
    Pair {
        group_key: String,
        role: String,
    },
    TableRow {
        table_key: String,
        row_index: u8,
        row_label: String,
    },
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

/// Wire-compatible shell used by the existing React client. No command
/// handler synthesizes serial data: absent runner observations remain absent
/// or explicitly unread at this presentation boundary.
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
    /// Adapter supply voltage from `ATRV`, when the adapter reports it.
    pub voltage: Option<f64>,
    pub rpm: u16,
    pub speed_mph: u16,
    pub poll_ms: u16,
    /// Age of the latest runner-owned sample when this DTO was built. This
    /// excludes frontend delivery/render time so the GUI can report its own
    /// presentation age independently.
    pub runner_sample_age_ms: Option<u64>,
    /// Approximate wall-clock instant of the runner sample. This is display
    /// telemetry only; runner scheduling remains monotonic.
    pub sample_at_unix_ms: Option<u64>,
    pub units: String,
    pub statuses: Vec<StatusValue>,
    pub alerts: Vec<String>,
    /// Whether the current DTC collection comes from a completed operator
    /// diagnostic pass rather than the empty initial runner state.
    pub dtc_scan_complete: bool,
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
        let mut signals = snapshot
            .signals
            .iter()
            .map(|(key, value)| profile_or_standard_signal(snapshot, key, *value))
            .collect::<Vec<_>>();
        append_profile_derived_signals(snapshot, &mut signals);
        let capability_sections = capability_sections(&signals);
        let alerts = alerts(snapshot);

        let runner_sample_age_ms = sample_age_ms(snapshot.sample_at);
        Self {
            mode: mode_dto(&snapshot.mode),
            capability_state: capability_state_dto(snapshot),
            foreground_result: None,
            vehicle: "Live OBD-II".to_string(),
            vin: snapshot
                .connection
                .vin
                .clone()
                .unwrap_or_else(|| "unread".to_string()),
            protocol: snapshot
                .connection
                .protocol
                .clone()
                .unwrap_or_else(|| "unresolved".to_string()),
            connection: connection_label(&snapshot.mode),
            voltage: snapshot.adapter_voltage,
            rpm: rounded_signal(snapshot, "010C"),
            speed_mph: speed_mph(snapshot),
            poll_ms: GUI_POLL_MS,
            runner_sample_age_ms,
            sample_at_unix_ms: sample_at_unix_ms(runner_sample_age_ms),
            units: "US".to_string(),
            statuses: vec![
                StatusValue {
                    label: "DTCs".to_string(),
                    value: if snapshot.diagnostic.completed {
                        dtcs.len().to_string()
                    } else {
                        "--".to_string()
                    },
                    state: if !snapshot.diagnostic.completed {
                        "muted"
                    } else if dtcs.is_empty() {
                        "ok"
                    } else {
                        "warn"
                    }
                    .to_string(),
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
            dtc_scan_complete: snapshot.diagnostic.completed,
            dtcs,
            modules: Vec::new(),
            source_confidence: Vec::new(),
            signals,
            capability_sections,
            active_tests_v2: Vec::new(),
        }
    }
}

fn capability_sections(signals: &[SignalSnapshot]) -> Vec<CapabilitySection> {
    const CATEGORIES: &[(&str, &str, &str)] = &[
        ("powertrain", "Powertrain", "Powertrain"),
        ("turbo", "Turbo", "Turbo"),
        ("fuel", "Fuel", "Fuel"),
        ("transmission", "Transmission", "Transmission"),
        ("body", "Body", "Body"),
        ("chassis", "Chassis", "Chassis"),
        ("emissions", "Emissions", "Emissions"),
        ("other", "Other", "Other"),
    ];

    CATEGORIES
        .iter()
        .filter_map(|(id, category, label)| {
            let signal_keys = signals
                .iter()
                .filter(|signal| signal.category == *category)
                .map(|signal| signal.key.clone())
                .collect::<Vec<_>>();
            (!signal_keys.is_empty()).then(|| CapabilitySection {
                id: (*id).to_string(),
                category: (*category).to_string(),
                label: (*label).to_string(),
                signal_keys,
                active_test_keys: Vec::new(),
                diagnostic_service_keys: Vec::new(),
                visible: true,
            })
        })
        .collect()
}

fn sample_age_ms(sample_at: Option<Instant>) -> Option<u64> {
    let elapsed = Instant::now().checked_duration_since(sample_at?)?;
    Some(duration_millis(elapsed))
}

fn sample_at_unix_ms(age_ms: Option<u64>) -> Option<u64> {
    let sample_time = SystemTime::now().checked_sub(Duration::from_millis(age_ms?))?;
    let since_epoch = sample_time.duration_since(UNIX_EPOCH).ok()?;
    Some(duration_millis(since_epoch))
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
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
        operating_range: None,
    }
}

fn profile_or_standard_signal(snapshot: &RunnerSnapshot, key: &str, value: f64) -> SignalSnapshot {
    let Some(profile) = snapshot.selected_profile.and_then(builtin_profile) else {
        return standard_signal(key, value);
    };
    let Some(display) = profile.signal_display().iter().find(|display| {
        matches!(display.source, SignalDisplaySource::ProfileSignal(profile_key) if profile_key == key)
    }) else {
        return standard_signal(key, value);
    };
    SignalSnapshot {
        key: key.to_string(),
        label: display.label.to_string(),
        category: profile_category(display.category).to_string(),
        module: "ecm".to_string(),
        unit: display.unit.to_string(),
        value: Some(value),
        state: "ok".to_string(),
        confidence: "LiveObserved".to_string(),
        provenance: vec!["confirmed profile runtime".to_string()],
        source_fields: None,
        request: Some(key.to_string()),
        decoder_id: Some("profile runtime".to_string()),
        evidence_policy: "normal".to_string(),
        failure_policy: "retain".to_string(),
        preferred_over: None,
        evidence: None,
        composition: profile_composition(display.composition),
        operating_range: display.operating_range.map(profile_operating_range),
    }
}

fn append_profile_derived_signals(snapshot: &RunnerSnapshot, signals: &mut Vec<SignalSnapshot>) {
    let Some(profile) = snapshot.selected_profile.and_then(builtin_profile) else {
        return;
    };
    for display in profile.signal_display() {
        let ProfileSignalDisplaySource::Derived {
            formula_key,
            input_keys,
        } = display.source
        else {
            continue;
        };
        let value_and_state = match formula_key {
            // LLY actual rail pressure falls back to standard Mode 01 PID 23
            // when the enhanced ECM read has not been observed.  That value
            // is stored as kPa by the core decoder but the profile display is
            // explicitly in psi.
            "first_available" => input_keys
                .iter()
                .find_map(|input| derived_input_value(snapshot, signals, input))
                .map(|(input, value)| (derived_display_value(input, display.unit, value), "ok")),
            "actual_minus_desired" if input_keys.len() == 2 => {
                let actual = derived_input_value(snapshot, signals, input_keys[0]);
                let desired = derived_input_value(snapshot, signals, input_keys[1]);
                actual.zip(desired).map(|((_, actual), (_, desired))| {
                    let delta = actual - desired;
                    (delta, rail_delta_state(delta, desired))
                })
            }
            _ => None,
        };
        let Some((value, state)) = value_and_state else {
            continue;
        };
        signals.push(SignalSnapshot {
            key: display.key.to_string(),
            label: display.label.to_string(),
            category: profile_category(display.category).to_string(),
            module: "derived".to_string(),
            unit: display.unit.to_string(),
            value: Some(value),
            state: state.to_string(),
            confidence: "LiveObserved".to_string(),
            provenance: vec!["derived from confirmed profile runtime".to_string()],
            source_fields: None,
            request: None,
            decoder_id: Some(formula_key.to_string()),
            evidence_policy: "normal".to_string(),
            failure_policy: "retain".to_string(),
            preferred_over: None,
            evidence: None,
            composition: profile_composition(display.composition),
            operating_range: display.operating_range.map(profile_operating_range),
        });
    }
}

fn rail_delta_state(delta_psi: f64, desired_psi: f64) -> &'static str {
    if !delta_psi.is_finite() || !desired_psi.is_finite() {
        return "error";
    }

    // Allow normal control/transient error, but surface gross disagreement.
    // A 20% relative band with a 500 psi floor avoids calling small idle
    // fluctuations unhealthy while catching unit/decoder mismatches.
    let tolerance_psi = (desired_psi.abs() * 0.20).max(500.0);
    if delta_psi.abs() <= tolerance_psi {
        "ok"
    } else {
        "warn"
    }
}

fn derived_input_value<'a>(
    snapshot: &RunnerSnapshot,
    signals: &'a [SignalSnapshot],
    input: &'a str,
) -> Option<(&'a str, f64)> {
    if let Some(value) = snapshot.signals.get(input) {
        return Some((input, *value));
    }
    let standard_key = input
        .strip_prefix("standard:")
        .and_then(|pid| u8::from_str_radix(pid, 16).ok())
        .map(|pid| format!("01{pid:02X}"));
    if let Some(key) = standard_key.as_deref() {
        if let Some(value) = snapshot.signals.get(key) {
            return Some((input, *value));
        }
    }
    signals
        .iter()
        .find(|signal| signal.key == input)
        .and_then(|signal| signal.value.map(|value| (input, value)))
}

fn derived_display_value(input: &str, unit: &str, value: f64) -> f64 {
    match (input, unit) {
        ("standard:23", "psi") => value * 0.145_037_738,
        _ => value,
    }
}

// Alias avoids confusing the profile definition source with the DTO enum.
use obd2_dash::profiles::SignalDisplaySource as ProfileSignalDisplaySource;

fn profile_category(category: ProfileSignalCategory) -> &'static str {
    match category {
        ProfileSignalCategory::Powertrain => "Powertrain",
        ProfileSignalCategory::Turbo => "Turbo",
        ProfileSignalCategory::Fuel => "Fuel",
        ProfileSignalCategory::Transmission => "Transmission",
        ProfileSignalCategory::Body => "Body",
        ProfileSignalCategory::Chassis => "Chassis",
        ProfileSignalCategory::Emissions => "Emissions",
        ProfileSignalCategory::Other => "Other",
    }
}

fn profile_composition(composition: ProfileSignalComposition) -> SignalComposition {
    match composition {
        ProfileSignalComposition::Scalar => SignalComposition::Scalar,
        ProfileSignalComposition::Pair { group_key, role } => SignalComposition::Pair {
            group_key: group_key.to_string(),
            role: match role {
                PairRole::Actual => "actual",
                PairRole::Desired => "desired",
                PairRole::Error => "error",
                PairRole::Delta => "delta",
            }
            .to_string(),
        },
        ProfileSignalComposition::TableRow {
            table_key,
            row_index,
            row_label,
        } => SignalComposition::TableRow {
            table_key: table_key.to_string(),
            row_index,
            row_label: row_label.to_string(),
        },
    }
}

fn profile_operating_range(range: SignalRangeDefinition) -> SignalOperatingRange {
    SignalOperatingRange {
        evaluation: match range.evaluation {
            SignalRangeEvaluation::AbsoluteMagnitude => "absolute_magnitude",
        }
        .to_string(),
        desired_max: range.desired_max,
        caution_max: range.caution_max,
        desired_label: range.desired_label.to_string(),
        caution_label: range.caution_label.to_string(),
        outside_label: range.outside_label.to_string(),
        conditions: range.conditions.to_string(),
        source_ref: range.source_ref.to_string(),
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
        CapabilityState, CapabilityVerification, ConnectionMetadata,
    };
    use std::{
        collections::BTreeMap,
        sync::Arc,
        time::{Duration, Instant},
    };

    #[test]
    fn runner_snapshot_conversion_retains_typed_dtc_details() {
        let mut snapshot = RunnerSnapshot::empty();
        snapshot.mode = ModeState::Telemetry;
        snapshot.signals = Arc::new(BTreeMap::from([
            ("010C".to_string(), 742.0),
            ("010D".to_string(), 64.4),
        ]));
        snapshot.diagnostic = Arc::new(DiagnosticResult {
            completed: true,
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
        snapshot.connection = ConnectionMetadata {
            vin: Some("1GCHK23224F000001".to_string()),
            protocol: Some("J1850 VPW".to_string()),
        };
        snapshot.adapter_voltage = Some(14.1);
        snapshot.sample_at = Some(Instant::now() - Duration::from_millis(12));

        let dto = DiagnosticSnapshot::from(&snapshot);

        assert_eq!(dto.rpm, 742);
        assert_eq!(dto.speed_mph, 40);
        assert_eq!(dto.dtcs[0].code, "P0101");
        assert_eq!(dto.dtcs[0].module, "ecm");
        assert_eq!(dto.signals[0].key, "010C");
        assert_eq!(dto.statuses[0].value, "1");
        assert_eq!(dto.vin, "1GCHK23224F000001");
        assert_eq!(dto.protocol, "J1850 VPW");
        assert_eq!(dto.voltage, Some(14.1));
        assert!(dto.runner_sample_age_ms.is_some_and(|age| age >= 12));
        assert!(dto.sample_at_unix_ms.is_some());
        let json = serde_json::to_value(dto).unwrap();
        assert!(json.get("active_tests_v2").is_some());
        assert_eq!(json["mode"]["state"], "telemetry");
        assert_eq!(json["capability_state"]["persistence"], "cached");
        assert!(json["foreground_result"].is_null());
    }

    #[test]
    fn absent_identity_and_voltage_remain_explicitly_unavailable() {
        let dto = DiagnosticSnapshot::from(&RunnerSnapshot::empty());

        assert_eq!(dto.vin, "unread");
        assert_eq!(dto.protocol, "unresolved");
        assert_eq!(dto.voltage, None);
    }

    #[test]
    fn lly_fuel_rail_values_share_physical_psi_units() {
        let mut snapshot = RunnerSnapshot::empty();
        snapshot.selected_profile = Some(obd2_dash::profiles::gm::lly::LLY_PROFILE_ID);
        snapshot.signals = Arc::new(BTreeMap::from([
            ("0123".to_string(), 29_060.0),
            ("lly.163D".to_string(), 4_350.0),
        ]));

        let dto = DiagnosticSnapshot::from(&snapshot);
        let actual = dto
            .signals
            .iter()
            .find(|signal| signal.key == "lly.fuel_rail.actual")
            .unwrap();
        let desired = dto
            .signals
            .iter()
            .find(|signal| signal.key == "lly.163D")
            .unwrap();
        let delta = dto
            .signals
            .iter()
            .find(|signal| signal.key == "lly.fuel_rail.delta")
            .unwrap();

        assert!((actual.value.unwrap() - 4_214.796).abs() < 0.01);
        assert_eq!(desired.value, Some(4_350.0));
        assert!((delta.value.unwrap() + 135.204).abs() < 0.01);
        assert_eq!(delta.state, "ok");
    }

    #[test]
    fn gross_fuel_rail_disagreement_is_not_reported_ok() {
        let mut snapshot = RunnerSnapshot::empty();
        snapshot.selected_profile = Some(obd2_dash::profiles::gm::lly::LLY_PROFILE_ID);
        snapshot.signals = Arc::new(BTreeMap::from([
            ("0123".to_string(), 29_060.0),
            ("lly.163D".to_string(), 435.0),
        ]));

        let dto = DiagnosticSnapshot::from(&snapshot);
        let delta = dto
            .signals
            .iter()
            .find(|signal| signal.key == "lly.fuel_rail.delta")
            .unwrap();

        assert!((delta.value.unwrap() - 3_779.796).abs() < 0.01);
        assert_eq!(delta.state, "warn");
    }

    #[test]
    fn lly_injector_balance_carries_profile_operating_range() {
        let mut snapshot = RunnerSnapshot::empty();
        snapshot.selected_profile = Some(obd2_dash::profiles::gm::lly::LLY_PROFILE_ID);
        snapshot.signals = Arc::new(BTreeMap::from([("lly.162F".to_string(), 4.5)]));

        let dto = DiagnosticSnapshot::from(&snapshot);
        let balance = dto
            .signals
            .iter()
            .find(|signal| signal.key == "lly.162F")
            .unwrap();
        let range = balance.operating_range.as_ref().unwrap();

        assert_eq!(range.evaluation, "absolute_magnitude");
        assert_eq!(range.desired_max, 4.0);
        assert_eq!(range.caution_max, 6.0);
        assert_eq!(range.desired_label, "Park/Neutral range");
        assert_eq!(range.caution_label, "Drive-only range");
        assert!(range.conditions.contains("ECT above 180 F"));
        assert!(range.source_ref.contains("GM 2005 LLY"));
    }
}
