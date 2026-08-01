use std::collections::HashSet;
use std::time::{Duration, Instant};

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

use super::capability::{default_probe_fingerprint, CapabilityKey, CapabilitySet};
use super::persistence::Persistence;
use super::scheduler::{Tier, ViewId};
use super::snapshot::{CapabilityPersistence, CapabilityVerification, ModeState, RunnerSnapshot};
use super::store::CapabilityStore;
use super::verifier::Verifier;
use crate::profiles::{acquire_identity, IdentityOutcome};
use tokio::sync::watch;

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
    persistence: Option<Persistence<S>>,
    verifier: Verifier,
    snapshot: RunnerSnapshot,
    reconnect_attempt: u32,
    snapshot_tx: watch::Sender<RunnerSnapshot>,
}

impl<C, S> ModeRunner<C, S>
where
    C: SessionConnector,
    S: CapabilityStore + Clone,
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
            persistence: None,
            verifier: Verifier::new(),
            snapshot: RunnerSnapshot::empty(),
            reconnect_attempt: 0,
            snapshot_tx: watch::channel(RunnerSnapshot::empty()).0,
        }
    }

    pub fn snapshot(&self) -> RunnerSnapshot {
        self.snapshot.clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<RunnerSnapshot> {
        self.snapshot_tx.subscribe()
    }

    fn publish(&self) {
        let _ = self.snapshot_tx.send(self.snapshot.clone());
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
            let candidates = match new_session.session.supported_pids().await {
                Ok(pids) if !pids.is_empty() => pids.into_iter().collect::<Vec<_>>(),
                _ => vec![
                    Pid::ENGINE_RPM,
                    Pid::VEHICLE_SPEED,
                    Pid::COOLANT_TEMP,
                    Pid::INTAKE_MAP,
                    Pid::MAF,
                ],
            };
            self.session = Some(new_session.session);
            self.identity = Some(identity);
            self.capabilities = CapabilitySet::default();
            self.verifier = Verifier::new();
            for pid in &candidates {
                let key = CapabilityKey::new(
                    CapabilityKind::Pid,
                    format!("01{:02X}", pid.0),
                    "broadcast",
                );
                self.capabilities
                    .insert(key.clone(), CapabilityOutcome::Unverified);
                self.verifier.insert(key, Tier::A, Some(ViewId::Gauges));
            }
            self.snapshot.capability.persistence = CapabilityPersistence::SessionOnlyNoVin;
            self.snapshot.capability.verification = CapabilityVerification::Verifying {
                remaining: candidates.len(),
            };
            self.snapshot.mode = ModeState::Telemetry;
            self.publish();
            return Ok(());
        };

        let context = CapabilityContext {
            protocol: super::capability::protocol_token(
                new_session.session.adapter_info().protocol,
            )
            .to_string(),
            profile_id: "generic".to_string(),
            probe_schema_version: 1,
            probe_fingerprint: default_probe_fingerprint(),
        };
        self.identity = Some(identity);
        self.vin = Some(vin.clone());
        self.context = Some(context.clone());
        self.persistence = Some(Persistence::new(
            self.store.clone(),
            vin.clone(),
            context.clone(),
        ));

        match self.store.load(&vin, &context).await {
            Ok(CapabilityLoad::Hit(cached)) => {
                let max_sequence = cached
                    .records
                    .iter()
                    .map(|record| record.observation_seq)
                    .max()
                    .unwrap_or(0);
                if let Some(persistence) = self.persistence.as_mut() {
                    persistence.adopt_loaded_set(cached.set_id.clone(), max_sequence);
                }
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
                for record in &cached.records {
                    if record.outcome == CapabilityOutcome::Unverified {
                        self.verifier.insert(
                            CapabilityKey::new(
                                record.kind,
                                record.request_id.clone(),
                                record.module.clone(),
                            ),
                            Tier::A,
                            Some(ViewId::Gauges),
                        );
                    }
                }
                self.snapshot.capability.persistence = CapabilityPersistence::Cached;
                self.snapshot.capability.verification = CapabilityVerification::Ready;
            }
            Ok(CapabilityLoad::Miss | CapabilityLoad::ContextMismatch) => {
                self.snapshot.mode = ModeState::Discovering {
                    origin: super::snapshot::DiscoveryOrigin::Initial,
                    step: 0,
                    total: 1,
                };
                let (supported, fallback) = match new_session.session.supported_pids().await {
                    Ok(pids) => (pids, false),
                    Err(error) => {
                        tracing::warn!(
                            "supported-PID discovery failed; using conservative fallback: {error}"
                        );
                        (HashSet::new(), true)
                    }
                };
                let candidates: Vec<Pid> = if fallback || supported.is_empty() {
                    [
                        Pid::ENGINE_RPM,
                        Pid::VEHICLE_SPEED,
                        Pid::COOLANT_TEMP,
                        Pid::INTAKE_MAP,
                        Pid::MAF,
                    ]
                    .into_iter()
                    .collect()
                } else {
                    supported.into_iter().collect()
                };
                let mut replacement = CapabilitySetReplacement {
                    vin: vin.clone(),
                    context: context.clone(),
                    completed_at: Utc::now().to_rfc3339(),
                    records: Vec::new(),
                };
                for pid in candidates {
                    replacement.records.push(CapabilityRecord {
                        kind: CapabilityKind::Pid,
                        request_id: format!("01{:02X}", pid.0),
                        module: "broadcast".to_string(),
                        outcome: CapabilityOutcome::Unverified,
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
                self.verifier = Verifier::new();
                for record in &replacement.records {
                    let key = CapabilityKey::new(
                        record.kind,
                        record.request_id.clone(),
                        record.module.clone(),
                    );
                    self.capabilities
                        .insert(key.clone(), CapabilityOutcome::Unverified);
                    self.verifier.insert(key, Tier::A, Some(ViewId::Gauges));
                }
                self.snapshot.capability.persistence = CapabilityPersistence::Pending;
                self.snapshot.capability.verification = CapabilityVerification::Verifying {
                    remaining: replacement.records.len(),
                };
                // The staged set is persisted only after verification completes.
                let _ = replacement;
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
        self.publish();
        self.reconnect_attempt = 0;
        Ok(())
    }

    pub async fn reconnect(&mut self) -> Result<()> {
        self.session = None;
        self.snapshot.mode = ModeState::Reconnecting {
            attempt: self.reconnect_attempt.saturating_add(1),
        };
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        let delay = if self.reconnect_attempt <= 3 {
            Duration::from_millis(100 * u64::from(self.reconnect_attempt))
        } else {
            Duration::from_secs(2)
        };
        tokio::time::sleep(delay).await;
        self.connect().await
    }

    /// Execute one bounded telemetry/verifier request and publish its result.
    pub async fn poll_cycle(&mut self) -> Result<bool> {
        let Some(key) = self.verifier.next(Instant::now(), &ViewId::Gauges).cloned() else {
            return Ok(false);
        };
        let pid = u8::from_str_radix(key.request_id.strip_prefix("01").unwrap_or(""), 16)
            .map(Pid)
            .map_err(|_| anyhow!("invalid verifier request {}", key.request_id))?;
        let result = self.read_pid(pid).await;
        let classification = self.verifier.classify(
            &key,
            result
                .as_ref()
                .map(|_| ())
                .map_err(|_| super::verifier::ProbeError::Decode),
            Instant::now(),
        );
        if let Some(classification) = classification {
            self.capabilities
                .insert(key.clone(), classification.outcome);
            if let Some(persistence) = self.persistence.as_mut() {
                persistence.observe(
                    key,
                    classification.outcome,
                    classification.error_code.map(str::to_string),
                );
                let _ = persistence.flush().await;
            }
            let unresolved = self.verifier.unresolved(&ViewId::Gauges);
            if unresolved == 0 {
                // Verification pass complete (§9.1 step 7): persist the full
                // set atomically once, then report Ready or Degraded depending
                // on whether exhausted entries stayed unverified.
                if let Some(persistence) = self.persistence.as_mut() {
                    if persistence.set_id().is_none() {
                        match persistence.replace_from_outcomes(&self.capabilities).await {
                            Ok(_) => {
                                self.snapshot.capability.persistence =
                                    CapabilityPersistence::Cached;
                            }
                            Err(error) => {
                                tracing::warn!("capability set persistence failed: {error}");
                                self.snapshot.capability.persistence =
                                    CapabilityPersistence::SessionOnlyStoreError;
                            }
                        }
                    }
                }
                let leftover = self.verifier.remaining(&ViewId::Gauges);
                self.snapshot.capability.verification = if leftover == 0 {
                    CapabilityVerification::Ready
                } else {
                    CapabilityVerification::Degraded {
                        unresolved: leftover,
                    }
                };
            } else {
                self.snapshot.capability.verification = CapabilityVerification::Verifying {
                    remaining: unresolved,
                };
            }
            self.publish();
        }
        result.map(|_| true)
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
