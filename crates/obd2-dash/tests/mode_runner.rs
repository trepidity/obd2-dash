use async_trait::async_trait;
use obd2_core::adapter::mock::MockAdapter;
use obd2_core::session::Session;
use obd2_dash::mode_runner::{
    CapabilityKey, CapabilityPersistence, CapabilityStore, CommandReply, ConnectError, ModeRunner,
    ModeState, NewSession, Persistence, ProbeError, RunnerCommand, SessionConnector, Tier,
    Verifier, ViewId,
};
use obd2_db::models::{
    CapabilityContext, CapabilityKind, CapabilityLoad, CapabilityOutcome, CapabilityRecord,
    CapabilitySetReplacement, OutcomeUpdate,
};
use obd2_db::Database;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

const VIN: &str = "1GCHK23224F000001";

#[derive(Clone)]
struct CountingConnector {
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl SessionConnector for CountingConnector {
    type Adapter = MockAdapter;

    async fn connect(&self) -> Result<NewSession<Self::Adapter>, ConnectError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(NewSession {
            session: Session::new(MockAdapter::with_vin(VIN)),
        })
    }
}

#[derive(Clone, Default)]
struct FailingStore;

#[async_trait]
impl CapabilityStore for FailingStore {
    async fn load(
        &self,
        _vin: &str,
        _context: &CapabilityContext,
    ) -> anyhow::Result<CapabilityLoad> {
        Err(anyhow::anyhow!("database unavailable"))
    }

    async fn replace(&self, _replacement: &CapabilitySetReplacement) -> anyhow::Result<String> {
        Err(anyhow::anyhow!("database unavailable"))
    }

    async fn update_outcomes(
        &self,
        _vin: &str,
        _set_id: &str,
        _records: &[CapabilityRecord],
    ) -> anyhow::Result<OutcomeUpdate> {
        Err(anyhow::anyhow!("database unavailable"))
    }

    async fn load_exact_vehicle_fuel_type(&self, _vin: &str) -> anyhow::Result<Option<String>> {
        Err(anyhow::anyhow!("database unavailable"))
    }
}

fn context() -> CapabilityContext {
    CapabilityContext {
        protocol: obd2_dash::mode_runner::protocol_token(obd2_core::vehicle::Protocol::Can11Bit500)
            .into(),
        profile_id: "generic".into(),
        probe_schema_version: 1,
        probe_fingerprint: obd2_dash::mode_runner::default_probe_fingerprint(),
    }
}

#[tokio::test]
async fn cache_miss_refreshes_masks_before_telemetry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let connector = CountingConnector {
        calls: Arc::clone(&calls),
    };
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(connector, store);

    runner.connect().await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
    assert!(!runner.capabilities().is_empty());
}

#[tokio::test]
async fn run_diagnostic_is_rejected_until_telemetry() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(ScriptedConnector::new(VIN), store);
    assert_eq!(
        runner.command(RunnerCommand::RunDiagnostic),
        CommandReply::NotReady
    );
    runner.connect().await.unwrap();
    assert_eq!(
        runner.command(RunnerCommand::RunDiagnostic),
        CommandReply::Accepted
    );
}

#[tokio::test]
async fn duplicate_foreground_command_returns_busy_without_queueing() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(ScriptedConnector::new(VIN), store);
    runner.connect().await.unwrap();
    assert_eq!(
        runner.command(RunnerCommand::RescanVehicle),
        CommandReply::Accepted
    );
    assert_eq!(
        runner.command(RunnerCommand::RunDiagnostic),
        CommandReply::Busy
    );
    assert!(matches!(
        runner.snapshot().mode,
        ModeState::Discovering { .. }
    ));
}

#[tokio::test]
async fn foreground_command_pauses_and_resumes_background_verifier() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(ScriptedConnector::new(VIN), store);
    runner.connect().await.unwrap();
    assert_eq!(
        runner.command(RunnerCommand::RunDiagnostic),
        CommandReply::Accepted
    );
    assert_eq!(
        runner.command(RunnerCommand::CancelForeground),
        CommandReply::Accepted
    );
    assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
}

