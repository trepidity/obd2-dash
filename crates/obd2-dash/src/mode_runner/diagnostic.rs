use std::collections::BTreeSet;

use super::snapshot::{
    DiagnosticDtc, DiagnosticDtcKey, DiagnosticDtcOrigin, DiagnosticDtcStatus,
    DiagnosticFreezeFrame, DiagnosticFreezeFrameReading, DiagnosticResult, DiagnosticResultError,
    DiagnosticResultErrorKind, ModeState,
};
use async_trait::async_trait;
use obd2_core::adapter::Adapter;
use obd2_core::error::Obd2Error;
use obd2_core::protocol::dtc::{Dtc, DtcStatus, Severity};
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::Target;
use obd2_core::session::Session;
use obd2_core::vehicle::Protocol;
use obd2_core::vehicle::{ModuleId, VehicleSpec};
use obd2_db::models::CapabilityOutcome;

use crate::gm_active::{GmActiveTestCommand, GmActiveTestResult};
use crate::profiles::{
    CapabilityId, CoverageMap, DispatchError, NullEvidenceSink, ProfileEvidenceRecord,
    ProfileRegistry, ProfileResponse, ProfileRuntime, SelectedProfile, VehicleContext,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuelClass {
    Gasoline,
    Diesel,
    Other,
    Unknown,
}

/// A raw fuel label is one of three things: a recognized class, an explicit
/// "no claim" (the embedded generic spec ships `fuel_type: unknown`, and a
/// blank field means the same), or an unrecognized claim — a vocabulary gap.
enum FuelLabel {
    Class(FuelClass),
    Absent,
    Unrecognized,
}

/// Resolve fuel using the approved precedence: embedded session identity,
/// then exact-VIN database data, otherwise Unknown.
///
/// Precedence is by SOURCE, not by value: if the curated embedded spec makes
/// a claim we cannot parse, the answer is Unknown — the cached NHTSA row is
/// never consulted to overrule the authoritative source. Only an explicit
/// "unknown"/blank sentinel (spec makes no claim) falls through to the
/// database. Unrecognized labels never resolve to gasoline.
pub fn resolve_fuel(session: Option<&str>, database: Option<&str>) -> FuelClass {
    match session.map(classify_fuel_label) {
        Some(FuelLabel::Class(class)) => class,
        Some(FuelLabel::Unrecognized) => FuelClass::Unknown,
        Some(FuelLabel::Absent) | None => match database.map(classify_fuel_label) {
            Some(FuelLabel::Class(class)) => class,
            _ => FuelClass::Unknown,
        },
    }
}

fn classify_fuel_label(raw: &str) -> FuelLabel {
    match raw.trim().to_ascii_lowercase().as_str() {
        "gasoline" => FuelLabel::Class(FuelClass::Gasoline),
        "diesel" => FuelLabel::Class(FuelClass::Diesel),
        "other" => FuelLabel::Class(FuelClass::Other),
        "" | "unknown" => FuelLabel::Absent,
        _ => FuelLabel::Unrecognized,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticPhase {
    Dtc,
    FreezeFrames,
    Readiness,
    Mode05O2,
    ModuleRefresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiagnosticRequest {
    pub phase: DiagnosticPhase,
    pub service: u8,
    pub target: RequestTarget,
    pub expansion: RequestExpansion,
}

/// Capability-record marker for a selected-profile DTC result.  Keeping the
/// service identity here prevents callers from reconstructing a Mode-03
/// request outside the diagnostic gate module.
pub fn profile_dtc_outcome_request() -> DiagnosticRequest {
    DiagnosticRequest {
        phase: DiagnosticPhase::Dtc,
        service: 0x03,
        target: RequestTarget::Broadcast,
        expansion: RequestExpansion::Static,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestTarget {
    Broadcast,
    /// Summary marker in [`request_plan`]: "fan out per discovered module".
    /// Wire execution replaces it via [`expand_dtc_requests`], which emits
    /// [`RequestTarget::Module`] entries instead.
    DiscoveredModules,
    /// One concrete module, as an index into the caller's module slice —
    /// expansion would be unusable for I/O without the binding.
    Module(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestExpansion {
    Static,
    PerDtc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticExecution {
    pub completed: usize,
    /// Non-transport step failures recorded while the bundle continued.
    pub step_errors: usize,
    pub cancelled: bool,
}

/// Result of one diagnostic request. Spec §11: a step failure (NO DATA,
/// negative response, decode error) is recorded and the bundle CONTINUES —
/// only transport loss aborts, via the outer `Err`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    Data(Vec<u8>),
    StepError { kind: StepErrorKind, detail: String },
}

/// Freeze-frame work derived from a retained decoded DTC.  The exact DTC key
/// stays attached to the work so a response can never be published against a
/// different code merely because two scans overlap in time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreezeFrameWork {
    pub dtc: DiagnosticDtcKey,
    pub pids: Vec<Pid>,
}

/// Retained response from a selected-profile DTC service.  Unlike the older
/// `StepResult`-only API, this keeps the profile decoder's actual output.
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileDtcExecution {
    pub result: StepResult,
    pub dtcs: Vec<crate::profiles::DecodedDtc>,
}

const DEFAULT_FREEZE_FRAME_PIDS: [Pid; 8] = [
    Pid::ENGINE_RPM,
    Pid::VEHICLE_SPEED,
    Pid::COOLANT_TEMP,
    Pid::ENGINE_LOAD,
    Pid::SHORT_FUEL_TRIM_B1,
    Pid::LONG_FUEL_TRIM_B1,
    Pid::THROTTLE_POSITION,
    Pid::MAF,
];

/// The result of a runner-owned active-test request.
///
/// Active tests are presently locked because no verified Class-2 actuator
/// bytes exist.  The command is still an auditable operator action: this
/// carries both evidence formats the lifecycle must publish, and never takes
/// a [`Session`] reference.  That makes an adapter write structurally
/// impossible while the profile remains locked.
#[derive(Debug, Clone)]
pub struct LockedActiveTestExecution {
    pub result: GmActiveTestResult,
    pub profile_evidence: ProfileEvidenceRecord,
    /// Set when the append-only raw evidence file could not be flushed.  The
    /// lifecycle must surface this; a failed audit write is not a successful
    /// evidence capture.
    pub evidence_write_error: Option<String>,
}

/// Record a locked active-test request without blocking the runner's async
/// task on filesystem I/O.
///
/// Lifecycle integration: on an accepted `RequestActiveTest`, await this
/// helper, publish `profile_evidence`, then publish `result`; log or display
/// `evidence_write_error` as an audit failure.  No Session call belongs in
/// that branch until a profile owns verified actuator bytes and a separately
/// reviewed executor exists.
pub async fn execute_locked_active_test(command: GmActiveTestCommand) -> LockedActiveTestExecution {
    let mut result = crate::gm_active::blocked_active_test_result(&command);
    let write_command = command.clone();
    let write_result = result.clone();
    let evidence_write_error = match tokio::task::spawn_blocking(move || {
        crate::gm_active::write_active_test_evidence(&write_command, &write_result)
    })
    .await
    {
        Ok(Ok(path)) => {
            result.evidence_path = Some(path.display().to_string());
            None
        }
        Ok(Err(error)) => Some(format!("active-test evidence write failed: {error}")),
        Err(error) => Some(format!("active-test evidence task failed: {error}")),
    };
    let profile_evidence =
        crate::profiles::gm::active::active_test_profile_evidence_record(&command, &result);

    LockedActiveTestExecution {
        result,
        profile_evidence,
        evidence_write_error,
    }
}

/// Typed classification of a step failure, assigned from the TYPED core
/// error in [`map_obd2_result`] — never recovered from display strings.
/// (String matching against `Obd2Error` Display output silently fails:
/// `NoData` renders as "no data (vehicle did not respond)" and negative
/// responses render Debug-style with no spaces.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepErrorKind {
    /// Vehicle did not answer; prunable only after separated confirmation.
    NoData,
    /// Explicitly rejected: unsupported PID/service or request-out-of-range.
    Unsupported,
    /// Everything else (timeout, decode, stale): stays unverified.
    Other,
}

/// A bundle aborted by transport loss. Partial progress is preserved so the
/// interrupted result can be reported (spec §13: "interrupted", not lost).
#[derive(Debug)]
pub struct DiagnosticAborted {
    pub completed: usize,
    pub step_errors: usize,
    pub error: anyhow::Error,
}

#[async_trait]
pub trait DiagnosticTransport {
    /// `Ok(StepResult)` for everything the bundle records and moves past,
    /// including per-step failures. `Err` is reserved for transport loss.
    async fn request(&mut self, request: DiagnosticRequest) -> anyhow::Result<StepResult>;
}

/// Classify a core result at the diagnostic transport boundary. Bundle-fatal
/// (`Err`) means the serial link itself is gone: `Transport` and `Io` (a
/// yanked USB adapter surfaces as an io error — treating it as a step error
/// would grind through every remaining request at full timeout instead of
/// reconnecting). `Timeout` stays a recorded step error: the spec's §8.2
/// error taxonomy lists it separately from `transport`, and a single request
/// can time out on a busy bus while the link is healthy.
pub fn map_obd2_result(result: Result<Vec<u8>, Obd2Error>) -> anyhow::Result<StepResult> {
    use obd2_core::error::NegativeResponse;
    match result {
        Ok(bytes) => Ok(StepResult::Data(bytes)),
        Err(Obd2Error::Transport(error)) => Err(anyhow::anyhow!("transport: {error}")),
        Err(Obd2Error::Io(error)) => Err(anyhow::Error::new(error).context("serial io")),
        Err(error) => {
            let kind = match &error {
                Obd2Error::NoData => StepErrorKind::NoData,
                Obd2Error::UnsupportedPid { .. } => StepErrorKind::Unsupported,
                Obd2Error::NegativeResponse {
                    nrc:
                        NegativeResponse::ServiceNotSupported
                        | NegativeResponse::SubFunctionNotSupported
                        | NegativeResponse::RequestOutOfRange,
                    ..
                } => StepErrorKind::Unsupported,
                _ => StepErrorKind::Other,
            };
            Ok(StepResult::StepError {
                kind,
                detail: error.to_string(),
            })
        }
    }
}

impl From<DtcStatus> for DiagnosticDtcStatus {
    fn from(status: DtcStatus) -> Self {
        match status {
            DtcStatus::Stored => Self::Stored,
            DtcStatus::Pending => Self::Pending,
            DtcStatus::Permanent => Self::Permanent,
        }
    }
}

fn dtc_status_for_service(service: u8) -> Option<DtcStatus> {
    match service {
        0x03 => Some(DtcStatus::Stored),
        0x07 => Some(DtcStatus::Pending),
        0x0A => Some(DtcStatus::Permanent),
        _ => None,
    }
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "info",
        Severity::Low => "low",
        Severity::Medium => "medium",
        Severity::High => "high",
        Severity::Critical => "critical",
    }
}

fn module_for_request(request: DiagnosticRequest, modules: &[ModuleId]) -> Option<String> {
    match request.target {
        RequestTarget::Module(index) => modules.get(index).map(|module| module.0.clone()),
        RequestTarget::Broadcast | RequestTarget::DiscoveredModules => None,
    }
}

/// Decode one Mode-03/07/0A payload into snapshot-safe DTC records.  The
/// session response is deliberately decoded here while its service, target,
/// and matched spec are still available; carrying only raw bytes to the GUI
/// previously made every successful DTC response disappear.
pub fn decode_standard_dtc_payload(
    request: DiagnosticRequest,
    payload: &[u8],
    modules: &[ModuleId],
    spec: Option<&VehicleSpec>,
) -> Vec<DiagnosticDtc> {
    let Some(status) = dtc_status_for_service(request.service) else {
        return Vec::new();
    };
    let module = module_for_request(request, modules);
    let mut decoded = Vec::new();
    let mut raw_records = Vec::new();
    for pair in payload.chunks_exact(2) {
        if pair == [0, 0] {
            continue;
        }
        let mut dtc = Dtc::from_bytes(pair[0], pair[1]);
        dtc.status = status;
        dtc.source_module = module.clone();
        raw_records.push([pair[0], pair[1]]);
        decoded.push(dtc);
    }
    obd2_core::session::diagnostics::enrich_dtcs(&mut decoded, spec);

    decoded
        .into_iter()
        .zip(raw_records)
        .map(|(dtc, raw)| DiagnosticDtc {
            key: DiagnosticDtcKey {
                code: dtc.code,
                status: dtc.status.into(),
                module: dtc.source_module,
            },
            origin: DiagnosticDtcOrigin::Standard,
            description: dtc.description,
            notes: dtc.notes,
            severity: dtc.severity.map(severity_label).map(str::to_owned),
            status_raw: None,
            status_flags: Vec::new(),
            raw: raw.to_vec(),
        })
        .collect()
}

fn profile_dtc_to_snapshot(profile_id: &str, dtc: crate::profiles::DecodedDtc) -> DiagnosticDtc {
    DiagnosticDtc {
        key: DiagnosticDtcKey {
            code: dtc.code,
            status: dtc.status.into(),
            module: dtc.module.map(|module| module.0),
        },
        origin: DiagnosticDtcOrigin::Profile {
            profile_id: profile_id.to_owned(),
        },
        description: None,
        notes: dtc.notes,
        severity: None,
        status_raw: dtc.status_raw,
        status_flags: dtc.status_flags,
        raw: dtc.raw,
    }
}

impl DiagnosticResult {
    /// Retain standard DTCs from this request, if it completed with payload.
    pub fn record_standard_dtc_payload(
        &mut self,
        request: DiagnosticRequest,
        result: &StepResult,
        modules: &[ModuleId],
        spec: Option<&VehicleSpec>,
    ) {
        let StepResult::Data(payload) = result else {
            return;
        };
        self.standard_dtcs
            .extend(decode_standard_dtc_payload(request, payload, modules, spec));
    }

    /// Retain the `ProfileRuntime` decode result; profile routing and decoder
    /// ownership remain with the profile rather than being recreated here.
    pub fn record_profile_dtcs(
        &mut self,
        profile_id: &str,
        dtcs: Vec<crate::profiles::DecodedDtc>,
    ) {
        self.profile_dtcs.extend(
            dtcs.into_iter()
                .map(|dtc| profile_dtc_to_snapshot(profile_id, dtc)),
        );
    }

    /// Build explicit Mode-02 work after DTC collection.  Known-spec
    /// `related_pids` win; an absent correlation gets the bounded headline
    /// fallback so a valid DTC can never structurally skip freeze-frame work.
    pub fn freeze_frame_work(&self, spec: Option<&VehicleSpec>) -> Vec<FreezeFrameWork> {
        let mut seen = BTreeSet::new();
        let mut work = Vec::new();
        for dtc in self.standard_dtcs.iter().chain(&self.profile_dtcs) {
            if !seen.insert(dtc.key.clone()) {
                continue;
            }
            let pids = spec
                .and_then(|spec| spec.lookup_dtc(&dtc.key.code))
                .and_then(|entry| entry.related_pids.as_deref())
                .map(|related| {
                    related
                        .iter()
                        .filter_map(|pid| u8::try_from(*pid).ok())
                        .map(Pid)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| DEFAULT_FREEZE_FRAME_PIDS.to_vec());
            work.push(FreezeFrameWork {
                dtc: dtc.key.clone(),
                pids,
            });
        }
        work
    }

    pub fn record_freeze_frame_reading(
        &mut self,
        work: &FreezeFrameWork,
        reading: DiagnosticFreezeFrameReading,
    ) {
        let frame = match self
            .freeze_frames
            .iter_mut()
            .find(|frame| frame.dtc == work.dtc)
        {
            Some(frame) => frame,
            None => {
                self.freeze_frames.push(DiagnosticFreezeFrame {
                    dtc: work.dtc.clone(),
                    readings: Vec::new(),
                });
                // The frame was just appended and is therefore present.
                self.freeze_frames
                    .last_mut()
                    .expect("invariant: appended frame")
            }
        };
        frame.readings.push(reading);
    }
}

fn snapshot_error_from_step(result: StepResult) -> DiagnosticResultError {
    match result {
        StepResult::StepError { kind, detail } => DiagnosticResultError {
            kind: match kind {
                StepErrorKind::NoData => DiagnosticResultErrorKind::NoData,
                StepErrorKind::Unsupported => DiagnosticResultErrorKind::Unsupported,
                StepErrorKind::Other => DiagnosticResultErrorKind::Other,
            },
            detail,
        },
        StepResult::Data(_) => unreachable!("freeze-frame error conversion requires step error"),
    }
}

/// Execute one concrete PID within previously derived freeze-frame work.
/// Normal protocol failures become per-PID retained errors; only link loss
/// returns `Err` so lifecycle can reconnect without losing earlier samples.
pub async fn execute_freeze_frame_pid<A: Adapter>(
    session: &mut Session<A>,
    pid: Pid,
) -> anyhow::Result<DiagnosticFreezeFrameReading> {
    match session.read_freeze_frame(pid, 0).await {
        Ok(reading) => match reading.value.as_f64() {
            Ok(value) => Ok(DiagnosticFreezeFrameReading {
                pid: pid.0,
                value: Some(value),
                unit: Some(reading.unit.to_owned()),
                raw: reading.raw_bytes,
                error: None,
            }),
            Err(error) => Ok(DiagnosticFreezeFrameReading {
                pid: pid.0,
                value: None,
                unit: Some(reading.unit.to_owned()),
                raw: reading.raw_bytes,
                error: Some(DiagnosticResultError {
                    kind: DiagnosticResultErrorKind::Other,
                    detail: error.to_string(),
                }),
            }),
        },
        Err(error) => match map_obd2_result(Err(error))? {
            as_step_error @ StepResult::StepError { .. } => Ok(DiagnosticFreezeFrameReading {
                pid: pid.0,
                value: None,
                unit: None,
                raw: Vec::new(),
                error: Some(snapshot_error_from_step(as_step_error)),
            }),
            StepResult::Data(_) => unreachable!("error mapping cannot produce data"),
        },
    }
}

/// Convert a completed diagnostic service result into the persisted capability
/// state. A bare no-data response is provisional; only a separated second
/// no-data confirmation is prunable as Unsupported.
pub fn capability_outcome(step: &StepResult, prior_no_data: bool) -> CapabilityOutcome {
    match step {
        StepResult::Data(_) => CapabilityOutcome::Supported,
        StepResult::StepError {
            kind: StepErrorKind::NoData,
            ..
        } => {
            if prior_no_data {
                CapabilityOutcome::Unsupported
            } else {
                CapabilityOutcome::Unverified
            }
        }
        StepResult::StepError {
            kind: StepErrorKind::Unsupported,
            ..
        } => CapabilityOutcome::Unsupported,
        StepResult::StepError {
            kind: StepErrorKind::Other,
            ..
        } => CapabilityOutcome::Unverified,
    }
}

/// Execute one already-authorized diagnostic request through the Session
/// boundary.  This deliberately lives beside the diagnostic service gate: no
/// other mode-runner module composes a 03/07/0A request.
///
/// The returned error is reserved for a lost serial link.  All normal vehicle
/// and decode failures are converted to [`StepResult::StepError`] so the
/// caller can record an outcome and continue the bundle.
pub async fn execute_session_request<A: Adapter>(
    session: &mut Session<A>,
    mode: &ModeState,
    request: DiagnosticRequest,
    modules: &[ModuleId],
) -> anyhow::Result<StepResult> {
    let target = match request.target {
        RequestTarget::Broadcast | RequestTarget::DiscoveredModules => Target::Broadcast,
        RequestTarget::Module(index) => match modules.get(index) {
            Some(module) => Target::Module(module.0.clone()),
            None => {
                return Ok(StepResult::StepError {
                    kind: StepErrorKind::Other,
                    detail: format!("diagnostic module index {index} is no longer available"),
                });
            }
        },
    };

    let result = match request.phase {
        DiagnosticPhase::Dtc => {
            if !service_allowed(mode, request.service) {
                return Ok(StepResult::StepError {
                    kind: StepErrorKind::Other,
                    detail: "DTC request denied outside Diagnostic mode".into(),
                });
            }
            session.raw_request(request.service, &[], target).await
        }
        // Freeze-frame correlation is expanded by the runner after the DTC
        // phase.  A request here uses the stable headline PID as a bounded
        // fallback when a code has no richer correlated PID list yet.
        DiagnosticPhase::FreezeFrames => session
            .read_freeze_frame(Pid::ENGINE_RPM, 0)
            .await
            .map(|reading| reading.raw_bytes),
        DiagnosticPhase::Readiness => session.read_readiness().await.map(|_| Vec::new()),
        // A bare 05 request is not an OBD service operation.  The core
        // Session API owns Mode-05's required TID/sensor request matrix and
        // response decoding; it intentionally broadcasts those requests.
        DiagnosticPhase::Mode05O2 => {
            let _ = target;
            session.read_all_o2_monitoring().await.map(|_| Vec::new())
        }
        // Module refresh asks Mode 01 PID 00 through each resolved target;
        // callers expand DiscoveredModules before reaching this boundary.
        DiagnosticPhase::ModuleRefresh => session.raw_request(0x01, &[0x00], target).await,
    };
    map_obd2_result(result)
}

/// Run the selected profile's DTC services through `ProfileRuntime`, rather
/// than reconstructing its Class-2 routing in the mode runner.  The profile
/// scheduler is invoked at a common maintenance boundary so every configured
/// DTC service is considered for this operator-initiated scan.
pub async fn execute_profile_dtc_requests<A: Adapter>(
    session: &mut Session<A>,
    context: &VehicleContext,
    selected: &SelectedProfile,
) -> Vec<anyhow::Result<StepResult>> {
    execute_profile_dtc_results(session, context, selected)
        .await
        .into_iter()
        .map(|result| result.map(|execution| execution.result))
        .collect()
}

/// Same profile DTC execution path as [`execute_profile_dtc_requests`], but
/// retains `ProfileResponse::Dtcs` for the runner snapshot.
pub async fn execute_profile_dtc_results<A: Adapter>(
    session: &mut Session<A>,
    context: &VehicleContext,
    selected: &SelectedProfile,
) -> Vec<anyhow::Result<ProfileDtcExecution>> {
    let registry = ProfileRegistry::with_builtins();
    let runtime = ProfileRuntime::new(&registry);
    let coverage = CoverageMap::new(Vec::new()).with_discovered_modules(
        context
            .discovered_modules
            .iter()
            .filter_map(profile_module_key)
            .collect(),
    );
    let plan = runtime.plan_poll_cycle(Some(selected), 60, &coverage);
    let mut results = Vec::new();
    for planned in plan.requests {
        if !matches!(planned.capability, CapabilityId::DtcService(_)) {
            continue;
        }
        let mut evidence = NullEvidenceSink;
        let result = match runtime
            .execute_request(
                session,
                context,
                selected,
                planned.capability,
                planned.request,
                &mut evidence,
            )
            .await
        {
            Ok(ProfileResponse::Dtcs(dtcs)) => Ok(ProfileDtcExecution {
                result: StepResult::Data(Vec::new()),
                dtcs,
            }),
            Ok(ProfileResponse::Signal(_)) => Ok(ProfileDtcExecution {
                result: StepResult::StepError {
                    kind: StepErrorKind::Other,
                    detail: "profile DTC capability returned a signal".into(),
                },
                dtcs: Vec::new(),
            }),
            Err(DispatchError::Transport(error)) => {
                map_obd2_result(Err(error)).map(|result| ProfileDtcExecution {
                    result,
                    dtcs: Vec::new(),
                })
            }
            Err(error) => Ok(ProfileDtcExecution {
                result: StepResult::StepError {
                    kind: StepErrorKind::Other,
                    detail: format!("profile DTC dispatch failed: {error:?}"),
                },
                dtcs: Vec::new(),
            }),
        };
        results.push(result);
    }
    results
}

fn profile_module_key(module: &ModuleId) -> Option<crate::profiles::ModuleKey> {
    match module.0.as_str() {
        "ecm" => Some(crate::profiles::ModuleKey::Ecm),
        "tcm" => Some(crate::profiles::ModuleKey::Tcm),
        "ficm" => Some(crate::profiles::ModuleKey::Ficm),
        "bcm" => Some(crate::profiles::ModuleKey::Bcm),
        "abs" => Some(crate::profiles::ModuleKey::Ebcm),
        "ipc" => Some(crate::profiles::ModuleKey::Ipc),
        "airbag" => Some(crate::profiles::ModuleKey::Sdm),
        "hvac" => Some(crate::profiles::ModuleKey::Hvac),
        _ => None,
    }
}

/// Execute requests at request boundaries. Cancellation is checked before
/// starting each request and after the previous request has fully completed;
/// an in-flight transport future is never dropped by this loop. Step errors
/// continue the bundle; only transport errors abort it.
pub async fn execute_requests<T, F>(
    transport: &mut T,
    requests: &[DiagnosticRequest],
    mut cancelled: F,
) -> Result<DiagnosticExecution, DiagnosticAborted>
where
    T: DiagnosticTransport + Send,
    F: FnMut() -> bool,
{
    let mut completed = 0;
    let mut step_errors = 0;
    for request in requests {
        if cancelled() {
            return Ok(DiagnosticExecution {
                completed,
                step_errors,
                cancelled: true,
            });
        }
        match transport.request(*request).await {
            Ok(StepResult::Data(_)) => completed += 1,
            Ok(StepResult::StepError { .. }) => {
                completed += 1;
                step_errors += 1;
            }
            Err(error) => {
                return Err(DiagnosticAborted {
                    completed,
                    step_errors,
                    error,
                });
            }
        }
    }
    Ok(DiagnosticExecution {
        completed,
        step_errors,
        cancelled: false,
    })
}

/// Service eligibility inputs for one diagnostic pass. Spec §11: readiness
/// is skipped only when its cached service row is already `unsupported`
/// (`unverified` is attempted — the operator explicitly asked); Mode-05 has
/// its own fail-closed gate.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServiceGates {
    pub cached_mode05_unsupported: bool,
    pub cached_readiness_unsupported: bool,
    pub is_lly_profile: bool,
}

pub fn request_plan(
    fuel: FuelClass,
    protocol: Protocol,
    gates: ServiceGates,
) -> Vec<DiagnosticRequest> {
    // Spec §11 phase 1: every DTC service runs broadcast first, then per
    // discovered module — the full 3×2 matrix, matching the TUI's
    // scan_standard_dtcs. Collapsing either axis loses codes: some modules
    // only answer addressed stored-DTC reads, and pending/permanent have
    // broadcast responders too.
    let mut requests = Vec::new();
    for target in [RequestTarget::Broadcast, RequestTarget::DiscoveredModules] {
        for service in [0x03u8, 0x07, 0x0A] {
            requests.push(DiagnosticRequest {
                phase: DiagnosticPhase::Dtc,
                service,
                target,
                expansion: RequestExpansion::Static,
            });
        }
    }
    requests.push(DiagnosticRequest {
        phase: DiagnosticPhase::FreezeFrames,
        service: 0x02,
        target: RequestTarget::Broadcast,
        expansion: RequestExpansion::PerDtc,
    });
    if !gates.cached_readiness_unsupported {
        requests.push(DiagnosticRequest {
            phase: DiagnosticPhase::Readiness,
            service: 0x01,
            target: RequestTarget::Broadcast,
            expansion: RequestExpansion::Static,
        });
    }
    if mode05_allowed(
        fuel,
        protocol,
        gates.cached_mode05_unsupported,
        gates.is_lly_profile,
    ) {
        requests.push(DiagnosticRequest {
            phase: DiagnosticPhase::Mode05O2,
            service: 0x05,
            target: RequestTarget::Broadcast,
            expansion: RequestExpansion::Static,
        });
    }
    requests.push(DiagnosticRequest {
        phase: DiagnosticPhase::ModuleRefresh,
        service: 0x01,
        target: RequestTarget::DiscoveredModules,
        expansion: RequestExpansion::Static,
    });
    requests
}

/// Expand the DTC summary into the wire order used by the legacy scanner:
/// broadcast stored/pending/permanent first, then the same trio module-major.
/// This REPLACES the six DTC summary rows of [`request_plan`] — executing
/// both would double-scan broadcast.
///
/// Module order is made deterministic here (sorted by module id, matching
/// the TUI's `dtc_scan_modules`) regardless of input order; each emitted
/// [`RequestTarget::Module`] index refers to the caller's ORIGINAL slice.
pub fn expand_dtc_requests(modules: &[String]) -> Vec<DiagnosticRequest> {
    let services = [0x03, 0x07, 0x0A];
    let mut requests = services
        .into_iter()
        .map(|service| DiagnosticRequest {
            phase: DiagnosticPhase::Dtc,
            service,
            target: RequestTarget::Broadcast,
            expansion: RequestExpansion::Static,
        })
        .collect::<Vec<_>>();
    let mut order: Vec<usize> = (0..modules.len()).collect();
    order.sort_by(|&left, &right| modules[left].cmp(&modules[right]));
    for module_index in order {
        requests.extend(services.into_iter().map(|service| DiagnosticRequest {
            phase: DiagnosticPhase::Dtc,
            service,
            target: RequestTarget::Module(module_index),
            expansion: RequestExpansion::Static,
        }));
    }
    requests
}

impl DiagnosticPhase {
    pub const ORDER: [Self; 5] = [
        Self::Dtc,
        Self::FreezeFrames,
        Self::Readiness,
        Self::Mode05O2,
        Self::ModuleRefresh,
    ];
}

/// Mode-05 is intentionally fail-closed. No profile/display heuristic may
/// make this request eligible; only explicit gasoline permits it, and only
/// on a positively identified legacy (non-CAN) protocol — `Auto` means the
/// protocol is unresolved, and `Protocol` is non-exhaustive, so both deny by
/// default rather than slipping past a CAN deny-list.
pub fn mode05_allowed(
    fuel: FuelClass,
    protocol: Protocol,
    cached_unsupported: bool,
    is_lly_profile: bool,
) -> bool {
    matches!(fuel, FuelClass::Gasoline)
        && matches!(
            protocol,
            Protocol::J1850Vpw | Protocol::J1850Pwm | Protocol::Iso9141(_) | Protocol::Kwp2000(_)
        )
        && !cached_unsupported
        && !is_lly_profile
}

pub fn phases() -> &'static [DiagnosticPhase; 5] {
    &DiagnosticPhase::ORDER
}

/// Gate diagnostic service IDs at the request boundary. Telemetry and every
/// non-diagnostic foreground state are denied; Mode-06 is never permitted.
pub fn service_allowed(mode: &ModeState, service: u8) -> bool {
    if !matches!(mode, ModeState::Diagnostic { .. }) {
        return false;
    }
    matches!(service, 0x03 | 0x07 | 0x0A)
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_core::vehicle::KLineInit;

    #[test]
    fn diagnostic_phases_have_stable_five_phase_order() {
        assert_eq!(phases().len(), 5);
        assert_eq!(phases()[0], DiagnosticPhase::Dtc);
        assert_eq!(phases()[4], DiagnosticPhase::ModuleRefresh);
    }

    #[test]
    fn mode05_requires_explicit_gasoline_and_non_can() {
        assert!(mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            false,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Unknown,
            Protocol::Iso9141(KLineInit::SlowInit),
            false,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Can11Bit500,
            false,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            true,
            false
        ));
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            false,
            true
        ));
    }

    #[test]
    fn mode05_denies_unresolved_and_unknown_protocols() {
        // Auto = protocol not yet identified: fail closed.
        assert!(!mode05_allowed(
            FuelClass::Gasoline,
            Protocol::Auto,
            false,
            false
        ));
        // The gate is a positive allow-list, so every non-legacy variant —
        // including ones core adds later — denies by default.
        for legacy in [
            Protocol::J1850Vpw,
            Protocol::J1850Pwm,
            Protocol::Iso9141(KLineInit::FastInit),
            Protocol::Kwp2000(KLineInit::SlowInit),
        ] {
            assert!(mode05_allowed(FuelClass::Gasoline, legacy, false, false));
        }
    }

    #[test]
    fn fuel_resolution_is_exact_and_precedence_ordered() {
        // The recognized session claim always wins.
        assert_eq!(
            resolve_fuel(Some("Diesel"), Some("Gasoline")),
            FuelClass::Diesel
        );
        // An unparseable session CLAIM resolves Unknown — the cached NHTSA
        // row never overrules the curated spec (no value shopping).
        assert_eq!(
            resolve_fuel(Some("mild gasoline blend"), Some("Gasoline")),
            FuelClass::Unknown
        );
        assert_eq!(
            resolve_fuel(Some("bio-diesel b20"), Some("Gasoline")),
            FuelClass::Unknown
        );
        // The embedded generic spec ships fuel_type: unknown — an explicit
        // "no claim" that falls through to the database, like a blank field.
        assert_eq!(
            resolve_fuel(Some("unknown"), Some("Gasoline")),
            FuelClass::Gasoline
        );
        assert_eq!(resolve_fuel(Some("  "), Some("Diesel")), FuelClass::Diesel);
        // Shipped embedded specs use lowercase labels.
        assert_eq!(resolve_fuel(Some("diesel"), None), FuelClass::Diesel);
        // Absent session falls through; unrecognized database stays Unknown.
        assert_eq!(resolve_fuel(None, Some("Other")), FuelClass::Other);
        assert_eq!(resolve_fuel(None, Some("unlisted")), FuelClass::Unknown);
        assert_eq!(resolve_fuel(None, None), FuelClass::Unknown);
    }

    #[test]
    fn diagnostic_services_are_impossible_before_command_and_mode06_is_locked() {
        assert!(!service_allowed(&ModeState::Telemetry, 0x03));
        assert!(!service_allowed(&ModeState::Connecting, 0x07));
        assert!(!service_allowed(
            &ModeState::Diagnostic {
                phase: 0,
                phase_total: 5,
                step: 0,
                total: 0,
            },
            0x06
        ));
        assert!(service_allowed(
            &ModeState::Diagnostic {
                phase: 0,
                phase_total: 5,
                step: 0,
                total: 0,
            },
            0x03
        ));
    }

    #[test]
    fn request_plan_preserves_phase_order_and_excludes_mode06() {
        let plan = request_plan(
            FuelClass::Gasoline,
            Protocol::Iso9141(KLineInit::SlowInit),
            ServiceGates::default(),
        );
        // Full 3x2 DTC matrix: broadcast S/P/P, then per-module S/P/P.
        let dtc: Vec<(u8, RequestTarget)> = plan
            .iter()
            .filter(|request| request.phase == DiagnosticPhase::Dtc)
            .map(|request| (request.service, request.target))
            .collect();
        assert_eq!(
            dtc,
            vec![
                (0x03, RequestTarget::Broadcast),
                (0x07, RequestTarget::Broadcast),
                (0x0A, RequestTarget::Broadcast),
                (0x03, RequestTarget::DiscoveredModules),
                (0x07, RequestTarget::DiscoveredModules),
                (0x0A, RequestTarget::DiscoveredModules),
            ],
        );
        assert_eq!(plan[6].service, 0x02);
        assert_eq!(plan[6].expansion, RequestExpansion::PerDtc);
        assert!(plan.iter().any(|request| request.service == 0x05));
        assert!(plan.iter().all(|request| request.service != 0x06));
        let diesel = request_plan(
            FuelClass::Diesel,
            Protocol::Iso9141(KLineInit::SlowInit),
            ServiceGates::default(),
        );
        assert!(diesel.iter().all(|request| request.service != 0x05));
    }

    #[test]
    fn readiness_skips_only_when_cached_unsupported() {
        let skipped = request_plan(
            FuelClass::Diesel,
            Protocol::J1850Vpw,
            ServiceGates {
                cached_readiness_unsupported: true,
                ..ServiceGates::default()
            },
        );
        assert!(skipped
            .iter()
            .all(|request| request.phase != DiagnosticPhase::Readiness));
        // Unverified (i.e. not cached-unsupported) is attempted: the
        // operator explicitly requested diagnostics.
        let attempted = request_plan(
            FuelClass::Diesel,
            Protocol::J1850Vpw,
            ServiceGates::default(),
        );
        assert!(attempted
            .iter()
            .any(|request| request.phase == DiagnosticPhase::Readiness));
    }

    #[test]
    fn dtc_expansion_is_broadcast_then_module_major() {
        // Deliberately unsorted input: order is normalized by module id, and
        // every module-major triple carries a concrete module binding.
        let requests = expand_dtc_requests(&["tcm".into(), "ecm".into()]);
        let sequence: Vec<(u8, RequestTarget)> = requests
            .iter()
            .map(|request| (request.service, request.target))
            .collect();
        assert_eq!(
            sequence,
            vec![
                (0x03, RequestTarget::Broadcast),
                (0x07, RequestTarget::Broadcast),
                (0x0A, RequestTarget::Broadcast),
                (0x03, RequestTarget::Module(1)), // "ecm" sorts first
                (0x07, RequestTarget::Module(1)),
                (0x0A, RequestTarget::Module(1)),
                (0x03, RequestTarget::Module(0)), // then "tcm"
                (0x07, RequestTarget::Module(0)),
                (0x0A, RequestTarget::Module(0)),
            ]
        );
    }

    #[test]
    fn dtc_expansion_without_modules_is_broadcast_only() {
        let requests = expand_dtc_requests(&[]);
        assert_eq!(requests.len(), 3);
        assert!(requests
            .iter()
            .all(|request| request.target == RequestTarget::Broadcast));
    }

    #[test]
    fn standard_dtc_payload_retains_status_module_and_raw_pairs() {
        let request = DiagnosticRequest {
            phase: DiagnosticPhase::Dtc,
            service: 0x07,
            target: RequestTarget::Module(0),
            expansion: RequestExpansion::Static,
        };
        let modules = [ModuleId("tcm".into())];
        let dtcs = decode_standard_dtc_payload(request, &[0x25, 0x63, 0, 0], &modules, None);

        assert_eq!(dtcs.len(), 1);
        assert_eq!(dtcs[0].key.code, "P2563");
        assert_eq!(dtcs[0].key.status, DiagnosticDtcStatus::Pending);
        assert_eq!(dtcs[0].key.module.as_deref(), Some("tcm"));
        assert_eq!(dtcs[0].raw, vec![0x25, 0x63]);
        assert!(matches!(dtcs[0].origin, DiagnosticDtcOrigin::Standard));
    }

    #[test]
    fn every_retained_dtc_derives_bounded_freeze_frame_work_without_spec_links() {
        let mut result = DiagnosticResult::default();
        result.standard_dtcs.push(DiagnosticDtc {
            key: DiagnosticDtcKey {
                code: "P0100".into(),
                status: DiagnosticDtcStatus::Stored,
                module: Some("ecm".into()),
            },
            origin: DiagnosticDtcOrigin::Standard,
            description: None,
            notes: None,
            severity: None,
            status_raw: None,
            status_flags: Vec::new(),
            raw: vec![0x01, 0x00],
        });
        // A duplicate DTC from a broadcast and addressed scan must be
        // retained as evidence but must not make duplicate Mode-02 traffic.
        result.standard_dtcs.push(result.standard_dtcs[0].clone());

        let work = result.freeze_frame_work(None);
        assert_eq!(work.len(), 1);
        assert_eq!(work[0].dtc.code, "P0100");
        assert_eq!(work[0].pids, DEFAULT_FREEZE_FRAME_PIDS);
    }

    struct RecordingTransport {
        seen: Vec<DiagnosticRequest>,
        /// (request index → scripted result) for non-default responses.
        script: Vec<(usize, anyhow::Result<StepResult>)>,
    }

    impl RecordingTransport {
        fn recording() -> Self {
            Self {
                seen: Vec::new(),
                script: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl DiagnosticTransport for RecordingTransport {
        async fn request(&mut self, request: DiagnosticRequest) -> anyhow::Result<StepResult> {
            let index = self.seen.len();
            self.seen.push(request);
            if let Some(position) = self.script.iter().position(|(at, _)| *at == index) {
                return self.script.remove(position).1;
            }
            Ok(StepResult::Data(Vec::new()))
        }
    }

    #[tokio::test]
    async fn executor_observes_cancel_only_between_requests() {
        let requests = expand_dtc_requests(&["ecm".into()]);
        let mut transport = RecordingTransport::recording();
        let mut checks = 0;
        let result = execute_requests(&mut transport, &requests, || {
            checks += 1;
            checks > 2
        })
        .await
        .unwrap();
        assert!(result.cancelled);
        assert_eq!(result.completed, 2);
        assert_eq!(transport.seen.len(), 2);
    }

    #[tokio::test]
    async fn step_errors_continue_the_bundle() {
        let requests = expand_dtc_requests(&["ecm".into()]);
        let mut transport = RecordingTransport::recording();
        transport.script.push((
            1,
            Ok(StepResult::StepError {
                kind: StepErrorKind::NoData,
                detail: "NO DATA".into(),
            }),
        ));
        let result = execute_requests(&mut transport, &requests, || false)
            .await
            .unwrap();
        assert!(!result.cancelled);
        assert_eq!(result.completed, requests.len());
        assert_eq!(result.step_errors, 1);
        assert_eq!(transport.seen.len(), requests.len());
    }

    #[tokio::test]
    async fn transport_loss_aborts_with_partial_progress() {
        let requests = expand_dtc_requests(&["ecm".into()]);
        let mut transport = RecordingTransport::recording();
        transport.script.push((
            0,
            Ok(StepResult::StepError {
                kind: StepErrorKind::NoData,
                detail: "NO DATA".into(),
            }),
        ));
        transport
            .script
            .push((2, Err(anyhow::anyhow!("serial gone"))));
        let aborted = execute_requests(&mut transport, &requests, || false)
            .await
            .unwrap_err();
        assert_eq!(aborted.completed, 2);
        assert_eq!(aborted.step_errors, 1);
        // Nothing past the transport loss was attempted.
        assert_eq!(transport.seen.len(), 3);
    }

    #[tokio::test]
    async fn mode05_execution_uses_the_session_owned_o2_monitoring_matrix() {
        use obd2_core::adapter::mock::MockAdapter;

        let mut session = Session::new(MockAdapter::new());
        let mode = ModeState::Diagnostic {
            phase: 3,
            phase_total: 5,
            step: 0,
            total: 1,
        };
        let request = DiagnosticRequest {
            phase: DiagnosticPhase::Mode05O2,
            service: 0x05,
            target: RequestTarget::Broadcast,
            expansion: RequestExpansion::Static,
        };

        // MockAdapter answers only well-formed [TID, sensor] Mode-05
        // requests.  A bare raw 05 would produce NoData here; Session's O2
        // method performs the required request matrix and decodes it.
        assert!(matches!(
            execute_session_request(&mut session, &mode, request, &[]).await,
            Ok(StepResult::Data(_))
        ));
    }

    #[test]
    fn core_mapping_keeps_protocol_failures_nonfatal() {
        use obd2_core::error::NegativeResponse;
        for nonfatal in [
            Obd2Error::NoData,
            Obd2Error::Timeout,
            Obd2Error::ParseError("bad payload".into()),
            Obd2Error::NegativeResponse {
                service: 0x03,
                nrc: NegativeResponse::ServiceNotSupported,
            },
        ] {
            assert!(
                matches!(
                    map_obd2_result(Err(nonfatal)),
                    Ok(StepResult::StepError { .. })
                ),
                "protocol failures must record and continue"
            );
        }
    }

    #[test]
    fn core_mapping_treats_link_loss_as_bundle_fatal() {
        // A yanked USB adapter surfaces as Io; both link classes must abort
        // so the runner reconnects instead of grinding through timeouts.
        assert!(map_obd2_result(Err(Obd2Error::Transport("gone".into()))).is_err());
        assert!(map_obd2_result(Err(Obd2Error::Io(std::io::Error::other("unplugged")))).is_err());
    }

    /// Classification must survive the REAL core error types end to end —
    /// the prior string matcher passed its tests against invented strings
    /// while every real `Obd2Error` fell through to Unverified.
    #[test]
    fn diagnostic_outcomes_prune_only_confirmed_unsupported_results() {
        use obd2_core::error::NegativeResponse;
        let through = |error: Obd2Error| map_obd2_result(Err(error)).unwrap();

        let no_data = through(Obd2Error::NoData);
        assert_eq!(
            capability_outcome(&no_data, false),
            CapabilityOutcome::Unverified
        );
        assert_eq!(
            capability_outcome(&no_data, true),
            CapabilityOutcome::Unsupported
        );
        assert_eq!(
            capability_outcome(&through(Obd2Error::UnsupportedPid { pid: 0x23 }), false),
            CapabilityOutcome::Unsupported
        );
        assert_eq!(
            capability_outcome(
                &through(Obd2Error::NegativeResponse {
                    service: 0x03,
                    nrc: NegativeResponse::ServiceNotSupported,
                }),
                false
            ),
            CapabilityOutcome::Unsupported
        );
        assert_eq!(
            capability_outcome(&through(Obd2Error::Timeout), false),
            CapabilityOutcome::Unverified
        );
        assert_eq!(
            capability_outcome(&StepResult::Data(Vec::new()), false),
            CapabilityOutcome::Supported
        );
    }
}
