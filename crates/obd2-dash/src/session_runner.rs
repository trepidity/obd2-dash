use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;

use obd2_core::adapter::Adapter;
use obd2_core::error::{NegativeResponse, Obd2Error};
use obd2_core::protocol::dtc::{Dtc, DtcStatus};
use obd2_core::protocol::enhanced::{Confidence as CoreConfidence, EnhancedPid};
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::Target;
use obd2_core::session::poller::{execute_poll_cycle, PollConfig, PollEvent};
use obd2_core::session::Session;
use obd2_core::vehicle::{ModuleId, Protocol};

use crate::app::{CaptureCommand, CaptureHandle, DiagnosticCommand, Message};
use crate::domain::{ConnectionState, DiscoveryState, O2Reading};
use crate::domain::{
    DiagnosticScanEntry, DiagnosticScanResult, DiagnosticScanScope, DtcService, FreezeFrameSnapshot,
};
use obd2_dash::gm_active::{
    active_test_evidence_record, blocked_active_test_result, write_active_test_evidence,
    GmActiveTestCommand,
};
use obd2_dash::gm_enhanced::{is_lly_spec_identity, lly_profile_matches};
use obd2_dash::gm_evidence::GmEvidenceRecord;
use obd2_dash::profiles::{
    acquire_identity, build_vehicle_context, confidence_warning, next_generation,
    select_into_state, validate_vin_charset, CapabilityId, Confidence as ProfileConfidence,
    CoverageMap, DecodedDtc, DispatchError, DispatchEvidence, IdentityConfidence, IdentityOutcome,
    PlannedRequest, ProfileDecodeError, ProfileEvidenceRecord, ProfileEvidenceSink,
    ProfileRegistry, ProfileResponse, ProfileRuntime, ProfileState,
    Provenance as ProfileProvenance, RequestId, SelectedProfile, SignalDefinition, VehicleContext,
};

#[cfg(feature = "profile-selection")]
const EXTRA_VIN_READS: u8 = 2;
#[cfg(not(feature = "profile-selection"))]
const EXTRA_VIN_READS: u8 = 0;

#[derive(Debug, Clone)]
pub struct SessionRunnerConfig {
    pub poll_ms: u64,
    pub standard_pids: Vec<Pid>,
}

#[derive(Debug)]
pub struct PreparedSession {
    poll_ms: u64,
    poll_config: PollConfig,
    enhanced_targets: Vec<EnhancedPollTarget>,
    gm_class2_backoff: GmClass2Backoff,
    last_connection: Option<ConnectionState>,
    last_discovery: Option<DiscoveryState>,
    profile_state: ProfileState,
    identity: IdentityOutcome,
    profile_context: ProfileContextFingerprint,
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct ProfileContextFingerprint {
    protocol: Protocol,
    vin: Option<String>,
    vin_confidence: IdentityConfidence,
    spec_identity: Option<String>,
    active_bus: Option<String>,
    discovered_modules: Vec<String>,
}

#[derive(Debug, Clone)]
struct EnhancedPollTarget {
    did: u16,
    module_id: ModuleId,
    module_label: String,
    name: String,
    unit: String,
    profile_request: Option<ProfileEnhancedPollTarget>,
    confidence: Option<String>,
    evidence: Option<String>,
}

#[derive(Debug, Clone)]
struct ProfileEnhancedPollTarget {
    context: VehicleContext,
    selected: SelectedProfile,
    capability: CapabilityId,
    request: RequestId,
}

struct ChannelProfileEvidenceSink<'a> {
    tx: &'a mpsc::UnboundedSender<Message>,
    identity_confidence: IdentityConfidence,
    manual_confirmation: bool,
}

impl<'a> ChannelProfileEvidenceSink<'a> {
    fn new(
        tx: &'a mpsc::UnboundedSender<Message>,
        identity_confidence: IdentityConfidence,
        manual_confirmation: bool,
    ) -> Self {
        Self {
            tx,
            identity_confidence,
            manual_confirmation,
        }
    }
}

impl ProfileEvidenceSink for ChannelProfileEvidenceSink<'_> {
    fn record(&mut self, evidence: &DispatchEvidence<'_>) {
        let record = ProfileEvidenceRecord::from_dispatch(
            evidence,
            Some(identity_confidence_label(self.identity_confidence)),
            self.manual_confirmation,
        );
        let _ = self.tx.send(Message::ProfileEvidence(Box::new(record)));
    }
}

#[derive(Debug, Default)]
struct GmClass2Backoff {
    cached: HashMap<(String, DtcService), GmClass2BackoffEntry>,
}

#[derive(Debug, Clone)]
struct GmClass2BackoffEntry {
    skips_remaining: u8,
    result: DiagnosticScanResult,
}

pub async fn prepare_session<A: Adapter>(
    session: &mut Session<A>,
    config: SessionRunnerConfig,
    tx: &mpsc::UnboundedSender<Message>,
) -> Result<PreparedSession, String> {
    let mut last_connection: Option<ConnectionState> = None;
    let mut last_discovery: Option<DiscoveryState> = None;

    let _ = tx.send(Message::ConnectionStatus(ConnectionState::AdapterPresent));
    let _ = tx.send(Message::ConnectionStatus(
        ConnectionState::ProtocolNegotiating,
    ));

    match session.initialize().await {
        Ok(info) => {
            let _ = tx.send(Message::AdapterDetected(info.clone()));
        }
        Err(e) => {
            let msg = format!("Init failed: {e}");
            let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(
                msg.clone(),
            )));
            let _ = tx.send(Message::Error(msg));
            return Err(format!("Init failed: {e}"));
        }
    }

    emit_session_state(session, tx, &mut last_connection);
    emit_discovery(session, tx, &mut last_discovery);

    let generation = next_generation();
    let identity = acquire_identity(session, EXTRA_VIN_READS).await;
    if let Some(vin) = identity.vin.as_ref() {
        let _ = tx.send(Message::VinDetected(vin.clone()));
    }
    emit_session_state(session, tx, &mut last_connection);
    emit_discovery(session, tx, &mut last_discovery);
    let (profile_state, profile_context) = compute_profile_state(session, generation, &identity);
    publish_profile_state(&profile_state, tx);

    let enhanced_targets = build_enhanced_targets(session, &profile_state);
    publish_enhanced_targets(&enhanced_targets, tx);

    let pids_to_poll = match session.supported_pids().await {
        Ok(supported) if !supported.is_empty() => config
            .standard_pids
            .iter()
            .copied()
            .filter(|pid| supported.contains(pid) || should_force_standard_poll(*pid))
            .collect(),
        Ok(_) | Err(_) => config.standard_pids.clone(),
    };

    let poll_config = PollConfig::new(pids_to_poll)
        .with_interval(Duration::from_millis(config.poll_ms))
        .with_voltage(true);

    Ok(PreparedSession {
        poll_ms: config.poll_ms,
        poll_config,
        enhanced_targets,
        gm_class2_backoff: GmClass2Backoff::default(),
        last_connection,
        last_discovery,
        profile_state,
        identity,
        profile_context,
    })
}

