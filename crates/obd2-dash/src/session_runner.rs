use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;

use obd2_core::adapter::Adapter;
use obd2_core::error::{NegativeResponse, Obd2Error};
use obd2_core::protocol::dtc::{Dtc, DtcStatus};
use obd2_core::protocol::enhanced::EnhancedPid;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::Target;
use obd2_core::session::poller::{execute_poll_cycle, PollConfig, PollEvent};
use obd2_core::session::Session;
use obd2_core::vehicle::ModuleId;

use crate::app::{CaptureCommand, CaptureHandle, DiagnosticCommand, Message};
use crate::domain::{ConnectionState, DiscoveryState, O2Reading};
use crate::domain::{
    DiagnosticScanEntry, DiagnosticScanResult, DiagnosticScanScope, DtcService, FreezeFrameSnapshot,
};

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
    last_connection: Option<ConnectionState>,
    last_discovery: Option<DiscoveryState>,
}

#[derive(Debug, Clone)]
struct EnhancedPollTarget {
    did: u16,
    module_id: ModuleId,
    module_label: String,
    name: String,
    unit: String,
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

    let enhanced_targets = match session.identify_vehicle().await {
        Ok(profile) => {
            let _ = tx.send(Message::VinDetected(profile.vin.clone()));
            emit_discovery(session, tx, &mut last_discovery);
            build_enhanced_targets(session)
        }
        Err(e) => {
            tracing::warn!("Could not identify vehicle: {e}");
            match session.read_vin().await {
                Ok(vin) => {
                    let _ = tx.send(Message::VinDetected(vin));
                }
                Err(vin_err) => {
                    tracing::warn!("Could not read VIN: {vin_err}");
                }
            }
            emit_session_state(session, tx, &mut last_connection);
            emit_discovery(session, tx, &mut last_discovery);
            build_enhanced_targets(session)
        }
    };
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
        last_connection,
        last_discovery,
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
        if emit_discovery(session, tx, &mut prepared.last_discovery) {
            prepared.enhanced_targets = build_enhanced_targets(session);
            publish_enhanced_targets(&prepared.enhanced_targets, tx);
        }

        cycle += 1;

