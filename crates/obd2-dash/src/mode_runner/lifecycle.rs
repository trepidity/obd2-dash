use std::collections::{BTreeSet, HashSet};
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

use super::capability::{probe_fingerprint, protocol_display, CapabilityKey, CapabilitySet};
use super::command::{
    reply_for, CommandReply, ControlCommand, ControlInput, RunnerCommand, RunnerControlReceiver,
};
use super::diagnostic::{
    capability_outcome, execute_freeze_frame_pid, execute_profile_dtc_results,
    execute_session_request, expand_dtc_requests, profile_dtc_outcome_request, request_plan,
    resolve_fuel, DiagnosticPhase, DiagnosticRequest, RequestTarget, ServiceGates, StepErrorKind,
    StepResult,
};
use super::persistence::Persistence;
use super::scheduler::{RequestDescriptor, Scheduler, Tier, ViewId};
use super::snapshot::{
    CapabilityPersistence, CapabilityVerification, ConnectionMetadata, DiagnosticResult, ModeState,
    RunnerSnapshot, VinSource,
};
use super::store::CapabilityStore;
use super::verifier::Verifier;
use crate::profiles::registry::ProfileRegistry;
use crate::profiles::{
    acquire_identity, build_vehicle_context, next_generation, select_into_state, IdentityOutcome,
};
use crate::profiles::{
    CapabilityId as ProfileCapabilityId, Confidence, DiagnosticProfile, FailurePolicy,
    IdentityConfidence, NullEvidenceSink, PollCadence, ProfileId, ProfileResponse, ProfileRuntime,
    RequestId as ProfileRequestId, SelectedProfile, SignalCategory, SignalDisplaySource,
    VehicleContext,
};
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

const PROFILE_FAST_SIGNAL_INTERVAL_CYCLES: u64 = 10;
const PROFILE_MEDIUM_SIGNAL_INTERVAL_CYCLES: u64 = 20;
/// Poll exactly one Fuel profile signal per interval. A complete LLY fuel
/// sweep is nine requests; issuing that whole sweep together can starve the
/// J1850 telemetry scheduler and make otherwise-working gauges appear dead.
const PROFILE_FUEL_SIGNAL_INTERVAL_CYCLES: u64 = 4;
const PROFILE_SLOW_SIGNAL_INTERVAL_CYCLES: u64 = 60;
/// A transient Mode 09 failure must not strand a live vehicle in the generic
/// profile for the rest of the session. Retry one read at a low cadence while
/// VIN is absent; unlike startup acquisition this is deliberately a single
/// request so identity recovery cannot monopolize a slow J1850 bus.
const MISSING_IDENTITY_RETRY_INTERVAL_CYCLES: u64 = 20;
/// A connected adapter is not proof of a connected vehicle. If every ECU
/// request stops producing observations for this long, drop the Session and
/// let the runner's bounded reconnect loop re-establish the bus.
const TELEMETRY_STALE_AFTER: Duration = Duration::from_secs(3);

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

/// Operator-supplied identity used only when a live Mode 09 read is unread.
/// The selected profile must still return at least a partial match before the
/// registry seals it as a manual confirmation.
#[derive(Debug, Clone)]
pub struct ManualProfileConfirmation {
    vin: String,
    profile_id: ProfileId,
}