pub async fn run_prepared_session<A: Adapter>(
    session: &mut Session<A>,
    mut prepared: PreparedSession,
    tx: &mpsc::UnboundedSender<Message>,
    mut capture_rx: mpsc::UnboundedReceiver<CaptureCommand>,
    capture_handle: CaptureHandle,
    mut diagnostic_rx: mpsc::UnboundedReceiver<DiagnosticCommand>,
) {
    let (poll_tx, mut poll_rx) = mpsc::channel(256);
    let mut interval = tokio::time::interval(Duration::from_millis(prepared.poll_ms));
    let mut cycle = 0u32;

    loop {
        interval.tick().await;

        while let Ok(cmd) = capture_rx.try_recv() {
            match cmd {
                CaptureCommand::Start { path, metadata } => {
                    match session.start_raw_capture(&path, &metadata) {
                        Ok(()) => {
                            capture_handle.set_active(true);
                            let _ = tx.send(Message::RawCaptureStarted);
                        }
                        Err(e) => {
                            let _ = tx.send(Message::RawCaptureError(e.to_string()));
                        }
                    }
                }
                CaptureCommand::Stop => match session.stop_raw_capture() {
                    Ok(Some(path)) => {
                        capture_handle.set_active(false);
                        let _ = tx.send(Message::RawCaptureStopped(path));
                    }
                    Ok(None) => {
                        capture_handle.set_active(false);
                    }
                    Err(e) => {
                        let _ = tx.send(Message::RawCaptureError(e.to_string()));
                    }
                },
            }
        }

        while let Ok(cmd) = diagnostic_rx.try_recv() {
            handle_diagnostic_command(session, cmd, tx).await;
        }

        execute_poll_cycle(session, &prepared.poll_config, &poll_tx, None).await;
        drain_poll_events(&mut poll_rx, tx).await;
        emit_session_state(session, tx, &mut prepared.last_connection);
        let discovery_changed = emit_discovery(session, tx, &mut prepared.last_discovery);
        let profile_changed = refresh_profile_selection_if_needed(session, &mut prepared, tx);
        if discovery_changed || profile_changed {
            prepared.enhanced_targets = build_enhanced_targets(session, &prepared.profile_state);
            publish_enhanced_targets(&prepared.enhanced_targets, tx);
        }

        cycle += 1;

        if !prepared.enhanced_targets.is_empty() && cycle % 5 == 0 {
            poll_enhanced(session, &prepared.enhanced_targets, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            let discovery_changed = emit_discovery(session, tx, &mut prepared.last_discovery);
            let profile_changed = refresh_profile_selection_if_needed(session, &mut prepared, tx);
            if discovery_changed || profile_changed {
                prepared.enhanced_targets =
                    build_enhanced_targets(session, &prepared.profile_state);
                publish_enhanced_targets(&prepared.enhanced_targets, tx);
            }
        }

        if cycle % 10 == 0 {
            poll_dtcs(
                session,
                tx,
                cycle.into(),
                &prepared.profile_state,
                &prepared.identity,
                &mut prepared.gm_class2_backoff,
            )
            .await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            let discovery_changed = emit_discovery(session, tx, &mut prepared.last_discovery);
            let profile_changed = refresh_profile_selection_if_needed(session, &mut prepared, tx);
            if discovery_changed || profile_changed {
                prepared.enhanced_targets =
                    build_enhanced_targets(session, &prepared.profile_state);
                publish_enhanced_targets(&prepared.enhanced_targets, tx);
            }
        }

        if cycle % 20 == 0 {
            poll_o2_monitoring(session, tx).await;
            poll_readiness(session, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            let discovery_changed = emit_discovery(session, tx, &mut prepared.last_discovery);
            let profile_changed = refresh_profile_selection_if_needed(session, &mut prepared, tx);
            if discovery_changed || profile_changed {
                prepared.enhanced_targets =
                    build_enhanced_targets(session, &prepared.profile_state);
                publish_enhanced_targets(&prepared.enhanced_targets, tx);
            }
        }
    }
}

pub async fn run_session_task<A: Adapter>(
    session: &mut Session<A>,
    config: SessionRunnerConfig,
    tx: &mpsc::UnboundedSender<Message>,
    capture_rx: mpsc::UnboundedReceiver<CaptureCommand>,
    capture_handle: CaptureHandle,
    diagnostic_rx: mpsc::UnboundedReceiver<DiagnosticCommand>,
) -> Result<(), String> {
    let prepared = prepare_session(session, config, tx).await?;
    run_prepared_session(
        session,
        prepared,
        tx,
        capture_rx,
        capture_handle,
        diagnostic_rx,
    )
    .await;
    Ok(())
}

async fn drain_poll_events(
    poll_rx: &mut mpsc::Receiver<PollEvent>,
    tx: &mpsc::UnboundedSender<Message>,
) {
    while let Ok(event) = poll_rx.try_recv() {
        match event {
            PollEvent::Reading { pid, reading } => {
                if tx.send(Message::PidUpdate(pid, reading)).is_err() {
                    return;
                }
            }
            PollEvent::Voltage(v) => {
                let _ = tx.send(Message::VoltageUpdate(v));
            }
            PollEvent::Error { pid, error } => {
                if let Some(pid) = pid {
                    if is_stale_pid_response_error(pid, &error) {
                        tracing::debug!("Suppressing stale PID response for {}: {}", pid, error);
                        continue;
                    }

                    let message = format!("{pid}: {error}");
                    let _ = tx.send(Message::Error(message));
                    continue;
                }

                let message = error;
                let _ = tx.send(Message::Error(message));
            }
            PollEvent::EnhancedReading { .. } => {
                tracing::debug!(
                    "Ignoring core poller EnhancedReading; dash uses explicit enhanced cadence"
                );
            }
            PollEvent::Alert(result) => {
                tracing::debug!(
                    "Ignoring core poller alert to avoid double-thresholding: {:?}",
                    result
                );
            }
            PollEvent::RuleFired {
                rule_name,
                description,
            } => {
                tracing::info!("Diagnostic rule fired: {rule_name}: {description}");
            }
            _ => {
                tracing::debug!("Ignoring unsupported future PollEvent variant");
            }
        }
    }
}

fn is_stale_pid_response_error(pid: Pid, error: &str) -> bool {
    let Some((_, response)) = error.rsplit_once("response:") else {
        return false;
    };

    let mut bytes = [0u8; 2];
    let mut count = 0usize;
    let mut high_nibble: Option<u8> = None;

    for byte in response.bytes() {
        let Some(nibble) = hex_nibble(byte) else {
            continue;
        };

        if let Some(high) = high_nibble.take() {
            bytes[count] = (high << 4) | nibble;
            count += 1;
            if count == bytes.len() {
                break;
            }
        } else {
            high_nibble = Some(nibble);
        }
    }

    count == bytes.len() && bytes[0] == 0x41 && bytes[1] != pid.0
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

async fn poll_enhanced<A: Adapter>(
    session: &mut Session<A>,
    targets: &[EnhancedPollTarget],
    tx: &mpsc::UnboundedSender<Message>,
) {
    for target in targets {
        match read_enhanced_target(session, target, tx).await {
            Ok(reading) => {
                let _ = tx.send(Message::EnhancedPidUpdate {
                    did: target.did,
                    module: target.module_label.clone(),
                    name: target.name.clone(),
                    value: reading,
                    unit: target.unit.clone(),
                    confidence: target.confidence.clone(),
                    evidence: target.evidence.clone(),
                });
            }
            Err(e) => {
                tracing::debug!(
                    "Enhanced read failed for {} {:04X}: {}",
                    target.module_label,
                    target.did,
                    e
                );
                let _ = tx.send(Message::EnhancedPidError {
                    did: target.did,
                    module: target.module_label.clone(),
                    name: target.name.clone(),
                    unit: target.unit.clone(),
                    error: e.to_string(),
                    confidence: target.confidence.clone(),
                    evidence: target.evidence.clone(),
                });
            }
        }
    }
}

async fn read_enhanced_target<A: Adapter>(
    session: &mut Session<A>,
    target: &EnhancedPollTarget,
    tx: &mpsc::UnboundedSender<Message>,
) -> Result<f64, Obd2Error> {
    if let Some(profile_request) = target.profile_request.as_ref() {
        let registry = ProfileRegistry::with_builtins();
        let runtime = ProfileRuntime::new(&registry);
        let mut evidence = ChannelProfileEvidenceSink::new(
            tx,
            profile_request.context.vin_confidence,
            profile_request.selected.manual_confirmed(),
        );
        let response = runtime
            .execute_request(
                session,
                &profile_request.context,
                &profile_request.selected,
                profile_request.capability,
                profile_request.request,
                &mut evidence,
            )
            .await
            .map_err(dispatch_to_obd_error)?;

        return match response {
            ProfileResponse::Signal(signal) => Ok(signal.value),
            ProfileResponse::Dtcs(_) => Err(Obd2Error::ParseError(format!(
                "profile capability {:?} returned DTCs for enhanced target {:04X}",
                profile_request.capability, target.did
            ))),
        };
    }

    session
        .read_enhanced(target.did, target.module_id.clone())
        .await
        .and_then(|reading| reading.value.as_f64())
}

fn dispatch_to_obd_error(error: DispatchError) -> Obd2Error {
    match error {
        DispatchError::Transport(error) => error,
        other => Obd2Error::ParseError(format!("profile dispatch failed: {other:?}")),
    }
}

async fn poll_dtcs<A: Adapter>(
    session: &mut Session<A>,
    tx: &mpsc::UnboundedSender<Message>,
    cycle: u64,
    profile_state: &ProfileState,
    identity: &IdentityOutcome,
    gm_backoff: &mut GmClass2Backoff,
) {
    let mut scan = scan_standard_dtcs(session).await;
    append_profile_dtcs(
        session,
        &mut scan,
        tx,
        cycle,
        profile_state,
        identity,
        gm_backoff,
    )
    .await;
    enrich_dtcs(session, &mut scan.dtcs);
    obd2_core::session::diagnostics::dedup_dtcs(&mut scan.dtcs);
    apply_profile_dtc_notes(&mut scan.dtcs, &scan.profile_notes);

    let _ = tx.send(Message::DtcUpdate(scan.dtcs));
    let _ = tx.send(Message::DiagnosticScanUpdate(scan.entries));
}

struct DtcScan {
    dtcs: Vec<Dtc>,
    entries: Vec<DiagnosticScanEntry>,
    profile_notes: Vec<DtcProfileNote>,
}

#[derive(Debug, Clone)]
struct DtcProfileNote {
    code: String,
    source_module: Option<String>,
    status: DtcStatus,
    note: String,
}

async fn scan_standard_dtcs<A: Adapter>(session: &mut Session<A>) -> DtcScan {
    let mut scan = DtcScan {
        dtcs: Vec::new(),
        entries: Vec::new(),
        profile_notes: Vec::new(),
    };

    for service in [
        DtcService::Stored,
        DtcService::Pending,
        DtcService::Permanent,
    ] {
        append_dtc_probe(
            session,
            &mut scan,
            DiagnosticScanScope::Broadcast,
            service,
            Target::Broadcast,
            None,
        )
        .await;
    }

    for module in dtc_scan_modules(session) {
        let scope = DiagnosticScanScope::Module(module.0.clone());
        for service in [
            DtcService::Stored,
            DtcService::Pending,
            DtcService::Permanent,
        ] {
            append_dtc_probe(
                session,
                &mut scan,
                scope.clone(),
                service,
                Target::Module(module.0.clone()),
                Some(&module),
            )
            .await;
        }
    }

    scan
}

async fn append_profile_dtcs<A: Adapter>(
    session: &mut Session<A>,
    scan: &mut DtcScan,
    tx: &mpsc::UnboundedSender<Message>,
    cycle: u64,
    profile_state: &ProfileState,
    identity: &IdentityOutcome,
    backoff: &mut GmClass2Backoff,
) {
    let Some(selected) = profile_state.selected.as_ref() else {
        return;
    };

    let mut context = build_vehicle_context(session, profile_state.generation, identity);
    context.discovered_modules = dtc_scan_modules(session);
    let coverage = CoverageMap::new(Vec::new()).with_discovered_modules(
        context
            .discovered_modules
            .iter()
            .filter_map(profile_module_key)
            .collect(),
    );
    let registry = ProfileRegistry::with_builtins();
    let runtime = ProfileRuntime::new(&registry);
    let plan = runtime.plan_poll_cycle(Some(selected), cycle, &coverage);

    for planned in plan.requests {
        let CapabilityId::DtcService(key) = planned.capability else {
            continue;
        };
        let Some(service) = profile_dtc_service_for_key(key) else {
            tracing::debug!("Skipping profile DTC service without scan-panel mapping: {key}");
            continue;
        };

        let module = planned.route.module.to_core_module_id();
        let scope = DiagnosticScanScope::Module(module.0.clone());
        if let Some(result) = backoff.cached_result(&module, service) {
            scan.entries.push(DiagnosticScanEntry {
                scope,
                service,
                result,
            });
            continue;
        }

        let result = execute_profile_dtc_request(
            session, scan, tx, &runtime, &context, selected, planned, service, &module,
        )
        .await;

        backoff.observe(&module, service, &result);
        scan.entries.push(DiagnosticScanEntry {
            scope,
            service,
            result,
        });
    }
}

async fn execute_profile_dtc_request<A: Adapter>(
    session: &mut Session<A>,
    scan: &mut DtcScan,
    tx: &mpsc::UnboundedSender<Message>,
    runtime: &ProfileRuntime<'_>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    planned: PlannedRequest,
    service: DtcService,
    module: &ModuleId,
) -> DiagnosticScanResult {
    let mut evidence =
        ChannelProfileEvidenceSink::new(tx, context.vin_confidence, selected.manual_confirmed());
    match runtime
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
        Ok(ProfileResponse::Dtcs(decoded)) => {
            let count = decoded.len();
            for decoded in decoded {
                let (dtc, note) = profile_decoded_dtc_to_core(decoded, module);
                if let Some(note) = note {
                    scan.profile_notes.push(note);
                }
                scan.dtcs.push(dtc);
            }
            if count == 0 {
                DiagnosticScanResult::Empty
            } else {
                DiagnosticScanResult::Codes(count)
            }
        }
        Ok(ProfileResponse::Signal(_)) => DiagnosticScanResult::Error(format!(
            "profile capability {:?} returned a signal for {}",
            planned.capability,
            service.label()
        )),
        Err(err) => profile_dispatch_scan_result(err),
    }
}

fn profile_dtc_service_for_key(key: &str) -> Option<DtcService> {
    match key {
        "lly.class2.dtc.all" => Some(DtcService::GmClass2All),
        "lly.class2.dtc.active" => Some(DtcService::GmClass2Active),
        _ => None,
    }
}

fn profile_decoded_dtc_to_core(
    decoded: DecodedDtc,
    fallback_module: &ModuleId,
) -> (Dtc, Option<DtcProfileNote>) {
    let mut dtc = Dtc::from_code(&decoded.code);
    dtc.status = decoded.status;
    let source_module = decoded
        .module
        .map(|module| module.0)
        .unwrap_or_else(|| fallback_module.0.clone());
    dtc.source_module = Some(source_module.clone());
    if let Some(note) = decoded.notes.clone() {
        dtc.notes = Some(note.clone());
        let annotation = DtcProfileNote {
            code: dtc.code.clone(),
            source_module: Some(source_module),
            status: dtc.status,
            note,
        };
        (dtc, Some(annotation))
    } else {
        (dtc, None)
    }
}

fn profile_dispatch_scan_result(error: DispatchError) -> DiagnosticScanResult {
    match error {
        DispatchError::Transport(Obd2Error::NoData) => DiagnosticScanResult::NoData,
        DispatchError::Transport(Obd2Error::NegativeResponse { nrc, .. })
            if is_unsupported_nrc(nrc) =>
        {
            DiagnosticScanResult::Unsupported(nrc.to_string())
        }
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { nrc, .. })
            if is_unsupported_nrc_byte(nrc) =>
        {
            DiagnosticScanResult::Unsupported(negative_response_label(nrc))
        }
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { service, nrc }) => {
            DiagnosticScanResult::Error(format!(
                "negative response: service 0x{service:02X}, {}",
                negative_response_label(nrc)
            ))
        }
        DispatchError::Decode(ProfileDecodeError::Decode(message))
        | DispatchError::Decode(ProfileDecodeError::Other(message)) => {
            DiagnosticScanResult::Error(message)
        }
        other => DiagnosticScanResult::Error(format!("{other:?}")),
    }
}

