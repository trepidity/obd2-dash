use std::time::Instant;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use obd2_core::adapter::Adapter;
use obd2_core::protocol::pid::Pid;
use obd2_core::session::Session;
use obd2_db::models::{
    CapabilityContext, CapabilityKind, CapabilityLoad, CapabilityOutcome, CapabilityRecord,
    CapabilitySetReplacement,
};

use super::capability::{CapabilityKey, CapabilitySet};
use super::snapshot::{CapabilityPersistence, CapabilityVerification, ModeState, RunnerSnapshot};
use super::store::CapabilityStore;
use crate::profiles::{acquire_identity, IdentityOutcome};

#[derive(Debug)]
pub enum ConnectError {
    Transport(String),
    Initialization(String),
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "transport: {error}"),
            Self::Initialization(error) => write!(f, "initialization: {error}"),
        }
    }
}

impl std::error::Error for ConnectError {}

pub struct NewSession<A: Adapter> {
    pub session: Session<A>,
}

#[async_trait]
pub trait SessionConnector: Send + Sync {
    type Adapter: Adapter;

    async fn connect(&self) -> std::result::Result<NewSession<Self::Adapter>, ConnectError>;
}

/// The transport-independent lifecycle coordinator used by the GUI and tests.
/// It owns one session at a time; a failed session is always dropped before a
/// connector retry, preventing a broken adapter from being reused.
pub struct ModeRunner<C, S>
where
    C: SessionConnector,
    S: CapabilityStore,
{
    connector: C,
    store: S,
    session: Option<Session<C::Adapter>>,
    identity: Option<IdentityOutcome>,
    vin: Option<String>,
    capabilities: CapabilitySet,
    context: Option<CapabilityContext>,
    snapshot: RunnerSnapshot,
    reconnect_attempt: u32,
}