#[tokio::test]
async fn poll_cycle_executes_a_request_and_publishes_snapshot() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    store
        .replace(&CapabilitySetReplacement {
            vin: VIN.into(),
            context: context(),
            completed_at: "now".into(),
            records: vec![CapabilityRecord {
                kind: CapabilityKind::Pid,
                request_id: "010C".into(),
                module: "broadcast".into(),
                outcome: CapabilityOutcome::Supported,
                observation_seq: 1,
                rtt_ms: None,
                attempted_at: "now".into(),
                error_code: None,
            }],
        })
        .await
        .unwrap();
    let connector = CountingConnector {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut runner = ModeRunner::new(connector, store);
    let mut updates = runner.subscribe();
    runner.connect().await.unwrap();

    assert!(runner.poll_cycle().await.unwrap());
    assert!(updates.changed().await.is_ok());
    assert!(runner.snapshot().sample_at.is_some());
    assert!(!runner.snapshot().signals.is_empty());
}

#[tokio::test]
async fn cache_hit_starts_telemetry_with_cached_capabilities() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    store
        .replace(&CapabilitySetReplacement {
            vin: VIN.into(),
            context: context(),
            completed_at: "now".into(),
            records: vec![CapabilityRecord {
                kind: CapabilityKind::Pid,
                request_id: "010C".into(),
                module: "broadcast".into(),
                outcome: CapabilityOutcome::Supported,
                observation_seq: 1,
                rtt_ms: None,
                attempted_at: "now".into(),
                error_code: None,
            }],
        })
        .await
        .unwrap();

    let connector = CountingConnector {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();

    assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
    assert_eq!(
        runner.snapshot().capability.persistence,
        CapabilityPersistence::Cached
    );
    assert_eq!(
        runner.capabilities().outcome(&key),
        CapabilityOutcome::Supported
    );
}

#[tokio::test]
async fn reconnect_constructs_a_new_session() {
    let calls = Arc::new(AtomicUsize::new(0));
    let connector = CountingConnector {
        calls: Arc::clone(&calls),
    };
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    runner.reconnect().await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
}

#[tokio::test]
async fn store_failure_keeps_session_local_telemetry_usable() {
    let connector = CountingConnector {
        calls: Arc::new(AtomicUsize::new(0)),
    };
    let mut runner = ModeRunner::new(connector, FailingStore);
    runner.connect().await.unwrap();

    assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
    assert_eq!(
        runner.snapshot().capability.persistence,
        CapabilityPersistence::SessionOnlyStoreError
    );
    let rpm = runner
        .read_pid(obd2_core::protocol::pid::Pid(0x0C))
        .await
        .unwrap();
    assert!(rpm > 0.0);
}

#[test]
fn verifier_keeps_transient_failures_unverified_and_applies_backoff() {
    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    let mut verifier = Verifier::new();
    verifier.insert(key.clone(), Tier::A, None);
    let now = Instant::now();

    let first = verifier
        .classify(&key, Err(ProbeError::Timeout), now)
        .unwrap();
    assert_eq!(first.outcome, CapabilityOutcome::Unverified);
    assert!(first.retry_after.is_some());
    assert!(verifier.next(now, &ViewId::Gauges).is_none());
    assert_eq!(verifier.outcome(&key), CapabilityOutcome::Unverified);
}