fn is_unsupported_nrc(nrc: NegativeResponse) -> bool {
    matches!(
        nrc,
        NegativeResponse::ServiceNotSupported | NegativeResponse::SubFunctionNotSupported
    )
}

fn is_unsupported_nrc_byte(nrc: u8) -> bool {
    matches!(
        NegativeResponse::from_byte(nrc),
        Some(NegativeResponse::ServiceNotSupported | NegativeResponse::SubFunctionNotSupported)
    )
}

fn negative_response_label(nrc: u8) -> String {
    NegativeResponse::from_byte(nrc)
        .map(|nrc| nrc.to_string())
        .unwrap_or_else(|| format!("NRC 0x{nrc:02X}"))
}

fn apply_profile_dtc_notes(dtcs: &mut [Dtc], notes: &[DtcProfileNote]) {
    for dtc in dtcs {
        let Some(note) = notes.iter().find(|note| {
            note.code == dtc.code
                && note.source_module == dtc.source_module
                && note.status == dtc.status
        }) else {
            continue;
        };

        match dtc.notes.as_mut() {
            Some(existing) if existing.contains(&note.note) => {}
            Some(existing) => {
                existing.push_str("; ");
                existing.push_str(&note.note);
            }
            None => dtc.notes = Some(note.note.clone()),
        }
    }
}

