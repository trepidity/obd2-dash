use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::Utc;
use obd2_core::adapter::Adapter;
use obd2_core::error::{NegativeResponse, Obd2Error};
use obd2_core::protocol::pid::Pid;
use obd2_core::session::Session;
use obd2_db::models::{
    CapabilityContext, CapabilityKind, CapabilityLoad, CapabilityOutcome, CapabilityRecord,
    CapabilitySetReplacement,
};

use super::capability::{probe_fingerprint, CapabilityKey, CapabilitySet};
use super::persistence::Persistence;
use super::scheduler::{RequestDescriptor, Scheduler, Tier, ViewId};
use super::snapshot::{CapabilityPersistence, CapabilityVerification, ModeState, RunnerSnapshot};
use super::store::CapabilityStore;
use super::verifier::Verifier;
use crate::profiles::registry::ProfileRegistry;
use crate::profiles::{
    acquire_identity, build_vehicle_context, next_generation, select_into_state, IdentityOutcome,
};
use crate::profiles::{DiagnosticProfile, SignalDisplaySource};
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

fn parse_standard_pid(request_id: &str) -> Result<Pid> {
    u8::from_str_radix(request_id.strip_prefix("01").unwrap_or(""), 16)
        .map(Pid)
        .map_err(|_| anyhow!("invalid standard PID request {request_id}"))
}

fn profile_probe_descriptors(profile: Option<&dyn DiagnosticProfile>) -> Vec<RequestDescriptor> {
    let Some(profile) = profile else {
        return [0x0C, 0x0D, 0x05, 0x0B, 0x10]
            .into_iter()
            .map(standard_pid_descriptor)
            .collect();
    };
    let mut pids = profile.standard_pid_policy().forced.to_vec();
    for display in profile.signal_display() {
        if let SignalDisplaySource::StandardPid(pid) = display.source {
            pids.push(pid);
        }
    }
    pids.sort_unstable();
    pids.dedup();
    pids.into_iter().map(standard_pid_descriptor).collect()
}

/// Standard-PID cadence tiers per the design's §10 telemetry table. Tier C is
/// the low-cadence remainder for secondary PIDs; view gating is reserved for
/// signals with an owning view (Class-2 tables, once seeded) — standard PIDs
/// poll regardless of the active view, so `view` stays `None` here.
fn pid_tier(pid: u8) -> (Tier, u32) {
    match pid {
        // Headline gauges: RPM, speed, MAP/boost, fuel rail actual.
        0x0C | 0x0D | 0x0B | 0x23 => (Tier::A, 1),
        // Secondary engine data: load, coolant, IAT, MAF, baro.
        0x04 | 0x05 | 0x0F | 0x10 | 0x33 => (Tier::B, 5),
        _ => (Tier::C, 20),
    }
}

fn standard_pid_descriptor(pid: u8) -> RequestDescriptor {
    let (tier, every_cycles) = pid_tier(pid);
    RequestDescriptor {
        key: CapabilityKey::new(CapabilityKind::Pid, format!("01{pid:02X}"), "broadcast"),
        tier,
        every_cycles,
        view: None,
    }
}

fn descriptor_for_key(key: CapabilityKey, profile: &[RequestDescriptor]) -> RequestDescriptor {
    if let Some(descriptor) = profile.iter().find(|descriptor| descriptor.key == key) {
        return descriptor.clone();
    }
    let cadence = key
        .request_id
        .strip_prefix("01")
        .and_then(|pid| u8::from_str_radix(pid, 16).ok())
        .map(pid_tier)
        .unwrap_or((Tier::C, 20));
    RequestDescriptor {
        key,
        tier: cadence.0,
        every_cycles: cadence.1,
        view: None,
    }
}