#[tokio::test]
async fn persistence_coalesces_latest_observation_and_installs_set_id() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut persistence = Persistence::new(store, VIN, context());
    let set_id = persistence.replace(Vec::new()).await.unwrap();
    assert_eq!(persistence.set_id(), Some(set_id.as_str()));

    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    persistence.observe(
        key.clone(),
        CapabilityOutcome::Unverified,
        Some("timeout".into()),
    );
    persistence.observe(key, CapabilityOutcome::Supported, None);
    assert_eq!(persistence.pending_len(), 1);
    assert_eq!(
        persistence.flush().await.unwrap(),
        Some(OutcomeUpdate::Applied)
    );
    assert_eq!(persistence.pending_len(), 0);
}

// ── Slice-3 audit regression tests ────────────────────────────────────────

use obd2_dash::mode_runner::testing::{ScriptedConnector, ScriptedResponse};

async fn seed_supported(store: &obd2_dash::mode_runner::SqliteCapabilityStore, pids: &[&str]) {
    store
        .replace(&CapabilitySetReplacement {
            vin: VIN.into(),
            context: context(),
            completed_at: "now".into(),
            records: pids
                .iter()
                .enumerate()
                .map(|(idx, request_id)| CapabilityRecord {
                    kind: CapabilityKind::Pid,
                    request_id: (*request_id).into(),
                    module: "broadcast".into(),
                    outcome: CapabilityOutcome::Supported,
                    observation_seq: idx as i64 + 1,
                    rtt_ms: None,
                    attempted_at: "now".into(),
                    error_code: None,
                })
                .collect(),
        })
        .await
        .unwrap();
}

/// Regression: poll_cycle executed only the first planned request, so every
/// gauge except one froze. A full cycle must poll every supported entry.
#[tokio::test]
async fn telemetry_polls_every_supported_request_per_cycle() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    seed_supported(&store, &["010C", "010D"]).await;

    let connector = ScriptedConnector::new(VIN);
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    runner.poll_cycle().await.unwrap();

    let signals = runner.snapshot().signals;
    assert!(signals.contains_key("010C"), "RPM missing: {signals:?}");
    assert!(signals.contains_key("010D"), "speed missing: {signals:?}");
}

/// Spec §10: a failing supported request demotes to Unverified for verifier
/// reclassification; it never flips directly to Unsupported.
#[tokio::test]
async fn supported_telemetry_failure_demotes_to_verifier_not_unsupported() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    seed_supported(&store, &["010C"]).await;

    let connector = ScriptedConnector::new(VIN);
    connector
        .script
        .push(0x01, Some(0x0C), ScriptedResponse::Timeout);
    connector
        .script
        .push(0x01, Some(0x0C), ScriptedResponse::Timeout);
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    runner.poll_cycle().await.unwrap();

    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    assert_eq!(
        runner.capabilities().outcome(&key),
        CapabilityOutcome::Unverified,
        "telemetry failure must demote, not prune"
    );
}

async fn poll_until_pid_probes(
    runner: &mut ModeRunner<ScriptedConnector, obd2_dash::mode_runner::SqliteCapabilityStore>,
    script: &obd2_dash::mode_runner::testing::RequestScript,
    pid: u8,
    count: usize,
) {
    for _ in 0..64 {
        let probes = script
            .requests()
            .await
            .iter()
            .filter(|(service, probed)| *service == 0x01 && *probed == Some(pid))
            .count();
        if probes >= count {
            return;
        }
        let _ = runner.poll_cycle().await;
        // Verifier retries pace themselves with real-time backoff (500ms
        // after the first failure); give them room instead of spinning.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("PID {pid:02X} was not probed {count} times");
}

/// Spec §13: verifier state resumes only for the same VIN. A different
/// vehicle with an identical context must start fresh — a leaked no-data
/// counter would let one NO DATA on the new truck confirm as Unsupported.
#[tokio::test]
async fn reconnect_to_new_vin_discards_partial_verifier_state() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let connector = ScriptedConnector::new(VIN);
    let script = connector.script.clone();
    let vin_handle = connector.vin_handle();
    script.push(0x01, Some(0x0C), ScriptedResponse::NoData);
    script.push(0x01, Some(0x0C), ScriptedResponse::NoData);

    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    poll_until_pid_probes(&mut runner, &script, 0x0C, 1).await;

    *vin_handle.lock().unwrap() = "1GDJK34204E000002".into();
    runner.reconnect().await.unwrap();
    poll_until_pid_probes(&mut runner, &script, 0x0C, 2).await;

    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    assert_eq!(
        runner.capabilities().outcome(&key),
        CapabilityOutcome::Unverified,
        "a fresh vehicle's first NO DATA must not confirm as Unsupported"
    );
}

