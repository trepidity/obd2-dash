use async_trait::async_trait;
use obd2_core::adapter::mock::MockAdapter;
use obd2_core::session::Session;
use obd2_dash::mode_runner::{
    CapabilityKey, CapabilityPersistence, CapabilityStore, ConnectError, ModeRunner, ModeState,
    NewSession, SessionConnector,
};
use obd2_db::models::{
    CapabilityContext, CapabilityKind, CapabilityLoad, CapabilityOutcome, CapabilityRecord,
    CapabilitySetReplacement, OutcomeUpdate,
};
use obd2_db::Database;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

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
        protocol: "Can11Bit500".into(),
        profile_id: "generic".into(),
        probe_schema_version: 1,
        probe_fingerprint: "mode-runner-v1".into(),
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