fn probe_error_code(error: super::verifier::ProbeError) -> &'static str {
    match error {
        super::verifier::ProbeError::NoData => "no_data",
        super::verifier::ProbeError::Timeout => "timeout",
        super::verifier::ProbeError::Transport => "transport",
        super::verifier::ProbeError::UnsupportedPid
        | super::verifier::ProbeError::Unsupported
        | super::verifier::ProbeError::ExplicitUnsupported => "unsupported",
        super::verifier::ProbeError::Decode => "decode",
        super::verifier::ProbeError::Stale => "stale",
        super::verifier::ProbeError::Other => "error",
    }
}

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
    scheduler: Scheduler,
    cycle: u64,
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
            scheduler: Scheduler::default(),
            cycle: 0,
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
        self.cycle = 0;
        self.scheduler = Scheduler::default();
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
            self.scheduler = Scheduler::new(
                candidates
                    .iter()
                    .map(|pid| standard_pid_descriptor(pid.0))
                    .collect(),
            );
            self.snapshot.capability.persistence = CapabilityPersistence::SessionOnlyNoVin;
            self.snapshot.capability.verification = CapabilityVerification::Verifying {
                remaining: candidates.len(),
            };
            self.snapshot.mode = ModeState::Telemetry;
            self.publish();
            return Ok(());
        };

        let vehicle_context =
            build_vehicle_context(&new_session.session, next_generation(), &identity);
        let profile_registry = ProfileRegistry::with_builtins();
        let profile_state = select_into_state(&profile_registry, &vehicle_context);
        let profile_id = profile_state
            .selected
            .as_ref()
            .map(|selected| selected.profile_id().as_str())
            .unwrap_or("generic")
            .to_string();
        let selected_profile = profile_state
            .selected
            .as_ref()
            .and_then(|selected| profile_registry.get(selected.profile_id()));
        let profile_descriptors = profile_probe_descriptors(selected_profile);
        let context = CapabilityContext {
            protocol: super::capability::protocol_token(
                new_session.session.adapter_info().protocol,
            )
            .to_string(),
            profile_id,
            probe_schema_version: 1,
            probe_fingerprint: probe_fingerprint(&profile_descriptors),
        };
        // Spec §13: an unfinished verifier pass may resume only when VIN,
        // protocol, profile, and fingerprint all match the previous session.
        // The context covers everything but VIN, which must be compared
        // before it is overwritten — two same-model trucks share a context.
        let same_vehicle = self.vin.as_deref() == Some(vin.as_str());
        let context_unchanged = self.context.as_ref() == Some(&context);
        if !(same_vehicle && context_unchanged && self.verifier.unresolved(&ViewId::Gauges) > 0) {
            self.verifier = Verifier::new();
        }
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
                self.scheduler = Scheduler::new(
                    cached
                        .records
                        .iter()
                        .map(|entry| RequestDescriptor {
                            key: CapabilityKey::new(
                                entry.kind,
                                entry.request_id.clone(),
                                entry.module.clone(),
                            ),
                            tier: Tier::A,
                            every_cycles: 1,
                            view: Some(ViewId::Gauges),
                        })
                        .map(|descriptor| descriptor_for_key(descriptor.key, &profile_descriptors))
                        .collect(),
                );
                for record in &cached.records {
                    if record.outcome == CapabilityOutcome::Unverified {
                        let key = CapabilityKey::new(
                            record.kind,
                            record.request_id.clone(),
                            record.module.clone(),
                        );
                        if !self.verifier.contains(&key) {
                            self.verifier.insert(key, Tier::A, Some(ViewId::Gauges));
                        }
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
                let mut candidates: Vec<Pid> = if fallback || supported.is_empty() {
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
                for descriptor in &profile_descriptors {
                    if let Ok(pid) = parse_standard_pid(&descriptor.key.request_id) {
                        if !candidates.contains(&pid) {
                            candidates.push(pid);
                        }
                    }
                }
                candidates.sort_unstable_by_key(|pid| pid.0);
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
                for record in &replacement.records {
                    let key = CapabilityKey::new(
                        record.kind,
                        record.request_id.clone(),
                        record.module.clone(),
                    );
                    // A preserved (same-vehicle, same-context) entry keeps its
                    // attempt/no-data counters AND its already-classified
                    // outcome; only new keys are staged as unverified.
                    let outcome = if self.verifier.contains(&key) {
                        self.verifier.outcome(&key)
                    } else {
                        self.verifier
                            .insert(key.clone(), Tier::A, Some(ViewId::Gauges));
                        CapabilityOutcome::Unverified
                    };
                    self.capabilities.insert(key, outcome);
                }
                self.scheduler = Scheduler::new(
                    replacement
                        .records
                        .iter()
                        .map(|entry| RequestDescriptor {
                            key: CapabilityKey::new(
                                entry.kind,
                                entry.request_id.clone(),
                                entry.module.clone(),
                            ),
                            tier: Tier::A,
                            every_cycles: 1,
                            view: Some(ViewId::Gauges),
                        })
                        .map(|descriptor| descriptor_for_key(descriptor.key, &profile_descriptors))
                        .collect(),
                );
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

    /// Execute one planned telemetry cycle as an explicit request loop, then
    /// at most one verifier probe (spec §7.2, §9.1.4). Every supported entry
    /// due this cycle is polled; verifier work runs only through
    /// [`Verifier::next`] so retry backoff and NO DATA separation hold.
    pub async fn poll_cycle(&mut self) -> Result<bool> {
        self.cycle = self.cycle.saturating_add(1);
        let plan = self
            .scheduler
            .plan_cycle(self.cycle, &ViewId::Gauges, &self.capabilities);
        let mut did_work = false;

        for request in plan {
            let key = request.0;
            // The plan tail may nominate unverified work; the verifier owns
            // that below, with its own due-time pacing.
            if self.capabilities.outcome(&key) != CapabilityOutcome::Supported {
                continue;
            }
            let pid = parse_standard_pid(&key.request_id)?;
            match self.read_pid_probe(pid).await {
                Ok(value) => {
                    let mut signals = (*self.snapshot.signals).clone();
                    signals.insert(key.request_id.clone(), value);
                    self.snapshot.signals = std::sync::Arc::new(signals);
                    self.snapshot.sample_at = Some(Instant::now());
                    self.publish();
                    did_work = true;
                }
                Err(super::verifier::ProbeError::Transport) => {
                    return Err(anyhow!("telemetry request failed: transport"));
                }
                Err(error) => {
                    // Spec §10: a non-transport failure of a supported request
                    // never becomes Unsupported directly. Demote to Unverified
                    // (retaining the last published value) and let the
                    // verifier reclassify it with backoff; the telemetry
                    // failure itself does not consume a verifier attempt.
                    self.capabilities
                        .insert(key.clone(), CapabilityOutcome::Unverified);
                    if !self.verifier.contains(&key) {
                        self.verifier.insert(key.clone(), Tier::A, None);
                    }
                    if let Some(persistence) = self.persistence.as_mut() {
                        persistence.observe(
                            key,
                            CapabilityOutcome::Unverified,
                            Some(probe_error_code(error).to_string()),
                        );
                        let _ = persistence.flush().await;
                    }
                    self.publish();
                    did_work = true;
                }
            }
        }

        let Some(key) = self.verifier.next(Instant::now(), &ViewId::Gauges).cloned() else {
            return Ok(did_work);
        };
        let pid = parse_standard_pid(&key.request_id)?;
        let result = self.read_pid_probe(pid).await;
        let classification = self.verifier.classify(
            &key,
            result.as_ref().map(|_| ()).map_err(|error| *error),
            Instant::now(),
        );
        if let Some(classification) = classification {
            self.capabilities
                .insert(key.clone(), classification.outcome);
            if let Ok(value) = result {
                // §9.1 step 5: a successful verification publishes its value
                // immediately rather than waiting for the next telemetry pass.
                let mut signals = (*self.snapshot.signals).clone();
                signals.insert(key.request_id.clone(), value);
                self.snapshot.signals = std::sync::Arc::new(signals);
            }
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
        match result {
            Ok(_) => Ok(true),
            Err(super::verifier::ProbeError::Transport) => {
                Err(anyhow!("PID verifier request failed: transport"))
            }
            Err(_) => Ok(true),
        }
    }

    pub async fn read_pid(&mut self, pid: Pid) -> Result<f64> {
        self.read_pid_probe(pid)
            .await
            .map_err(|error| anyhow!("PID {:02X} failed: {error:?}", pid.0))
    }

    async fn read_pid_probe(
        &mut self,
        pid: Pid,
    ) -> std::result::Result<f64, super::verifier::ProbeError> {
        let session = self
            .session
            .as_mut()
            .ok_or(super::verifier::ProbeError::Transport)?;
        let reading = session.read_pid(pid).await.map_err(|error| match error {
            Obd2Error::NoData => super::verifier::ProbeError::NoData,
            Obd2Error::Timeout => super::verifier::ProbeError::Timeout,
            Obd2Error::Transport(_) => super::verifier::ProbeError::Transport,
            Obd2Error::UnsupportedPid { .. } => super::verifier::ProbeError::UnsupportedPid,
            Obd2Error::NegativeResponse {
                nrc:
                    NegativeResponse::RequestOutOfRange
                    | NegativeResponse::ServiceNotSupported
                    | NegativeResponse::SubFunctionNotSupported,
                ..
            } => super::verifier::ProbeError::ExplicitUnsupported,
            _ => super::verifier::ProbeError::Decode,
        })?;
        let value = reading
            .value
            .as_f64()
            .map_err(|_| super::verifier::ProbeError::Decode)?;
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

    #[test]
    fn pid_tiers_match_design_telemetry_table() {
        // Spec §10: headline gauges every cycle; secondary data every 5th.
        for pid in [0x0Cu8, 0x0D, 0x0B, 0x23] {
            assert_eq!(pid_tier(pid), (Tier::A, 1), "PID {pid:02X}");
        }
        for pid in [0x04u8, 0x05, 0x0F, 0x10, 0x33] {
            assert_eq!(pid_tier(pid), (Tier::B, 5), "PID {pid:02X}");
        }
        assert_eq!(pid_tier(0x46), (Tier::C, 20));
        // Standard PIDs are never view-gated; gating is reserved for
        // signals with an owning view.
        assert_eq!(standard_pid_descriptor(0x0C).view, None);
        assert_eq!(standard_pid_descriptor(0x46).view, None);
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