async fn append_dtc_probe<A: Adapter>(
    session: &mut Session<A>,
    scan: &mut DtcScan,
    scope: DiagnosticScanScope,
    service: DtcService,
    target: Target,
    source_module: Option<&ModuleId>,
) {
    let status = dtc_status_for_service(service);
    let result = match session.raw_request(service.service_id(), &[], target).await {
        Ok(data) => {
            let mut dtcs = decode_dtc_bytes(&data, status);
            if let Some(module) = source_module {
                for dtc in &mut dtcs {
                    dtc.source_module = Some(module.0.clone());
                }
            }
            let count = dtcs.len();
            scan.dtcs.append(&mut dtcs);
            if count == 0 {
                DiagnosticScanResult::Empty
            } else {
                DiagnosticScanResult::Codes(count)
            }
        }
        Err(Obd2Error::NoData) => DiagnosticScanResult::NoData,
        Err(Obd2Error::NegativeResponse { nrc, .. })
            if nrc == NegativeResponse::ServiceNotSupported =>
        {
            DiagnosticScanResult::Unsupported(nrc.to_string())
        }
        Err(Obd2Error::NegativeResponse { nrc, .. })
            if nrc == NegativeResponse::SubFunctionNotSupported =>
        {
            DiagnosticScanResult::Unsupported(nrc.to_string())
        }
        Err(err) => DiagnosticScanResult::Error(err.to_string()),
    };

    scan.entries.push(DiagnosticScanEntry {
        scope,
        service,
        result,
    });
}

fn dtc_scan_modules<A: Adapter>(session: &Session<A>) -> Vec<ModuleId> {
    let Some(discovery) = session.discovery() else {
        return Vec::new();
    };

    let mut modules: Vec<ModuleId> = discovery
        .modules
        .iter()
        .filter_map(|(id, resolved)| {
            if let Some(active_bus) = discovery.active_bus.as_ref() {
                if resolved.bus != active_bus.id {
                    return None;
                }
            }
            Some(id.clone())
        })
        .collect();
    modules.sort_by(|a, b| a.0.cmp(&b.0));
    modules
}

impl GmClass2Backoff {
    fn cached_result(
        &mut self,
        module: &ModuleId,
        service: DtcService,
    ) -> Option<DiagnosticScanResult> {
        let key = (module.0.clone(), service);
        let entry = self.cached.get_mut(&key)?;
        if entry.skips_remaining == 0 {
            self.cached.remove(&key);
            return None;
        }
        entry.skips_remaining -= 1;
        Some(entry.result.clone())
    }

    fn observe(&mut self, module: &ModuleId, service: DtcService, result: &DiagnosticScanResult) {
        let key = (module.0.clone(), service);
        if matches!(
            result,
            DiagnosticScanResult::NoData | DiagnosticScanResult::Unsupported(_)
        ) {
            self.cached.insert(
                key,
                GmClass2BackoffEntry {
                    skips_remaining: 3,
                    result: result.clone(),
                },
            );
        } else {
            self.cached.remove(&key);
        }
    }
}

fn dtc_status_for_service(service: DtcService) -> DtcStatus {
    match service {
        DtcService::Stored => DtcStatus::Stored,
        DtcService::Pending => DtcStatus::Pending,
        DtcService::Permanent => DtcStatus::Permanent,
        DtcService::GmClass2All | DtcService::GmClass2Active => DtcStatus::Stored,
    }
}