impl<C, S> ModeRunner<C, S>
where
    C: SessionConnector,
    S: CapabilityStore,
{
    pub fn new(connector: C, store: S) -> Self {
        Self {
            connector,
            store,
            session: None,
            identity: None,
            vin: None,
            capabilities: CapabilitySet::default(),
            context: None,
            snapshot: RunnerSnapshot::empty(),
            reconnect_attempt: 0,
        }
    }

    pub fn snapshot(&self) -> RunnerSnapshot {
        self.snapshot.clone()
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    pub async fn connect(&mut self) -> Result<()> {
        self.snapshot.mode = ModeState::Connecting;
        let mut new_session = self
            .connector
            .connect()
            .await
            .map_err(|error| anyhow!(error))?;
        new_session
            .session
            .initialize()
            .await
            .map_err(|error| anyhow!(ConnectError::Initialization(error.to_string())))?;

        let identity = acquire_identity(&mut new_session.session, 2).await;
        let Some(vin) = identity.vin.clone() else {
            self.session = Some(new_session.session);
            self.identity = Some(identity);
            self.capabilities = CapabilitySet::default();
            self.snapshot.capability.persistence = CapabilityPersistence::SessionOnlyNoVin;
            self.snapshot.capability.verification = CapabilityVerification::ConservativeFallback;
            self.snapshot.mode = ModeState::Telemetry;
            return Ok(());
        };

        let context = CapabilityContext {
            protocol: format!("{:?}", new_session.session.adapter_info().protocol),
            profile_id: "generic".to_string(),
            probe_schema_version: 1,
            probe_fingerprint: "mode-runner-v1".to_string(),
        };
        self.identity = Some(identity);
        self.vin = Some(vin.clone());
        self.context = Some(context.clone());

        match self.store.load(&vin, &context).await {
            Ok(CapabilityLoad::Hit(cached)) => {
                self.capabilities = cached
                    .records
                    .iter()
                    .map(|record| {
                        (
                            CapabilityKey::new(
                                record.kind,
                                record.request_id.clone(),
                                record.module.clone(),
                            ),
                            record.outcome,
                        )
                    })
                    .fold(CapabilitySet::default(), |mut set, (key, outcome)| {
                        set.insert(key, outcome);
                        set
                    });
                self.snapshot.capability.persistence = CapabilityPersistence::Cached;
                self.snapshot.capability.verification = CapabilityVerification::Ready;
            }
            Ok(CapabilityLoad::Miss | CapabilityLoad::ContextMismatch) => {
                self.snapshot.mode = ModeState::Discovering {
                    origin: super::snapshot::DiscoveryOrigin::Initial,
                    step: 0,
                    total: 1,
                };
                let supported = new_session
                    .session
                    .supported_pids()
                    .await
                    .map_err(|error| anyhow!("supported-PID discovery failed: {error}"))?;
                let mut replacement = CapabilitySetReplacement {
                    vin: vin.clone(),
                    context: context.clone(),
                    completed_at: Utc::now().to_rfc3339(),
                    records: Vec::new(),
                };
                for pid in supported {
                    replacement.records.push(CapabilityRecord {
                        kind: CapabilityKind::Pid,
                        request_id: format!("01{:02X}", pid.0),
                        module: "broadcast".to_string(),
                        outcome: CapabilityOutcome::Supported,
                        observation_seq: 1,
                        rtt_ms: None,
                        attempted_at: replacement.completed_at.clone(),
                        error_code: None,
                    });
                }
                self.capabilities =
                    replacement
                        .records
                        .iter()
                        .fold(CapabilitySet::default(), |mut set, record| {
                            set.insert(
                                CapabilityKey::new(
                                    record.kind,
                                    record.request_id.clone(),
                                    record.module.clone(),
                                ),
                                record.outcome,
                            );
                            set
                        });
                match self.store.replace(&replacement).await {
                    Ok(_) => {
                        self.snapshot.capability.persistence = CapabilityPersistence::Pending;
                    }
                    Err(_) => {
                        self.snapshot.capability.persistence =
                            CapabilityPersistence::SessionOnlyStoreError;
                    }
                }
                self.snapshot.capability.verification = CapabilityVerification::Ready;
            }
            Err(_) => {
                self.capabilities = CapabilitySet::default();
                self.snapshot.capability.persistence = CapabilityPersistence::SessionOnlyStoreError;
                self.snapshot.capability.verification =
                    CapabilityVerification::ConservativeFallback;
            }
        }
        self.session = Some(new_session.session);
        self.snapshot.mode = ModeState::Telemetry;
        self.reconnect_attempt = 0;
        Ok(())
    }

    pub async fn reconnect(&mut self) -> Result<()> {
        self.session = None;
        self.snapshot.mode = ModeState::Reconnecting {
            attempt: self.reconnect_attempt.saturating_add(1),
        };
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        self.connect().await
    }

    pub async fn read_pid(&mut self, pid: Pid) -> Result<f64> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| anyhow!("runner is not connected"))?;
        let reading = session
            .read_pid(pid)
            .await
            .map_err(|error| anyhow!("PID {:02X} failed: {error}", pid.0))?;
        let value = reading
            .value
            .as_f64()
            .map_err(|error| anyhow!("PID {:02X} was not scalar: {error}", pid.0))?;
        self.snapshot.sample_at = Some(Instant::now());
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode_runner::SqliteCapabilityStore;
    use obd2_core::adapter::mock::MockAdapter;
    use obd2_db::Database;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct ScriptedConnector {
        calls: Arc<AtomicUsize>,
        vin: &'static str,
    }

    #[async_trait]
    impl SessionConnector for ScriptedConnector {
        type Adapter = MockAdapter;

        async fn connect(&self) -> std::result::Result<NewSession<Self::Adapter>, ConnectError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(NewSession {
                session: Session::new(MockAdapter::with_vin(self.vin)),
            })
        }
    }

    #[test]
    fn connect_error_has_stable_display() {
        assert_eq!(
            ConnectError::Transport("lost".into()).to_string(),
            "transport: lost"
        );
    }

    #[tokio::test]
    async fn reconnect_drops_session_and_invokes_connector_again() {
        let calls = Arc::new(AtomicUsize::new(0));
        let connector = ScriptedConnector {
            calls: Arc::clone(&calls),
            vin: "1GCHK23224F000001",
        };
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let mut runner = ModeRunner::new(connector, store);
        runner.connect().await.unwrap();
        runner.reconnect().await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
    }
}