/// Spec §13: same VIN and context resumes the unfinished pass, so a NO DATA
/// before the reconnect and one after form the separated confirmation.
#[tokio::test]
async fn same_context_reconnect_resumes_unfinished_initial_verifier() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let connector = ScriptedConnector::new(VIN);
    let script = connector.script.clone();
    script.push(0x01, Some(0x0C), ScriptedResponse::NoData);
    script.push(0x01, Some(0x0C), ScriptedResponse::NoData);

    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    poll_until_pid_probes(&mut runner, &script, 0x0C, 1).await;

    runner.reconnect().await.unwrap();
    poll_until_pid_probes(&mut runner, &script, 0x0C, 2).await;

    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    assert_eq!(
        runner.capabilities().outcome(&key),
        CapabilityOutcome::Unsupported,
        "resumed pass must treat the second separated NO DATA as confirmation"
    );
}

#[tokio::test]
async fn cache_miss_verifies_one_unknown_per_cycle() {
    let connector = ScriptedConnector::new(VIN);
    let script = connector.script.clone();
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    let before = script.requests().await.len();
    runner.poll_cycle().await.unwrap();
    let after = script.requests().await.len();
    assert!(after > before);
}

#[tokio::test]
async fn successful_verifier_value_is_published_immediately() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    seed_supported(&store, &["010C"]).await;
    let mut runner = ModeRunner::new(ScriptedConnector::new(VIN), store);
    runner.connect().await.unwrap();
    runner.poll_cycle().await.unwrap();
    assert!(runner.snapshot().sample_at.is_some());
    assert!(runner.snapshot().signals.contains_key("010C"));
}

#[tokio::test]
async fn fallback_never_schedules_full_legacy_pid_set() {
    let connector = ScriptedConnector::new(VIN);
    // Force the §9.2 path: the mask walk itself fails.
    connector
        .script
        .push(0x01, Some(0x00), ScriptedResponse::Timeout);
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();

    // Conservative fallback is the display set only — never the ~43-PID
    // legacy sweep (which a loose `< 64` bound would have let through).
    let staged = runner.capabilities().len();
    assert!(
        staged <= 8,
        "fallback staged {staged} capabilities; expected the conservative display set"
    );
    let legacy_only = CapabilityKey::new(CapabilityKind::Pid, "0121", "broadcast");
    assert_eq!(
        runner.capabilities().outcome(&legacy_only),
        CapabilityOutcome::Unverified,
        "legacy-sweep PIDs must stay untracked (absent reads as Unverified)"
    );
    assert!(
        !runner
            .capabilities()
            .iter()
            .any(|(key, _)| key.request_id == "0121"),
        "legacy-sweep PID 0121 must not be staged in fallback"
    );
}

#[tokio::test]
async fn missing_vin_never_calls_store_replace() {
    let connector = ScriptedConnector::new("");
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();
    assert_eq!(
        runner.snapshot().capability.persistence,
        CapabilityPersistence::SessionOnlyNoVin
    );
}