        if !prepared.enhanced_targets.is_empty() && cycle % 5 == 0 {
            poll_enhanced(session, &prepared.enhanced_targets, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            if emit_discovery(session, tx, &mut prepared.last_discovery) {
                prepared.enhanced_targets = build_enhanced_targets(session);
                publish_enhanced_targets(&prepared.enhanced_targets, tx);
            }
        }

        if cycle % 10 == 0 {
            poll_dtcs(session, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            if emit_discovery(session, tx, &mut prepared.last_discovery) {
                prepared.enhanced_targets = build_enhanced_targets(session);
                publish_enhanced_targets(&prepared.enhanced_targets, tx);
            }
        }

        if cycle % 20 == 0 {
            poll_o2_monitoring(session, tx).await;
            poll_readiness(session, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            if emit_discovery(session, tx, &mut prepared.last_discovery) {
                prepared.enhanced_targets = build_enhanced_targets(session);
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
        match session
            .read_enhanced(target.did, target.module_id.clone())
            .await
        {
            Ok(reading) => {
                let value = reading.value.as_f64().unwrap_or(0.0);
                let _ = tx.send(Message::EnhancedPidUpdate {
                    did: target.did,
                    module: target.module_label.clone(),
                    name: target.name.clone(),
                    value,
                    unit: target.unit.clone(),
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
                });
            }
        }
    }
}

async fn poll_dtcs<A: Adapter>(session: &mut Session<A>, tx: &mpsc::UnboundedSender<Message>) {
    let mut scan = scan_standard_dtcs(session).await;
    enrich_dtcs(session, &mut scan.dtcs);
    obd2_core::session::diagnostics::dedup_dtcs(&mut scan.dtcs);

    let _ = tx.send(Message::DtcUpdate(scan.dtcs));
    let _ = tx.send(Message::DiagnosticScanUpdate(scan.entries));
}

struct DtcScan {
    dtcs: Vec<Dtc>,
    entries: Vec<DiagnosticScanEntry>,
}

async fn scan_standard_dtcs<A: Adapter>(session: &mut Session<A>) -> DtcScan {
    let mut scan = DtcScan {
        dtcs: Vec::new(),
        entries: Vec::new(),
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

fn dtc_status_for_service(service: DtcService) -> DtcStatus {
    match service {
        DtcService::Stored => DtcStatus::Stored,
        DtcService::Pending => DtcStatus::Pending,
        DtcService::Permanent => DtcStatus::Permanent,
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
    }
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

fn build_enhanced_targets<A: Adapter>(session: &Session<A>) -> Vec<EnhancedPollTarget> {
    let Some(discovery) = session.discovery() else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    let mut module_ids: Vec<ModuleId> = discovery.modules.keys().cloned().collect();
    module_ids.sort_by(|a, b| a.0.cmp(&b.0));

    for module_id in module_ids {
        let module_label = module_id.0.clone();
        let pids = session.module_pids(module_id.clone());
        for pid in pids {
            targets.push(enhanced_target(&module_label, &module_id, pid));
        }
    }

    targets
}

fn publish_enhanced_targets(targets: &[EnhancedPollTarget], tx: &mpsc::UnboundedSender<Message>) {
    tracing::debug!(count = targets.len(), "Configured enhanced PID targets");
    for target in targets {
        let _ = tx.send(Message::EnhancedPidTarget {
            did: target.did,
            module: target.module_label.clone(),
            name: target.name.clone(),
            unit: target.unit.clone(),
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
    }
}

fn enrich_dtcs<A: Adapter>(session: &Session<A>, dtcs: &mut Vec<Dtc>) {
    obd2_core::session::diagnostics::enrich_dtcs(dtcs, session.spec());
}

pub fn build_mock_capture_handle() -> CaptureHandle {
    CaptureHandle::new(PathBuf::from("recordings"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    use obd2_core::adapter::mock::MockAdapter;
    use obd2_core::protocol::enhanced::{Reading, ReadingSource, Value};

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
    async fn test_build_enhanced_targets_from_identified_session() {
        let vin = crate::mock_profile::mock_vin("chevy");
        let adapter = MockAdapter::with_vin(vin);
        let mut session = Session::new(adapter);
        session.initialize().await.unwrap();
        session.identify_vehicle().await.unwrap();

        let discovery = session.discovery().expect("discovery should be populated");
        let mut module_ids: Vec<ModuleId> = discovery.modules.keys().cloned().collect();
        module_ids.sort_by(|a, b| a.0.cmp(&b.0));
        let expected: Vec<(String, u16, String, String)> = module_ids
            .into_iter()
            .flat_map(|module_id| {
                let module_label = module_id.0.clone();
                session
                    .module_pids(module_id.clone())
                    .into_iter()
                    .map(move |pid| {
                        (
                            module_label.clone(),
                            pid.did,
                            pid.name.clone(),
                            pid.unit.clone(),
                        )
                    })
            })
            .collect();

        let targets = build_enhanced_targets(&session);
        let actual: Vec<(String, u16, String, String)> = targets
            .iter()
            .map(|target| {
                (
                    target.module_label.clone(),
                    target.did,
                    target.name.clone(),
                    target.unit.clone(),
                )
            })
            .collect();

        assert_eq!(actual, expected);
        assert!(
            targets
                .iter()
                .any(|target| target.module_label == "ecm" && target.did == 0x1543),
            "expected Duramax VGT actual DID to be polled"
        );
        assert!(
            targets
                .iter()
                .any(|target| target.module_label == "ecm" && target.did == 0x1540),
            "expected Duramax VGT desired DID to be polled"
        );
        for did in 0x162F..=0x1636 {
            assert!(
                targets
                    .iter()
                    .any(|target| target.module_label == "ecm" && target.did == did),
                "expected Duramax injector balance DID {did:#06X} to be polled"
            );
        }
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