fn decode_dtc_bytes(data: &[u8], status: DtcStatus) -> Vec<Dtc> {
    let mut dtcs = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 {
            i += 2;
            continue;
        }
        let mut dtc = Dtc::from_bytes(data[i], data[i + 1]);
        dtc.status = status;
        dtcs.push(dtc);
        i += 2;
    }
    dtcs
}

async fn poll_o2_monitoring<A: Adapter>(
    session: &mut Session<A>,
    tx: &mpsc::UnboundedSender<Message>,
) {
    match session.read_all_o2_monitoring().await {
        Ok(results) => {
            let readings: Vec<O2Reading> = results
                .into_iter()
                .map(|r| O2Reading {
                    test_name: r.test_name.to_string(),
                    sensor: r.sensor.to_string(),
                    value: r.value,
                    unit: r.unit.to_string(),
                })
                .collect();
            if !readings.is_empty() {
                let _ = tx.send(Message::O2MonitoringUpdate(readings));
            }
        }
        Err(e) => {
            tracing::debug!("Skipping O2 monitoring update this cycle: {}", e);
        }
    }
}

async fn handle_diagnostic_command<A: Adapter>(
    session: &mut Session<A>,
    cmd: DiagnosticCommand,
    tx: &mpsc::UnboundedSender<Message>,
) {
    match cmd {
        DiagnosticCommand::ClearAll => match session.clear_dtcs().await {
            Ok(()) => {
                let _ = tx.send(Message::ClearDtcsComplete);
            }
            Err(e) => {
                let _ = tx.send(Message::ClearDtcsError(e.to_string()));
            }
        },
        DiagnosticCommand::ClearOnModule(module_id) => {
            match session.clear_dtcs_on_module(module_id).await {
                Ok(()) => {
                    let _ = tx.send(Message::ClearDtcsComplete);
                }
                Err(e) => {
                    let _ = tx.send(Message::ClearDtcsError(e.to_string()));
                }
            }
        }
        DiagnosticCommand::FetchFreezeFrame { dtc_code, pids } => {
            let mut readings = Vec::new();
            for pid in &pids {
                match session.read_freeze_frame(*pid, 0).await {
                    Ok(reading) => {
                        if let Ok(val) = reading.value.as_f64() {
                            readings.push((*pid, val, reading.unit));
                        }
                    }
                    Err(_) => {} // Skip PIDs with no freeze-frame data
                }
            }
            if !readings.is_empty() {
                let _ = tx.send(Message::FreezeFrameResult(FreezeFrameSnapshot {
                    dtc_code,
                    readings,
                }));
            } else {
                let _ = tx.send(Message::FreezeFrameError(
                    "No freeze-frame data available".into(),
                ));
            }
        }
        DiagnosticCommand::GmActiveTest(command) => {
            let (result, record) = handle_gm_active_test(command);
            let _ = tx.send(Message::ActiveTestAttempt(Box::new(record)));
            let _ = tx.send(Message::ActiveTestResult(result));
        }
    }
}

fn handle_gm_active_test(
    command: GmActiveTestCommand,
) -> (obd2_dash::gm_active::GmActiveTestResult, GmEvidenceRecord) {
    let mut result = blocked_active_test_result(&command);
    let record = active_test_evidence_record(&command, &result);
    match write_active_test_evidence(&command, &result) {
        Ok(path) => {
            result.evidence_path = Some(path.display().to_string());
        }
        Err(error) => {
            tracing::warn!("failed to write GM active-test evidence: {error}");
        }
    }
    (result, record)
}

async fn poll_readiness<A: Adapter>(session: &mut Session<A>, tx: &mpsc::UnboundedSender<Message>) {
    match session.read_readiness().await {
        Ok(status) => {
            let _ = tx.send(Message::ReadinessUpdate(status));
        }
        Err(e) => {
            tracing::debug!("Skipping readiness update this cycle: {}", e);
        }
    }
}

fn emit_session_state<A: Adapter>(
    session: &Session<A>,
    tx: &mpsc::UnboundedSender<Message>,
    last_connection: &mut Option<ConnectionState>,
) {
    let current = ConnectionState::from_session(session.connection_state());
    if last_connection.as_ref() != Some(&current) {
        *last_connection = Some(current.clone());
        let _ = tx.send(Message::ConnectionStatus(current));
    }
}

fn emit_discovery<A: Adapter>(
    session: &Session<A>,
    tx: &mpsc::UnboundedSender<Message>,
    last_discovery: &mut Option<DiscoveryState>,
) -> bool {
    let current = session.discovery().map(DiscoveryState::from);
    if current != *last_discovery {
        *last_discovery = current.clone();
        if let Some(discovery) = current {
            let _ = tx.send(Message::DiscoveryUpdated(discovery));
        }
        return true;
    }
    false
}

fn should_force_standard_poll(pid: Pid) -> bool {
    // This LLY's supported-PID bitmap can be incomplete on J1850 VPW, while
    // direct reads for dashboard-critical PIDs work. Keep these in the poll
    // set and let the core poller silently skip real NO DATA responses.
    matches!(
        pid.0,
        0x04  // Engine load
            | 0x05  // Coolant temperature
            | 0x0B  // Intake MAP
            | 0x0C  // Engine RPM
            | 0x0D  // Vehicle speed
            | 0x0F  // Intake air temperature
            | 0x10  // MAF
            | 0x11  // Throttle position
            | 0x23  // Fuel rail gauge pressure
            | 0x33  // Barometric pressure
            | 0x42  // Control module voltage
            | 0x46  // Ambient air temperature
            | 0x5C // Engine oil temperature
    )
}

fn build_enhanced_targets<A: Adapter>(
    session: &Session<A>,
    profile_state: &ProfileState,
) -> Vec<EnhancedPollTarget> {
    let Some(discovery) = session.discovery() else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    let mut module_ids: Vec<ModuleId> = discovery.modules.keys().cloned().collect();
    module_ids.sort_by(|a, b| a.0.cmp(&b.0));
    let profile_matches_lly = session_matches_lly_profile(session);
    let loaded_lly_spec = session.spec().is_some_and(is_lly_spec_identity);

    for module_id in module_ids {
        let module_label = module_id.0.clone();
        let pids = session.module_pids(module_id.clone());
        for pid in pids {
            if loaded_lly_spec && !profile_matches_lly {
                continue;
            }
            targets.push(enhanced_target(&module_label, &module_id, pid));
        }
    }

    append_profile_enhanced_targets(session, profile_state, &mut targets);

    targets
}

