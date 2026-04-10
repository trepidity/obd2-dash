use std::path::PathBuf;
use std::time::Duration;

use tokio::sync::mpsc;

use obd2_core::adapter::Adapter;
use obd2_core::protocol::dtc::Dtc;
use obd2_core::protocol::enhanced::EnhancedPid;
use obd2_core::protocol::pid::Pid;
use obd2_core::session::poller::{execute_poll_cycle, PollConfig, PollEvent};
use obd2_core::session::Session;
use obd2_core::vehicle::ModuleId;

use crate::app::{CaptureCommand, CaptureHandle, Message};
use crate::domain::{ConnectionState, DiscoveryState, O2Reading};

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
    let _ = tx.send(Message::ConnectionStatus(ConnectionState::ProtocolNegotiating));

    match session.initialize().await {
        Ok(info) => {
            let _ = tx.send(Message::AdapterDetected(info.clone()));
        }
        Err(e) => {
            let msg = format!("Init failed: {e}");
            let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(msg.clone())));
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

    let pids_to_poll = match session.supported_pids().await {
        Ok(supported) if !supported.is_empty() => config
            .standard_pids
            .iter()
            .copied()
            .filter(|pid| supported.contains(pid))
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
) {
    let (poll_tx, mut poll_rx) = mpsc::channel(256);
    let mut interval = tokio::time::interval(Duration::from_millis(prepared.poll_ms));
    let mut cycle = 0u32;

    loop {
        interval.tick().await;

        while let Ok(cmd) = capture_rx.try_recv() {
            match cmd {
                CaptureCommand::Start { path, metadata } => match session.start_raw_capture(&path, &metadata) {
                    Ok(()) => {
                        capture_handle.set_active(true);
                        let _ = tx.send(Message::RawCaptureStarted);
                    }
                    Err(e) => {
                        let _ = tx.send(Message::RawCaptureError(e.to_string()));
                    }
                },
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

        execute_poll_cycle(session, &prepared.poll_config, &poll_tx, None).await;
        drain_poll_events(&mut poll_rx, tx).await;
        emit_session_state(session, tx, &mut prepared.last_connection);
        if emit_discovery(session, tx, &mut prepared.last_discovery) {
            prepared.enhanced_targets = build_enhanced_targets(session);
        }

        cycle += 1;

        if !prepared.enhanced_targets.is_empty() && cycle % 5 == 0 {
            poll_enhanced(session, &prepared.enhanced_targets, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            if emit_discovery(session, tx, &mut prepared.last_discovery) {
                prepared.enhanced_targets = build_enhanced_targets(session);
            }
        }

        if cycle % 10 == 0 {
            poll_dtcs(session, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            if emit_discovery(session, tx, &mut prepared.last_discovery) {
                prepared.enhanced_targets = build_enhanced_targets(session);
            }
        }

        if cycle % 20 == 0 {
            poll_o2_monitoring(session, tx).await;
            emit_session_state(session, tx, &mut prepared.last_connection);
            if emit_discovery(session, tx, &mut prepared.last_discovery) {
                prepared.enhanced_targets = build_enhanced_targets(session);
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
) -> Result<(), String> {
    let prepared = prepare_session(session, config, tx).await?;
    run_prepared_session(session, prepared, tx, capture_rx, capture_handle).await;
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
                let message = pid
                    .map(|pid| format!("{pid}: {error}"))
                    .unwrap_or(error);
                let _ = tx.send(Message::Error(message));
            }
            PollEvent::EnhancedReading { .. } => {
                tracing::debug!("Ignoring core poller EnhancedReading; dash uses explicit enhanced cadence");
            }
            PollEvent::Alert(result) => {
                tracing::debug!("Ignoring core poller alert to avoid double-thresholding: {:?}", result);
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
            }
        }
    }
}

async fn poll_dtcs<A: Adapter>(session: &mut Session<A>, tx: &mpsc::UnboundedSender<Message>) {
    match session.read_all_dtcs().await {
        Ok(mut dtcs) => {
            enrich_dtcs(session, &mut dtcs);
            let _ = tx.send(Message::DtcUpdate(dtcs));
        }
        Err(e) => {
            tracing::debug!("Skipping DTC update this cycle: {}", e);
        }
    }
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

fn enhanced_target(module_label: &str, module_id: &ModuleId, pid: &EnhancedPid) -> EnhancedPollTarget {
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

    #[tokio::test]
    async fn test_build_enhanced_targets_from_identified_session() {
        let vin = crate::mock_profile::mock_vin("chevy");
        let adapter = MockAdapter::with_vin(vin);
        let mut session = Session::new(adapter);
        session.initialize().await.unwrap();
        session.identify_vehicle().await.unwrap();

        let discovery = session.discovery().expect("discovery should be populated");
        let expected: Vec<(String, u16, String, String)> = discovery
            .modules
            .keys()
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .flat_map(|module_id| {
                let module_label = module_id.0.clone();
                session
                    .module_pids(module_id.clone())
                    .into_iter()
                    .map(move |pid| (module_label.clone(), pid.did, pid.name.clone(), pid.unit.clone()))
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