#[tokio::test]
async fn reconnect_reacquires_vin_before_cache_load() {
    const OTHER_VIN: &str = "1GDJK34204E000002";
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    // Seed a cache only under the vehicle the adapter will move to.
    store
        .replace(&CapabilitySetReplacement {
            vin: OTHER_VIN.into(),
            context: context(),
            completed_at: "now".into(),
            records: vec![CapabilityRecord {
                kind: CapabilityKind::Pid,
                request_id: "010C".into(),
                module: "broadcast".into(),
                outcome: CapabilityOutcome::Supported,
                observation_seq: 1,
                rtt_ms: None,
                attempted_at: "now".into(),
                error_code: None,
            }],
        })
        .await
        .unwrap();

    let connector = ScriptedConnector::new(VIN);
    let calls = connector.calls.clone();
    let vin_handle = connector.vin_handle();
    let mut runner = ModeRunner::new(connector, store);
    runner.connect().await.unwrap();

    // Move the adapter to the other truck. A cache hit under OTHER_VIN is
    // only possible if reconnect reacquired identity before loading — a
    // runner reusing the stale VIN would miss and enter discovery instead.
    *vin_handle.lock().unwrap() = OTHER_VIN.into();
    runner.reconnect().await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(runner.snapshot().mode, ModeState::Telemetry);
    assert_eq!(
        runner.snapshot().capability.persistence,
        CapabilityPersistence::Cached,
        "cache hit must key on the freshly reacquired VIN"
    );
    let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
    assert_eq!(
        runner.capabilities().outcome(&key),
        CapabilityOutcome::Supported
    );
}

#[tokio::test]
async fn fingerprint_mismatch_runs_discovery() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut mismatched = context();
    mismatched.probe_fingerprint = "v1:stale".into();
    store
        .replace(&CapabilitySetReplacement {
            vin: VIN.into(),
            context: mismatched,
            completed_at: "now".into(),
            records: vec![],
        })
        .await
        .unwrap();
    let mut runner = ModeRunner::new(ScriptedConnector::new(VIN), store);
    runner.connect().await.unwrap();
    assert_eq!(
        runner.snapshot().capability.persistence,
        CapabilityPersistence::Pending
    );
}

#[test]
fn tier_c_requests_are_not_due_on_fast_cycles() {
    let descriptor = obd2_dash::mode_runner::RequestDescriptor {
        key: CapabilityKey::new(CapabilityKind::Pid, "015C", "broadcast"),
        tier: Tier::C,
        every_cycles: 20,
        view: None,
    };
    let scheduler = obd2_dash::mode_runner::Scheduler::new(vec![descriptor.clone()]);
    let mut caps = obd2_dash::mode_runner::CapabilitySet::default();
    caps.insert(descriptor.key.clone(), CapabilityOutcome::Supported);
    assert!(scheduler.plan_cycle(1, &ViewId::Gauges, &caps).is_empty());
    assert_eq!(scheduler.plan_cycle(20, &ViewId::Gauges, &caps).len(), 1);
}

#[test]
fn tier_c_requests_are_view_independent_when_unrestricted() {
    let descriptor = obd2_dash::mode_runner::RequestDescriptor {
        key: CapabilityKey::new(CapabilityKind::Pid, "015C", "broadcast"),
        tier: Tier::C,
        every_cycles: 1,
        view: None,
    };
    let scheduler = obd2_dash::mode_runner::Scheduler::new(vec![descriptor.clone()]);
    let mut caps = obd2_dash::mode_runner::CapabilitySet::default();
    caps.insert(descriptor.key, CapabilityOutcome::Supported);
    assert_eq!(scheduler.plan_cycle(1, &ViewId::Engine, &caps).len(), 1);
}

#[tokio::test]
async fn runner_snapshot_preserves_generic_and_lly_signal_shapes() {
    let store = obd2_dash::mode_runner::SqliteCapabilityStore::from_database(
        Database::open_in_memory().unwrap(),
    );
    let mut runner = ModeRunner::new(ScriptedConnector::new(VIN), store);
    runner.connect().await.unwrap();
    let snapshot = runner.snapshot();
    assert!(snapshot.signals.is_empty());
    assert_eq!(
        snapshot.capability.persistence,
        CapabilityPersistence::Pending
    );
}