fn append_profile_enhanced_targets<A: Adapter>(
    session: &Session<A>,
    profile_state: &ProfileState,
    targets: &mut Vec<EnhancedPollTarget>,
) {
    let Some(selected) = profile_state.selected.as_ref() else {
        return;
    };

    let identity = profile_identity(session);
    let context = build_vehicle_context(session, profile_state.generation, &identity);
    let registry = ProfileRegistry::with_builtins();
    let runtime = ProfileRuntime::new(&registry);
    let coverage = CoverageMap::new(Vec::new()).with_discovered_modules(
        context
            .discovered_modules
            .iter()
            .filter_map(profile_module_key)
            .collect(),
    );
    let plan = runtime.plan_poll_cycle(Some(selected), 5, &coverage);
    let Some(profile) = registry.get(selected.profile_id()) else {
        return;
    };

    for planned in plan.requests {
        let CapabilityId::Signal(key) = planned.capability else {
            continue;
        };
        let Some(signal) = profile.signals().iter().find(|signal| signal.key == key) else {
            continue;
        };
        push_profile_enhanced_target(targets, &context, selected, planned, signal);
    }
}

fn push_profile_enhanced_target(
    targets: &mut Vec<EnhancedPollTarget>,
    context: &VehicleContext,
    selected: &SelectedProfile,
    planned: PlannedRequest,
    signal: &SignalDefinition,
) {
    let did = signal_did(signal);
    let module_id = planned.route.module.to_core_module_id();
    if targets
        .iter()
        .any(|target| target.did == did && target.module_id == module_id)
    {
        return;
    }

    targets.push(EnhancedPollTarget {
        did,
        module_id,
        module_label: planned.route.module.canonical().to_string(),
        name: signal.label.to_string(),
        unit: signal.unit.to_string(),
        profile_request: Some(ProfileEnhancedPollTarget {
            context: context.clone(),
            selected: selected.clone(),
            capability: planned.capability,
            request: planned.request,
        }),
        confidence: Some(profile_confidence_label(signal.confidence).to_string()),
        evidence: Some(profile_provenance_label(signal.provenance)),
    });
}

fn session_matches_lly_profile<A: Adapter>(session: &Session<A>) -> bool {
    let vin = session
        .vehicle()
        .map(|profile| profile.vin.as_str())
        .unwrap_or_default();
    let protocol = session
        .discovery()
        .map(|discovery| discovery.selected_protocol)
        .unwrap_or(session.adapter_info().protocol);

    lly_profile_matches(vin, session.spec(), protocol)
}

fn profile_identity<A: Adapter>(session: &Session<A>) -> IdentityOutcome {
    let vin = session.vehicle().map(|profile| profile.vin.clone());
    let confidence = if vin.as_deref().is_some_and(validate_vin_charset) {
        IdentityConfidence::Confirmed
    } else {
        IdentityConfidence::Unread
    };
    IdentityOutcome { vin, confidence }
}

fn profile_module_key(module: &ModuleId) -> Option<obd2_dash::profiles::ModuleKey> {
    match module.0.as_str() {
        "ecm" => Some(obd2_dash::profiles::ModuleKey::Ecm),
        "tcm" => Some(obd2_dash::profiles::ModuleKey::Tcm),
        "ficm" => Some(obd2_dash::profiles::ModuleKey::Ficm),
        "bcm" => Some(obd2_dash::profiles::ModuleKey::Bcm),
        "abs" => Some(obd2_dash::profiles::ModuleKey::Ebcm),
        "ipc" => Some(obd2_dash::profiles::ModuleKey::Ipc),
        "airbag" => Some(obd2_dash::profiles::ModuleKey::Sdm),
        "hvac" => Some(obd2_dash::profiles::ModuleKey::Hvac),
        _ => None,
    }
}

fn signal_did(signal: &SignalDefinition) -> u16 {
    u16::from_be_bytes([signal.request_data[0], signal.request_data[1]])
}

fn profile_provenance_label(provenance: &[ProfileProvenance]) -> String {
    let mut label = String::new();
    for (idx, item) in provenance.iter().enumerate() {
        if idx > 0 {
            label.push('+');
        }
        label.push_str(profile_provenance_part(*item));
    }
    label
}

fn profile_provenance_part(provenance: ProfileProvenance) -> &'static str {
    match provenance {
        ProfileProvenance::ScanGaugePublished => "scangauge-published",
        ProfileProvenance::LiveObserved => "live-observed",
        ProfileProvenance::LegacySpec => "legacy-spec",
        ProfileProvenance::LocalRejection => "local-rejection",
        ProfileProvenance::LocalFixture => "local-fixture",
    }
}

fn identity_confidence_label(confidence: IdentityConfidence) -> &'static str {
    match confidence {
        IdentityConfidence::Unread => "unread",
        IdentityConfidence::Corrupted => "corrupted",
        IdentityConfidence::Single => "single",
        IdentityConfidence::Confirmed => "confirmed",
    }
}

fn publish_enhanced_targets(targets: &[EnhancedPollTarget], tx: &mpsc::UnboundedSender<Message>) {
    tracing::debug!(count = targets.len(), "Configured enhanced PID targets");
    for target in targets {
        let _ = tx.send(Message::EnhancedPidTarget {
            did: target.did,
            module: target.module_label.clone(),
            name: target.name.clone(),
            unit: target.unit.clone(),
            confidence: target.confidence.clone(),
            evidence: target.evidence.clone(),
        });
    }
}

fn enhanced_target(
    module_label: &str,
    module_id: &ModuleId,
    pid: &EnhancedPid,
) -> EnhancedPollTarget {
    EnhancedPollTarget {
        did: pid.did,
        module_id: module_id.clone(),
        module_label: module_label.to_string(),
        name: pid.name.clone(),
        unit: pid.unit.clone(),
        profile_request: None,
        confidence: Some(core_confidence_label(pid.confidence).to_string()),
        evidence: None,
    }
}

fn core_confidence_label(confidence: CoreConfidence) -> &'static str {
    match confidence {
        CoreConfidence::Verified => "verified",
        CoreConfidence::Community => "community",
        CoreConfidence::Inferred => "inferred",
        CoreConfidence::Unverified => "unverified",
    }
}

fn profile_confidence_label(confidence: ProfileConfidence) -> &'static str {
    match confidence {
        ProfileConfidence::Candidate => "candidate",
        ProfileConfidence::LiveObserved => "live-observed",
        ProfileConfidence::Community => "community",
        ProfileConfidence::Verified => "verified",
        ProfileConfidence::Rejected => "rejected",
    }
}

fn enrich_dtcs<A: Adapter>(session: &Session<A>, dtcs: &mut Vec<Dtc>) {
    obd2_core::session::diagnostics::enrich_dtcs(dtcs, session.spec());
}

fn compute_profile_state<A: Adapter>(
    session: &Session<A>,
    generation: u64,
    identity: &IdentityOutcome,
) -> (ProfileState, ProfileContextFingerprint) {
    let context = build_vehicle_context(session, generation, identity);
    let fingerprint = ProfileContextFingerprint::from_context(&context);
    let registry = ProfileRegistry::with_builtins();
    let state = select_into_state(&registry, &context);
    (state, fingerprint)
}