impl ManualProfileConfirmation {
    pub fn new(vin: impl Into<String>, profile_id: ProfileId) -> Result<Self> {
        let vin = vin.into().trim().to_ascii_uppercase();
        if !crate::profiles::validate_vin_charset(&vin) {
            return Err(anyhow!(
                "manual profile confirmation requires a 17-character VIN"
            ));
        }
        Ok(Self { vin, profile_id })
    }
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
    telemetry_started_at: Option<Instant>,
    telemetry_stale_after: Duration,
    profile_descriptors: Vec<RequestDescriptor>,
    forced_standard_keys: BTreeSet<CapabilityKey>,
    profile_context: Option<VehicleContext>,
    selected_profile: Option<SelectedProfile>,
    manual_profile_confirmation: Option<ManualProfileConfirmation>,
    /// A VIN read during a rescan remains trustworthy for the immediately
    /// following reconnect, even if the adapter's second Mode 09 attempt is
    /// transiently unreadable. It is consumed exactly once by `connect`.
    rescan_identity: Option<IdentityOutcome>,
    foreground_cancel_requested: bool,
    diagnostic_no_data: BTreeSet<CapabilityKey>,
    control: Option<RunnerControlReceiver>,
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
            telemetry_started_at: None,
            telemetry_stale_after: TELEMETRY_STALE_AFTER,
            profile_descriptors: Vec::new(),
            forced_standard_keys: BTreeSet::new(),
            profile_context: None,
            selected_profile: None,
            manual_profile_confirmation: None,
            rescan_identity: None,
            foreground_cancel_requested: false,
            diagnostic_no_data: BTreeSet::new(),
            control: None,
        }
    }

    pub fn snapshot(&self) -> RunnerSnapshot {
        self.snapshot.clone()
    }

    pub fn with_manual_profile_confirmation(
        mut self,
        confirmation: ManualProfileConfirmation,
    ) -> Self {
        self.manual_profile_confirmation = Some(confirmation);
        self
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

    /// Attach the bounded control receiver owned by the runner task. The
    /// producer is kept by GUI/TUI command surfaces; lifecycle polls this only
    /// at serial request boundaries.
    pub fn attach_control(&mut self, control: RunnerControlReceiver) {
        self.control = Some(control);
    }

    /// Apply queued controls at a request boundary. A closed producer follows
    /// the same orderly cleanup as Shutdown and cannot trigger reconnect.
    pub async fn process_control_boundary(&mut self) -> Result<()> {
        loop {
            let input = self
                .control
                .as_mut()
                .and_then(RunnerControlReceiver::try_recv);
            let Some(input) = input else { return Ok(()) };
            match input {
                ControlInput::Closed => return self.shutdown().await,
                ControlInput::Command(queued) => {
                    let (command, acknowledgement) = queued.into_parts();
                    let reply = command.reply_for(&self.snapshot.mode);
                    if reply != CommandReply::Accepted {
                        let _ = acknowledgement.send(reply);
                        continue;
                    }
                    match command {
                        ControlCommand::RunDiagnostic => {
                            let _ = self.command(RunnerCommand::RunDiagnostic);
                            let _ = acknowledgement.send(CommandReply::Accepted);
                        }
                        ControlCommand::RescanVehicle => {
                            let _ = self.command(RunnerCommand::RescanVehicle);
                            let _ = acknowledgement.send(CommandReply::Accepted);
                        }
                        ControlCommand::CancelForeground => {
                            let _ = self.command(RunnerCommand::CancelForeground);
                            let _ = acknowledgement.send(CommandReply::Accepted);
                        }
                        ControlCommand::RequestActiveTest(command) => {
                            let execution =
                                super::diagnostic::execute_locked_active_test(command).await;
                            if let Some(error) = execution.evidence_write_error {
                                tracing::warn!("{error}");
                            }
                            tracing::info!(
                                result = ?execution.result,
                                evidence = ?execution.profile_evidence,
                                "locked active-test request recorded"
                            );
                            let _ = acknowledgement.send(CommandReply::Accepted);
                        }
                        ControlCommand::Shutdown => {
                            // Ack only once the Session is dropped and the
                            // latest accepted persistence batch is flushed.
                            self.shutdown().await?;
                            let _ = acknowledgement.send(CommandReply::Accepted);
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    /// Apply one bounded foreground command. Rejected commands are never
    /// retained for later execution; accepted work owns the mode transition.
    pub fn command(&mut self, command: RunnerCommand) -> CommandReply {
        let reply = reply_for(command, &self.snapshot.mode);
        if reply != CommandReply::Accepted {
            return reply;
        }
        match command {
            RunnerCommand::RunDiagnostic => {
                self.snapshot.mode = ModeState::Diagnostic {
                    phase: 0,
                    phase_total: 5,
                    step: 0,
                    total: 0,
                };
            }
            RunnerCommand::RescanVehicle => {
                self.snapshot.mode = ModeState::Discovering {
                    origin: super::snapshot::DiscoveryOrigin::Rescan,
                    step: 0,
                    total: 1,
                };
            }
            RunnerCommand::CancelForeground => {
                // The serial request currently in flight must run to
                // completion. `run_foreground` observes this flag at the next
                // request boundary and then returns to Telemetry.
                self.foreground_cancel_requested = true;
            }
            RunnerCommand::Shutdown => {
                self.snapshot.mode = ModeState::ShuttingDown;
            }
        }
        self.publish();
        reply
    }

    pub async fn connect(&mut self) -> Result<()> {
        self.snapshot.mode = ModeState::Connecting;
        self.snapshot.connection = ConnectionMetadata::default();
        self.snapshot.adapter_voltage = None;
        self.snapshot.signals = std::sync::Arc::new(Default::default());
        self.snapshot.diagnostic = std::sync::Arc::new(DiagnosticResult::default());
        self.snapshot.sample_at = None;
        self.telemetry_started_at = None;
        self.cycle = 0;
        self.scheduler = Scheduler::default();
        self.profile_descriptors = profile_probe_descriptors(None);
        self.forced_standard_keys.clear();
        self.profile_context = None;
        self.selected_profile = None;
        self.snapshot.selected_profile = None;
        self.snapshot.profile_manually_confirmed = false;
        self.publish();
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

        self.snapshot.connection.protocol =
            Some(protocol_display(new_session.session.adapter_info().protocol).to_string());
        // `ATRV` measures adapter supply voltage. It is intentionally kept
        // separate from PID 01 42, which this vehicle has not advertised.
        self.snapshot.adapter_voltage = new_session.session.battery_voltage().await.ok().flatten();

        let observed_identity = acquire_identity(&mut new_session.session, 2).await;
        let rescan_identity = self.rescan_identity.take();
        let (identity, vin_source) = match (
            &observed_identity.vin,
            rescan_identity,
            &self.manual_profile_confirmation,
        ) {
            (Some(_), _, _) => (observed_identity, Some(VinSource::Observed)),
            (None, Some(identity), _) => (identity, Some(VinSource::Observed)),
            (None, None, Some(confirmation)) => (
                IdentityOutcome {
                    vin: Some(confirmation.vin.clone()),
                    confidence: IdentityConfidence::Single,
                },
                Some(VinSource::Manual),
            ),
            (None, None, None) => (observed_identity, None),
        };
        self.snapshot.connection.vin = identity.vin.clone();
        self.snapshot.connection.vin_source = vin_source;
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
            self.enter_telemetry();
            return Ok(());
        };

        let vehicle_context =
            build_vehicle_context(&new_session.session, next_generation(), &identity);
        let profile_registry = ProfileRegistry::with_builtins();
        let mut profile_state = select_into_state(&profile_registry, &vehicle_context);
        if let Some(confirmation) = &self.manual_profile_confirmation {
            if identity.vin.as_deref() == Some(confirmation.vin.as_str()) {
                profile_state.selected = Some(
                    profile_registry
                        .confirm_manual(&vehicle_context, confirmation.profile_id)
                        .map_err(|error| {
                            anyhow!("manual profile confirmation rejected: {error}")
                        })?,
                );
            }
        }
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
        self.forced_standard_keys = selected_profile
            .map(|profile| {
                profile
                    .standard_pid_policy()
                    .forced
                    .iter()
                    .map(|pid| standard_pid_descriptor(*pid).key)
                    .collect()
            })
            .unwrap_or_default();
        self.profile_descriptors = profile_descriptors.clone();
        self.profile_context = Some(vehicle_context.clone());
        self.selected_profile = profile_state.selected.clone();
        self.snapshot.selected_profile = self
            .selected_profile
            .as_ref()
            .map(SelectedProfile::profile_id);
        self.snapshot.profile_manually_confirmed = self
            .selected_profile
            .as_ref()
            .is_some_and(SelectedProfile::manual_confirmed);
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
                        // Diagnostic service outcomes share the capability
                        // store with PID outcomes, but only PIDs belong to
                        // the live telemetry scheduler.  Scheduling a cached
                        // service (for example "01") as a PID turns it into
                        // an invalid request id and forces a reconnect loop.
                        .filter(|entry| entry.kind == CapabilityKind::Pid)
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
                    if record.kind != CapabilityKind::Pid {
                        continue;
                    }
                    let key = CapabilityKey::new(
                        record.kind,
                        record.request_id.clone(),
                        record.module.clone(),
                    );
                    // A forced PID is part of the selected vehicle profile's
                    // minimum live-data contract.  A previous transient NO
                    // DATA must not suppress it indefinitely: re-verify an
                    // old cached Unsupported result on each fresh session.
                    // This matters on Class-2 where the initial supported-PID
                    // sweep can be incomplete while the engine is waking up.
                    let needs_forced_recheck = record.outcome == CapabilityOutcome::Unsupported
                        && self.forced_standard_keys.contains(&key);
                    if record.outcome == CapabilityOutcome::Unverified || needs_forced_recheck {
                        if needs_forced_recheck {
                            self.capabilities
                                .insert(key.clone(), CapabilityOutcome::Unverified);
                        }
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
        self.enter_telemetry();
        self.reconnect_attempt = 0;
        Ok(())
    }

    pub async fn reconnect(&mut self) -> Result<()> {
        self.session = None;
        self.snapshot.mode = ModeState::Reconnecting {
            attempt: self.reconnect_attempt.saturating_add(1),
        };
        self.telemetry_started_at = None;
        self.reconnect_attempt = self.reconnect_attempt.saturating_add(1);
        self.publish();
        let delay = if self.reconnect_attempt <= 3 {
            Duration::from_millis(100 * u64::from(self.reconnect_attempt))
        } else {
            Duration::from_secs(2)
        };
        tokio::time::sleep(delay).await;
        self.connect().await
    }

    /// Drive the accepted foreground operation.  The future owns the Session
    /// for the duration of each request; cancellation is sampled only after a
    /// completed request, never by dropping a serial future.
    pub async fn run_foreground(&mut self) -> Result<()> {
        match self.snapshot.mode {
            ModeState::Diagnostic { .. } => self.run_diagnostic().await,
            ModeState::Discovering {
                origin: super::snapshot::DiscoveryOrigin::Rescan,
                ..
            } => self.run_rescan().await,
            ModeState::ShuttingDown => self.shutdown().await,
            _ => Ok(()),
        }
    }

    /// Drive one runner iteration for callers that use the bounded control
    /// plane. A foreground command accepted at the preceding boundary is
    /// executed by this same Session owner; it is never handed back to a GUI
    /// handler or a second task.
    pub async fn run_once(&mut self) -> Result<bool> {
        self.process_control_boundary().await?;
        match self.snapshot.mode {
            ModeState::Diagnostic { .. }
            | ModeState::Discovering {
                origin: super::snapshot::DiscoveryOrigin::Rescan,
                ..
            }
            | ModeState::ShuttingDown => {
                self.run_foreground().await?;
                Ok(true)
            }
            ModeState::Telemetry => self.poll_cycle().await,
            ModeState::Connecting
            | ModeState::Reconnecting { .. }
            | ModeState::Discovering { .. } => Ok(false),
        }
    }

    /// Complete a requested shutdown after the current request boundary.
    /// Releasing the Session precedes the final SQLite flush so the serial
    /// port is not retained while storage is slow.
    pub async fn shutdown(&mut self) -> Result<()> {
        self.snapshot.mode = ModeState::ShuttingDown;
        self.session = None;
        if let Some(persistence) = self.persistence.as_mut() {
            persistence.flush().await?;
        }
        self.publish();
        Ok(())
    }

    async fn run_diagnostic(&mut self) -> Result<()> {
        let (protocol, session_fuel, spec, modules) = {
            let session = self
                .session
                .as_ref()
                .ok_or_else(|| anyhow!("diagnostic requested without an active session"))?;
            let mut modules = session
                .discovery()
                .map(|discovery| {
                    discovery
                        .modules
                        .iter()
                        .filter_map(|(id, resolved)| {
                            let active = discovery.active_bus.as_ref();
                            active
                                .is_none_or(|bus| resolved.bus == bus.id)
                                .then(|| id.clone())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            modules.sort_by(|left, right| left.0.cmp(&right.0));
            (
                session.adapter_info().protocol,
                session
                    .spec()
                    .map(|spec| spec.identity.engine.fuel_type.clone()),
                session.spec().cloned(),
                modules,
            )
        };
        let database_fuel = match self.vin.as_deref() {
            Some(vin) => self
                .store
                .load_exact_vehicle_fuel_type(vin)
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let gates = ServiceGates {
            cached_mode05_unsupported: self.service_is_unsupported(0x05),
            cached_readiness_unsupported: self.service_is_unsupported(0x01),
            // Exact id: a substring test would also match unrelated
            // profiles ("rally" contains "lly"), and the durable diesel
            // guarantee is the fuel class — this flag is belt-and-suspenders
            // for exactly one known profile.
            is_lly_profile: self
                .context
                .as_ref()
                .is_some_and(|context| context.profile_id == "gm.gmt800.lly.class2"),
        };
        let plan = request_plan(
            resolve_fuel(session_fuel.as_deref(), database_fuel.as_deref()),
            protocol,
            gates,
        );
        let mut requests = Vec::new();
        let module_names = modules
            .iter()
            .map(|module| module.0.clone())
            .collect::<Vec<_>>();
        for request in plan {
            match request.phase {
                DiagnosticPhase::Dtc if request.target == RequestTarget::Broadcast => {
                    // The DTC expansion contains the broadcast trio once and
                    // then concrete module-major triples.
                    requests.extend(expand_dtc_requests(&module_names));
                }
                DiagnosticPhase::Dtc => {}
                DiagnosticPhase::FreezeFrames => {
                    // The detailed frame count is known only after decoding
                    // DTCs. The runner currently has no decoded DTC payload
                    // sink, so an empty-code scan correctly skips this phase.
                }
                DiagnosticPhase::ModuleRefresh
                    if request.target == RequestTarget::DiscoveredModules =>
                {
                    for index in 0..modules.len() {
                        requests.push(DiagnosticRequest {
                            target: RequestTarget::Module(index),
                            ..request
                        });
                    }
                    if modules.is_empty() {
                        requests.push(DiagnosticRequest {
                            target: RequestTarget::Broadcast,
                            ..request
                        });
                    }
                }
                _ => requests.push(request),
            }
        }

        let total = requests.len() as u32;
        let mut profile_dtc_ran = false;
        let mut diagnostic = DiagnosticResult::default();
        self.snapshot.diagnostic = std::sync::Arc::new(diagnostic.clone());
        for (step, request) in requests.into_iter().enumerate() {
            self.process_control_boundary().await?;
            if self.foreground_cancel_requested {
                self.finish_foreground();
                return Ok(());
            }
            if !profile_dtc_ran && request.phase != DiagnosticPhase::Dtc {
                self.run_profile_dtc(&mut diagnostic).await?;
                self.run_freeze_frames(&mut diagnostic).await?;
                profile_dtc_ran = true;
            }
            self.snapshot.mode = ModeState::Diagnostic {
                phase: request.phase as u8,
                phase_total: 5,
                step: step as u32,
                total,
            };
            self.publish();
            let result = {
                let session = self
                    .session
                    .as_mut()
                    .ok_or_else(|| anyhow!("diagnostic session was released"))?;
                execute_session_request(session, &self.snapshot.mode, request, &modules).await
            };
            match result {
                Ok(result) => {
                    diagnostic.record_standard_dtc_payload(
                        request,
                        &result,
                        &modules,
                        spec.as_ref(),
                    );
                    self.record_diagnostic_outcome(request, &result, &modules);
                }
                Err(error) => {
                    self.session = None;
                    self.snapshot.mode = ModeState::Reconnecting {
                        attempt: self.reconnect_attempt.saturating_add(1),
                    };
                    self.publish();
                    return Err(error);
                }
            }
        }
        if !profile_dtc_ran {
            self.run_profile_dtc(&mut diagnostic).await?;
            self.run_freeze_frames(&mut diagnostic).await?;
        }
        if let Some(persistence) = self.persistence.as_mut() {
            let _ = persistence.flush().await;
        }
        diagnostic.completed = true;
        self.snapshot.diagnostic = std::sync::Arc::new(diagnostic);
        self.finish_foreground();
        Ok(())
    }

    async fn run_profile_dtc(&mut self, diagnostic: &mut DiagnosticResult) -> Result<()> {
        let (Some(context), Some(selected)) =
            (self.profile_context.clone(), self.selected_profile.clone())
        else {
            return Ok(());
        };
        let profile_results = {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| anyhow!("diagnostic session was released"))?;
            execute_profile_dtc_results(session, &context, &selected).await
        };
        for result in profile_results {
            match result {
                Ok(result) => {
                    // Profile DTC services share phase one but own their
                    // routing inside ProfileRuntime.
                    diagnostic.record_profile_dtcs(selected.profile_id().as_str(), result.dtcs);
                    diagnostic.profile_evidence.extend(result.evidence);
                    self.record_diagnostic_outcome(
                        profile_dtc_outcome_request(),
                        &result.result,
                        &[],
                    );
                }
                Err(error) => {
                    self.session = None;
                    self.snapshot.mode = ModeState::Reconnecting {
                        attempt: self.reconnect_attempt.saturating_add(1),
                    };
                    self.publish();
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    async fn run_freeze_frames(&mut self, diagnostic: &mut DiagnosticResult) -> Result<()> {
        let spec = self
            .session
            .as_ref()
            .and_then(|session| session.spec().cloned());
        for work in diagnostic.freeze_frame_work(spec.as_ref()) {
            for pid in &work.pids {
                self.process_control_boundary().await?;
                if self.foreground_cancel_requested {
                    return Ok(());
                }
                let reading = {
                    let session = self
                        .session
                        .as_mut()
                        .ok_or_else(|| anyhow!("diagnostic session was released"))?;
                    execute_freeze_frame_pid(session, *pid).await
                }?;
                diagnostic.record_freeze_frame_reading(&work, reading);
            }
        }
        Ok(())
    }

    async fn run_rescan(&mut self) -> Result<()> {
        let total = 4u32
            .saturating_add(self.profile_descriptors.len() as u32)
            .saturating_add(1);
        self.snapshot.mode = ModeState::Discovering {
            origin: super::snapshot::DiscoveryOrigin::Rescan,
            step: 0,
            total,
        };
        self.publish();
        if self.foreground_cancel_requested {
            self.finish_foreground();
            return Ok(());
        }

        // A successful VIN read after generic fallback changes both the
        // profile and the probe schema. Reopen the session so `connect` can
        // atomically select that profile and load its matching capability
        // context. Preserve this just-read identity for that one reconnect:
        // Mode 09 is known to be intermittently unreadable on this VPW truck.
        let refreshed_identity = {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| anyhow!("rescan requested without an active session"))?;
            acquire_identity(session, 2).await
        };
        if refreshed_identity.vin.is_some()
            && refreshed_identity.vin.as_deref() != self.vin.as_deref()
        {
            self.rescan_identity = Some(refreshed_identity);
            self.session = None;
            return self.connect().await;
        }

        let supported = match self
            .session
            .as_mut()
            .ok_or_else(|| anyhow!("rescan requested without an active session"))?
            .refresh_supported_pids()
            .await
        {
            Ok(pids) => pids,
            Err(error) => {
                self.finish_foreground();
                return Err(anyhow!("forced supported-PID refresh failed: {error}"));
            }
        };
        self.snapshot.mode = ModeState::Discovering {
            origin: super::snapshot::DiscoveryOrigin::Rescan,
            step: 4,
            total,
        };
        self.publish();
        let mut staged = CapabilitySet::default();
        for (index, descriptor) in self.profile_descriptors.clone().into_iter().enumerate() {
            if self.foreground_cancel_requested {
                self.finish_foreground();
                return Ok(());
            }
            let pid = parse_standard_pid(&descriptor.key.request_id)?;
            let forced = self.forced_standard_keys.contains(&descriptor.key);
            let initial = if supported.contains(&pid) || forced {
                CapabilityOutcome::Unverified
            } else {
                CapabilityOutcome::Unsupported
            };
            let outcome = if initial == CapabilityOutcome::Unverified {
                match self.read_pid_probe(pid).await {
                    Ok(value) => {
                        let mut signals = (*self.snapshot.signals).clone();
                        signals.insert(descriptor.key.request_id.clone(), value);
                        self.snapshot.signals = std::sync::Arc::new(signals);
                        CapabilityOutcome::Supported
                    }
                    Err(super::verifier::ProbeError::UnsupportedPid)
                    | Err(super::verifier::ProbeError::ExplicitUnsupported) => {
                        CapabilityOutcome::Unsupported
                    }
                    Err(_) => CapabilityOutcome::Unverified,
                }
            } else {
                initial
            };
            staged.insert(descriptor.key, outcome);
            self.snapshot.mode = ModeState::Discovering {
                origin: super::snapshot::DiscoveryOrigin::Rescan,
                step: 4 + index as u32 + 1,
                total,
            };
            self.publish();
        }
        if self.foreground_cancel_requested {
            self.finish_foreground();
            return Ok(());
        }
        if let Some(persistence) = self.persistence.as_mut() {
            if let Err(error) = persistence.replace_from_outcomes(&staged).await {
                // The staged map never becomes active until the replacement
                // transaction commits.  A store failure therefore resumes
                // the previous in-memory/SQLite capability generation.
                self.finish_foreground();
                return Err(error);
            }
            self.snapshot.capability.persistence = CapabilityPersistence::Cached;
        }
        self.capabilities = staged;
        self.rebuild_verifier_after_rescan();
        self.snapshot.capability.verification = CapabilityVerification::Ready;
        self.finish_foreground();
        Ok(())
    }

    fn service_is_unsupported(&self, service: u8) -> bool {
        self.capabilities.outcome(&CapabilityKey::new(
            CapabilityKind::Service,
            format!("{service:02X}"),
            "broadcast",
        )) == CapabilityOutcome::Unsupported
    }

    fn record_diagnostic_outcome(
        &mut self,
        request: DiagnosticRequest,
        result: &StepResult,
        modules: &[obd2_core::vehicle::ModuleId],
    ) {
        // Spec §8.1: module values are canonical module ids or the fixed
        // sentinels — never session-local indices, which reorder between
        // sessions and would attach cached outcomes to the wrong module.
        let module = match request.target {
            RequestTarget::Module(index) => match modules.get(index) {
                Some(module) => module.0.clone(),
                None => {
                    tracing::warn!("dropping diagnostic outcome for stale module index {index}");
                    return;
                }
            },
            _ => "broadcast".to_string(),
        };
        let key = CapabilityKey::new(
            CapabilityKind::Service,
            format!("{:02X}", request.service),
            module,
        );
        let prior_no_data = self.diagnostic_no_data.contains(&key);
        let outcome = capability_outcome(result, prior_no_data);
        match result {
            StepResult::StepError {
                kind: StepErrorKind::NoData,
                ..
            } => {
                self.diagnostic_no_data.insert(key.clone());
            }
            _ => {
                self.diagnostic_no_data.remove(&key);
            }
        }
        self.capabilities.insert(key.clone(), outcome);
        if let Some(persistence) = self.persistence.as_mut() {
            let error_code = match result {
                StepResult::Data(_) => None,
                StepResult::StepError {
                    kind: StepErrorKind::NoData,
                    ..
                } => Some("no_data".into()),
                StepResult::StepError {
                    kind: StepErrorKind::Unsupported,
                    ..
                } => Some("unsupported".into()),
                StepResult::StepError {
                    kind: StepErrorKind::Other,
                    ..
                } => Some("error".into()),
            };
            persistence.observe(key, outcome, error_code);
        }
    }

    fn rebuild_verifier_after_rescan(&mut self) {
        self.verifier = Verifier::new();
        for descriptor in &self.profile_descriptors {
            if self.capabilities.outcome(&descriptor.key) == CapabilityOutcome::Unverified {
                self.verifier.insert(
                    descriptor.key.clone(),
                    descriptor.tier,
                    descriptor.view.clone(),
                );
            }
        }
    }

    fn finish_foreground(&mut self) {
        self.foreground_cancel_requested = false;
        self.enter_telemetry();
    }

    fn enter_telemetry(&mut self) {
        self.telemetry_started_at = Some(Instant::now());
        self.snapshot.mode = ModeState::Telemetry;
        self.publish();
    }

    fn finish_poll_cycle(&self, did_work: bool) -> Result<bool> {
        let freshness_reference = match (self.telemetry_started_at, self.snapshot.sample_at) {
            (Some(started), Some(sampled)) => Some(started.max(sampled)),
            (Some(started), None) => Some(started),
            (None, Some(sampled)) => Some(sampled),
            (None, None) => None,
        };
        if freshness_reference.is_some_and(|last_fresh| {
            Instant::now().saturating_duration_since(last_fresh) >= self.telemetry_stale_after
        }) {
            return Err(anyhow!(
                "telemetry stale: no successful vehicle response for {} ms",
                self.telemetry_stale_after.as_millis()
            ));
        }
        Ok(did_work)
    }

    /// Keep reconnecting until a complete identity/cache acquisition succeeds.
    /// The bounded delays in [`Self::reconnect`] prevent a failed transport from
    /// turning this driver into a busy loop; callers can cancel the future.
    pub async fn drive_reconnect(&mut self) -> Result<()> {
        loop {
            match self.reconnect().await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    tracing::warn!("reconnect attempt failed; retrying: {error}");
                }
            }
        }
    }

    /// Execute one planned telemetry cycle as an explicit request loop, then
    /// at most one verifier probe (spec §7.2, §9.1.4). Every supported entry
    /// due this cycle is polled; verifier work runs only through
    /// [`Verifier::next`] so retry backoff and NO DATA separation hold.
    pub async fn poll_cycle(&mut self) -> Result<bool> {
        self.process_control_boundary().await?;
        // Telemetry pauses for foreground modes (spec §11) and must never run
        // after Shutdown — a session-gone transport error here would send a
        // driver loop into reconnect against an intentionally released port.
        if !matches!(self.snapshot.mode, ModeState::Telemetry) {
            return Ok(false);
        }
        self.cycle = self.cycle.saturating_add(1);

        if self
            .cycle
            .is_multiple_of(MISSING_IDENTITY_RETRY_INTERVAL_CYCLES)
            && self.retry_missing_identity().await?
        {
            return Ok(true);
        }

        let plan = self
            .scheduler
            .plan_cycle(self.cycle, &ViewId::Gauges, &self.capabilities);
        let mut did_work = false;

        // Adapter voltage is independent of the ECU PID scheduler. Refresh
        // it sparsely so the extra serial command cannot dominate J1850 bus
        // time, while still exposing an actual electrical measurement.
        if self.cycle.is_multiple_of(20) {
            self.refresh_adapter_voltage().await;
        }

        for request in plan {
            self.process_control_boundary().await?;
            if !matches!(self.snapshot.mode, ModeState::Telemetry) {
                return Ok(did_work);
            }
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

        did_work |= self.poll_profile_display_signals().await?;

        let Some(key) = self.verifier.next(Instant::now(), &ViewId::Gauges).cloned() else {
            return self.finish_poll_cycle(did_work);
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
            Ok(_) => self.finish_poll_cycle(true),
            Err(super::verifier::ProbeError::Transport) => {
                Err(anyhow!("PID verifier request failed: transport"))
            }
            Err(_) => self.finish_poll_cycle(true),
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
        let reading = session.read_pid(pid).await.map_err(|error| {
            if error.is_connection_loss() {
                return super::verifier::ProbeError::Transport;
            }
            match error {
                Obd2Error::NoData => super::verifier::ProbeError::NoData,
                Obd2Error::Timeout => super::verifier::ProbeError::Timeout,
                Obd2Error::UnsupportedPid { .. } => super::verifier::ProbeError::UnsupportedPid,
                Obd2Error::NegativeResponse {
                    nrc:
                        NegativeResponse::RequestOutOfRange
                        | NegativeResponse::ServiceNotSupported
                        | NegativeResponse::SubFunctionNotSupported,
                    ..
                } => super::verifier::ProbeError::ExplicitUnsupported,
                _ => super::verifier::ProbeError::Decode,
            }
        })?;
        let value = reading
            .value
            .as_f64()
            .map_err(|_| super::verifier::ProbeError::Decode)?;
        self.snapshot.sample_at = Some(Instant::now());
        Ok(value)
    }

    /// Poll display-owned Turbo signals at their normal cadence and at most
    /// one Fuel signal per pass. A full LLY injector sweep is deliberately
    /// round-robin: it must not monopolize a J1850 request boundary.
    async fn poll_profile_display_signals(&mut self) -> Result<bool> {
        let (context, selected) = match (&self.profile_context, &self.selected_profile) {
            (Some(context), Some(selected)) => (context.clone(), selected.clone()),
            _ => return Ok(false),
        };
        let registry = ProfileRegistry::with_builtins();
        let Some(profile) = registry.get(selected.profile_id()) else {
            return Ok(false);
        };
        let turbo_keys = profile
            .signal_display()
            .iter()
            .filter_map(|display| match (display.category, display.source) {
                (SignalCategory::Turbo, SignalDisplaySource::ProfileSignal(key)) => profile
                    .signals()
                    .iter()
                    .find(|signal| signal.key == key)
                    .filter(|signal| {
                        Self::profile_signal_pollable(signal.confidence, signal.failure_policy)
                    })
                    .filter(|signal| Self::profile_signal_due(signal.cadence, self.cycle))
                    .map(|signal| signal.key),
                _ => None,
            })
            .collect::<Vec<_>>();
        let fuel_keys = profile
            .signal_display()
            .iter()
            .filter_map(|display| match (display.category, display.source) {
                (SignalCategory::Fuel, SignalDisplaySource::ProfileSignal(key)) => profile
                    .signals()
                    .iter()
                    .find(|signal| signal.key == key)
                    .filter(|signal| {
                        Self::profile_signal_pollable(signal.confidence, signal.failure_policy)
                    })
                    .map(|signal| signal.key),
                _ => None,
            })
            .collect::<Vec<_>>();
        let fuel_key = if fuel_keys.is_empty()
            || !self
                .cycle
                .is_multiple_of(PROFILE_FUEL_SIGNAL_INTERVAL_CYCLES)
        {
            None
        } else {
            fuel_keys
                .get(
                    ((self.cycle / PROFILE_FUEL_SIGNAL_INTERVAL_CYCLES) as usize) % fuel_keys.len(),
                )
                .copied()
        };
        let keys = turbo_keys.into_iter().chain(fuel_key).collect::<Vec<_>>();
        if keys.is_empty() {
            return Ok(false);
        }

        let mut observed = Vec::new();
        {
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| anyhow!("profile telemetry requested without an active session"))?;
            let runtime = ProfileRuntime::new(&registry);
            let mut evidence = NullEvidenceSink;
            for key in keys {
                match runtime
                    .execute_request(
                        session,
                        &context,
                        &selected,
                        ProfileCapabilityId::Signal(key),
                        ProfileRequestId::SINGLE,
                        &mut evidence,
                    )
                    .await
                {
                    Ok(ProfileResponse::Signal(signal)) => observed.push((key, signal.value)),
                    Ok(ProfileResponse::Dtcs(_)) => {}
                    Err(error) => {
                        // A rejected profile signal must not disturb standard
                        // telemetry. Evidence and capability backoff are added
                        // by the profile-verifier phase; this bounded display
                        // poll merely withholds an unobserved value.
                        tracing::debug!(
                            ?error,
                            profile_signal = key,
                            "profile display signal read failed"
                        );
                    }
                }
            }
        }
        if observed.is_empty() {
            return Ok(false);
        }
        let mut signals = (*self.snapshot.signals).clone();
        for (key, value) in observed {
            signals.insert(key.to_string(), value);
        }
        self.snapshot.signals = std::sync::Arc::new(signals);
        self.snapshot.sample_at = Some(Instant::now());
        self.publish();
        Ok(true)
    }

    fn profile_signal_pollable(confidence: Confidence, failure_policy: FailurePolicy) -> bool {
        !matches!(confidence, Confidence::Candidate | Confidence::Rejected)
            && !matches!(
                failure_policy,
                FailurePolicy::CandidateOnly | FailurePolicy::DoNotPoll
            )
    }

    fn profile_signal_due(cadence: PollCadence, cycle: u64) -> bool {
        let interval = match cadence {
            PollCadence::Fast => PROFILE_FAST_SIGNAL_INTERVAL_CYCLES,
            PollCadence::Medium => PROFILE_MEDIUM_SIGNAL_INTERVAL_CYCLES,
            PollCadence::Slow => PROFILE_SLOW_SIGNAL_INTERVAL_CYCLES,
            PollCadence::OnDemand => return false,
        };
        cycle.is_multiple_of(interval)
    }

    /// Recover automatically when startup Mode 09 was transiently unreadable.
    /// The successful identity is carried across exactly one fresh connection,
    /// because the reconnect's own Mode 09 read can fail independently on VPW.
    async fn retry_missing_identity(&mut self) -> Result<bool> {
        if self.vin.is_some() || self.snapshot.connection.vin.is_some() {
            return Ok(false);
        }
        let Some(session) = self.session.as_mut() else {
            return Ok(false);
        };

        match session.read_vin().await {
            Ok(vin) if crate::profiles::validate_vin_charset(&vin) => {
                self.rescan_identity = Some(IdentityOutcome {
                    vin: Some(vin),
                    confidence: IdentityConfidence::Single,
                });
                self.session = None;
                self.connect().await?;
                Ok(true)
            }
            Ok(vin) => {
                tracing::warn!(%vin, "discarding malformed VIN returned during identity retry");
                Ok(false)
            }
            Err(error) if error.is_connection_loss() => {
                Err(anyhow!(error).context("VIN recovery lost the vehicle connection"))
            }
            Err(error) => {
                tracing::debug!(%error, "VIN still unreadable during background retry");
                Ok(false)
            }
        }
    }

    async fn refresh_adapter_voltage(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        match session.battery_voltage().await {
            Ok(Some(voltage)) => {
                self.snapshot.adapter_voltage = Some(voltage);
                self.publish();
            }
            Ok(None) => {
                self.snapshot.adapter_voltage = None;
                self.publish();
            }
            Err(error) => {
                // Retain the last good reading; a transient ATRV failure is
                // neither an ECU fault nor a reason to disturb telemetry.
                tracing::debug!(%error, "adapter voltage refresh failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mode_runner::testing::{ScriptedConnector as TestConnector, ScriptedResponse};
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

    #[tokio::test]
    async fn generic_session_retries_vin_and_selects_profile_after_recovery() {
        let connector = TestConnector::new("1GTHK29294E391526")
            .with_protocol(obd2_core::vehicle::Protocol::J1850Vpw);
        for _ in 0..3 {
            connector
                .script
                .push(0x09, Some(0x02), ScriptedResponse::NoData);
        }
        let calls = Arc::clone(&connector.calls);
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let mut runner = ModeRunner::new(connector, store);

        runner.connect().await.unwrap();
        assert_eq!(runner.snapshot().connection.vin, None);
        assert_eq!(runner.snapshot().selected_profile, None);

        runner.cycle = MISSING_IDENTITY_RETRY_INTERVAL_CYCLES - 1;
        assert!(runner.poll_cycle().await.unwrap());

        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            runner.snapshot().connection.vin.as_deref(),
            Some("1GTHK29294E391526")
        );
        assert_eq!(
            runner.snapshot().selected_profile,
            Some(crate::profiles::gm::LLY_PROFILE_ID)
        );
        assert!(matches!(runner.snapshot().mode, ModeState::Telemetry));
    }

    #[tokio::test]
    async fn explicit_adapter_bus_loss_ends_the_telemetry_cycle() {
        let connector = TestConnector::new("1HGCM82633A004352");
        connector.script.push(
            0x01,
            Some(0x0C),
            ScriptedResponse::Adapter("unable to connect to vehicle".to_string()),
        );
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let mut runner = ModeRunner::new(connector, store);
        runner.connect().await.unwrap();

        let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
        runner.capabilities = CapabilitySet::default();
        runner
            .capabilities
            .insert(key, CapabilityOutcome::Supported);
        runner.scheduler = Scheduler::new(vec![standard_pid_descriptor(0x0C)]);
        runner.verifier = Verifier::new();

        let error = runner.poll_cycle().await.unwrap_err();
        assert!(error.to_string().contains("transport"));
    }

    #[tokio::test]
    async fn telemetry_without_any_successful_vehicle_response_becomes_stale() {
        let connector = TestConnector::new("1HGCM82633A004352");
        connector
            .script
            .push(0x01, Some(0x0C), ScriptedResponse::NoData);
        connector
            .script
            .push(0x01, Some(0x0C), ScriptedResponse::NoData);
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let mut runner = ModeRunner::new(connector, store);
        runner.connect().await.unwrap();

        let key = CapabilityKey::new(CapabilityKind::Pid, "010C", "broadcast");
        runner.capabilities = CapabilitySet::default();
        runner
            .capabilities
            .insert(key, CapabilityOutcome::Supported);
        runner.scheduler = Scheduler::new(vec![standard_pid_descriptor(0x0C)]);
        runner.verifier = Verifier::new();
        runner.telemetry_stale_after = Duration::ZERO;

        let error = runner.poll_cycle().await.unwrap_err();
        assert!(error.to_string().contains("telemetry stale"));
    }

    #[tokio::test]
    async fn a_new_connection_discards_prior_session_values() {
        let calls = Arc::new(AtomicUsize::new(0));
        let connector = ScriptedConnector {
            calls,
            vin: "1HGCM82633A004352",
        };
        let store = SqliteCapabilityStore::from_database(Database::open_in_memory().unwrap());
        let mut runner = ModeRunner::new(connector, store);
        runner.snapshot.signals = std::sync::Arc::new(std::collections::BTreeMap::from([(
            "010C".to_string(),
            681.0,
        )]));
        runner.snapshot.sample_at = Some(Instant::now());

        runner.connect().await.unwrap();

        assert!(runner.snapshot().signals.is_empty());
        assert!(runner.snapshot().sample_at.is_none());
    }
}