fn refresh_profile_selection_if_needed<A: Adapter>(
    session: &Session<A>,
    prepared: &mut PreparedSession,
    tx: &mpsc::UnboundedSender<Message>,
) -> bool {
    sync_identity_from_session(session, &mut prepared.identity);

    let current_generation = prepared.profile_state.generation;
    let context = build_vehicle_context(session, current_generation, &prepared.identity);
    let current = ProfileContextFingerprint::from_context(&context);
    if current == prepared.profile_context {
        return false;
    }

    let generation = next_generation();
    let context = build_vehicle_context(session, generation, &prepared.identity);
    let registry = ProfileRegistry::with_builtins();
    prepared.profile_state = select_into_state(&registry, &context);
    prepared.profile_context = ProfileContextFingerprint::from_context(&context);
    publish_profile_state(&prepared.profile_state, tx);
    true
}

fn sync_identity_from_session<A: Adapter>(session: &Session<A>, identity: &mut IdentityOutcome) {
    let Some(vehicle) = session.vehicle() else {
        return;
    };
    if identity.vin.as_deref() == Some(vehicle.vin.as_str()) {
        return;
    }

    identity.vin = Some(vehicle.vin.clone());
    identity.confidence = if validate_vin_charset(&vehicle.vin) {
        IdentityConfidence::Single
    } else {
        IdentityConfidence::Corrupted
    };
}

fn publish_profile_state(state: &ProfileState, tx: &mpsc::UnboundedSender<Message>) {
    let _ = tx.send(Message::ProfileStateUpdate(state.snapshot()));
    if let Some(confidence) = state.vin_confidence {
        if let Some(warning) = confidence_warning(confidence) {
            let _ = tx.send(Message::IdentityConfidenceWarning(warning));
        }
    }
}

impl ProfileContextFingerprint {
    fn from_context(ctx: &VehicleContext) -> Self {
        Self {
            protocol: ctx.protocol,
            vin: ctx.vin.clone(),
            vin_confidence: ctx.vin_confidence,
            spec_identity: ctx.spec.as_ref().map(|spec| {
                format!(
                    "{}:{}:{}:{}",
                    spec.identity.name,
                    spec.identity.engine.code,
                    spec.identity.model_years.0,
                    spec.identity.model_years.1
                )
            }),
            active_bus: ctx.active_bus.clone(),
            discovered_modules: ctx
                .discovered_modules
                .iter()
                .map(|module| module.0.clone())
                .collect(),
        }
    }
}

pub fn build_mock_capture_handle() -> CaptureHandle {
    CaptureHandle::new(PathBuf::from("recordings"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use std::time::Instant;

    use async_trait::async_trait;
    use obd2_core::adapter::mock::MockAdapter;
    use obd2_core::adapter::{
        AdapterInfo, Capabilities, Chipset, InitializationReport, PhysicalTarget, RoutedRequest,
    };
    use obd2_core::protocol::enhanced::{Reading, ReadingSource, Value};
    use obd2_core::protocol::service::ServiceRequest;
    use obd2_core::vehicle::PhysicalAddress;

    #[derive(Clone)]
    struct CountingDtcAdapter {
        routed_writes: Arc<AtomicUsize>,
    }

    impl CountingDtcAdapter {
        fn new() -> Self {
            Self {
                routed_writes: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn routed_writes(&self) -> Arc<AtomicUsize> {
            Arc::clone(&self.routed_writes)
        }
    }

    #[async_trait]
    impl Adapter for CountingDtcAdapter {
        async fn initialize(&mut self) -> Result<InitializationReport, Obd2Error> {
            Ok(InitializationReport {
                info: AdapterInfo {
                    chipset: Chipset::Elm327Genuine,
                    firmware: "test".into(),
                    protocol: Protocol::J1850Vpw,
                    capabilities: Capabilities::default(),
                },
                probe_attempts: Vec::new(),
                events: Vec::new(),
            })
        }

        async fn request(&mut self, req: &ServiceRequest) -> Result<Vec<u8>, Obd2Error> {
            if req.service_id == 0x09 && req.data.as_slice() == [0x02] {
                Ok(b"1GTHK29294E391526".to_vec())
            } else {
                Ok(Vec::new())
            }
        }

        async fn routed_request(&mut self, req: &RoutedRequest) -> Result<Vec<u8>, Obd2Error> {
            if matches!(req.target, PhysicalTarget::Broadcast)
                && req.service_id == 0x09
                && req.data.as_slice() == [0x02]
            {
                return Ok(b"1GTHK29294E391526".to_vec());
            }
            match req.target {
                PhysicalTarget::Addressed(PhysicalAddress::J1850 { .. }) => {
                    self.routed_writes.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![0x00, 0x00, 0x00])
                }
                _ => Ok(Vec::new()),
            }
        }

        async fn supported_pids(&mut self) -> Result<HashSet<Pid>, Obd2Error> {
            Ok(HashSet::new())
        }

        async fn battery_voltage(&mut self) -> Result<Option<f64>, Obd2Error> {
            Ok(Some(12.6))
        }

        fn info(&self) -> &AdapterInfo {
            static INFO: std::sync::OnceLock<AdapterInfo> = std::sync::OnceLock::new();
            INFO.get_or_init(|| AdapterInfo {
                chipset: Chipset::Elm327Genuine,
                firmware: "test".into(),
                protocol: Protocol::J1850Vpw,
                capabilities: Capabilities::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_drain_poll_events_translates_reading_and_error() {
        let (poll_tx, mut poll_rx) = mpsc::channel(8);
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        poll_tx
            .send(PollEvent::Reading {
                pid: Pid::ENGINE_RPM,
                reading: Reading {
                    value: Value::Scalar(1234.0),
                    unit: Pid::ENGINE_RPM.unit(),
                    timestamp: Instant::now(),
                    raw_bytes: vec![0x13, 0x88],
                    source: ReadingSource::Live,
                },
            })
            .await
            .unwrap();
        poll_tx
            .send(PollEvent::Error {
                pid: Some(Pid::COOLANT_TEMP),
                error: "timeout".into(),
            })
            .await
            .unwrap();
        drop(poll_tx);

        drain_poll_events(&mut poll_rx, &msg_tx).await;

        match msg_rx.recv().await {
            Some(Message::PidUpdate(pid, reading)) => {
                assert_eq!(pid, Pid::ENGINE_RPM);
                assert_eq!(reading.value.as_f64().unwrap(), 1234.0);
            }
            other => panic!("unexpected first message: {:?}", other),
        }

        match msg_rx.recv().await {
            Some(Message::Error(message)) => {
                assert!(message.contains("Coolant"));
                assert!(message.contains("timeout"));
            }
            other => panic!("unexpected second message: {:?}", other),
        }
    }

    #[test]
    fn test_stale_pid_response_error_detects_mismatched_positive_response() {
        assert!(is_stale_pid_response_error(
            Pid::BAROMETRIC_PRESSURE,
            "parse error: no valid payload in response: 4123007B\r\r>"
        ));
        assert!(!is_stale_pid_response_error(
            Pid::BAROMETRIC_PRESSURE,
            "parse error: no valid payload in response: 413364\r\r>"
        ));
        assert!(!is_stale_pid_response_error(
            Pid::BAROMETRIC_PRESSURE,
            "timeout"
        ));
    }

    #[tokio::test]
    async fn test_drain_poll_events_suppresses_stale_pid_response_error() {
        let (poll_tx, mut poll_rx) = mpsc::channel(8);
        let (msg_tx, mut msg_rx) = mpsc::unbounded_channel();

        poll_tx
            .send(PollEvent::Error {
                pid: Some(Pid::BAROMETRIC_PRESSURE),
                error: "parse error: no valid payload in response: 4123007B".into(),
            })
            .await
            .unwrap();
        drop(poll_tx);

        drain_poll_events(&mut poll_rx, &msg_tx).await;

        assert!(msg_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_build_enhanced_targets_gates_lly_spec_when_protocol_does_not_match() {
        let vin = crate::mock_profile::mock_vin("chevy");
        let adapter = MockAdapter::with_vin(vin);
        let mut session = Session::new(adapter);
        session.initialize().await.unwrap();
        session.identify_vehicle().await.unwrap();

        let identity = IdentityOutcome {
            vin: session.vehicle().map(|profile| profile.vin.clone()),
            confidence: IdentityConfidence::Single,
        };
        let (profile_state, _) = compute_profile_state(&session, next_generation(), &identity);
        let targets = build_enhanced_targets(&session, &profile_state);
        assert!(
            targets.is_empty(),
            "LLY enhanced targets must not be polled without a J1850 VPW LLY profile match"
        );
    }

    #[test]
    fn test_should_force_barometric_standard_poll() {
        assert!(should_force_standard_poll(Pid::BAROMETRIC_PRESSURE));
    }

    #[test]
    fn test_should_force_dashboard_standard_polls() {
        for pid in [
            Pid::ENGINE_LOAD,
            Pid::COOLANT_TEMP,
            Pid::INTAKE_MAP,
            Pid::ENGINE_RPM,
            Pid::VEHICLE_SPEED,
            Pid::INTAKE_AIR_TEMP,
            Pid::MAF,
            Pid::THROTTLE_POSITION,
            Pid::FUEL_RAIL_GAUGE_PRESSURE,
            Pid::BAROMETRIC_PRESSURE,
            Pid::CONTROL_MODULE_VOLTAGE,
            Pid::AMBIENT_AIR_TEMP,
            Pid::ENGINE_OIL_TEMP,
        ] {
            assert!(
                should_force_standard_poll(pid),
                "PID {pid} should be forced"
            );
        }
        assert!(!should_force_standard_poll(Pid::FUEL_TANK_LEVEL));
    }

    #[test]
    fn test_decode_dtc_bytes_sets_status_and_skips_padding() {
        let dtcs = decode_dtc_bytes(&[0x25, 0x63, 0x00, 0x00], DtcStatus::Pending);

        assert_eq!(dtcs.len(), 1);
        assert_eq!(dtcs[0].code, "P2563");
        assert_eq!(dtcs[0].status, DtcStatus::Pending);
    }

    #[test]
    fn test_profile_dispatch_decode_error_preserves_decoder_message() {
        let result = profile_dispatch_scan_result(DispatchError::Decode(
            ProfileDecodeError::Decode("unsupported payload length 2: 4379".into()),
        ));

        assert_eq!(
            result,
            DiagnosticScanResult::Error("unsupported payload length 2: 4379".into())
        );
    }

    #[test]
    fn test_profile_dispatch_negative_response_maps_unsupported() {
        let result = profile_dispatch_scan_result(DispatchError::Decode(
            ProfileDecodeError::NegativeResponse {
                service: 0x19,
                nrc: 0x12,
            },
        ));

        assert!(matches!(result, DiagnosticScanResult::Unsupported(_)));
    }

    #[test]
    fn test_profile_dispatch_non_unsupported_negative_response_is_plain_error() {
        let result = profile_dispatch_scan_result(DispatchError::Decode(
            ProfileDecodeError::NegativeResponse {
                service: 0x19,
                nrc: 0x78,
            },
        ));

        match result {
            DiagnosticScanResult::Error(message) => {
                assert!(message.contains("service 0x19"));
                assert!(!message.contains("Decode("));
                assert!(!message.contains("NegativeResponse {"));
            }
            other => panic!("unexpected scan result: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_profile_dtc_backoff_suppresses_three_scan_writes() {
        let adapter = CountingDtcAdapter::new();
        let routed_writes = adapter.routed_writes();
        let mut session = Session::new(adapter);
        session.initialize().await.unwrap();
        session.identify_vehicle().await.unwrap();

        let identity = IdentityOutcome {
            vin: Some("1GTHK29294E391526".into()),
            confidence: IdentityConfidence::Confirmed,
        };
        let (profile_state, _) = compute_profile_state(&session, next_generation(), &identity);
        assert!(profile_state.selected.is_some());

        let modules = dtc_scan_modules(&session);
        assert!(
            !modules.is_empty(),
            "fixture VIN should resolve LLY discovered modules"
        );

        let mut backoff = GmClass2Backoff::default();
        for module in &modules {
            for service in [DtcService::GmClass2All, DtcService::GmClass2Active] {
                backoff.observe(module, service, &DiagnosticScanResult::NoData);
            }
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let baseline = routed_writes.load(Ordering::SeqCst);
        for cycle in [60, 120, 180] {
            let mut scan = DtcScan {
                dtcs: Vec::new(),
                entries: Vec::new(),
                profile_notes: Vec::new(),
            };
            append_profile_dtcs(
                &mut session,
                &mut scan,
                &tx,
                cycle,
                &profile_state,
                &identity,
                &mut backoff,
            )
            .await;
            assert_eq!(routed_writes.load(Ordering::SeqCst), baseline);
            assert_eq!(scan.entries.len(), modules.len() * 2);
            assert!(scan
                .entries
                .iter()
                .all(|entry| entry.result == DiagnosticScanResult::NoData));
            assert!(rx.try_recv().is_err(), "cached backoff emitted evidence");
        }

        let mut scan = DtcScan {
            dtcs: Vec::new(),
            entries: Vec::new(),
            profile_notes: Vec::new(),
        };
        append_profile_dtcs(
            &mut session,
            &mut scan,
            &tx,
            240,
            &profile_state,
            &identity,
            &mut backoff,
        )
        .await;

        assert!(routed_writes.load(Ordering::SeqCst) > baseline);
        assert!(
            matches!(rx.try_recv(), Ok(Message::ProfileEvidence(_))),
            "real profile DTC retry should emit profile evidence"
        );
    }

    #[tokio::test]
    async fn test_emit_discovery_only_sends_on_change() {
        let vin = crate::mock_profile::mock_vin("chevy");
        let adapter = MockAdapter::with_vin(vin);
        let mut session = Session::new(adapter);
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut last_discovery = None;

        session.initialize().await.unwrap();
        session.identify_vehicle().await.unwrap();

        assert!(emit_discovery(&session, &tx, &mut last_discovery));
        match rx.recv().await {
            Some(Message::DiscoveryUpdated(discovery)) => {
                assert_eq!(Some(discovery), last_discovery);
            }
            other => panic!("unexpected discovery message: {:?}", other),
        }

        assert!(!emit_discovery(&session, &tx, &mut last_discovery));
        assert!(rx.try_recv().is_err());
    }
}
