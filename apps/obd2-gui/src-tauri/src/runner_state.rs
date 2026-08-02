//! GUI ownership boundary for the shared mode runner.
//!
//! Tauri command handlers keep only [`RunnerControl`] and a cached snapshot.
//! The spawned task below is the sole owner of `ModeRunner`, and therefore of
//! the live `Session`.  Nothing in this module constructs a serial adapter;
//! that is the connector's responsibility.

use std::{
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use obd2_dash::mode_runner::{
    control_channel, CapabilityStore, CommandReply, ControlCommand, ModeRunner, ModeState,
    RunnerControl, RunnerSnapshot, SessionConnector, SqliteCapabilityStore,
};
use obd2_dash::recording::writer::RecordingWriter;
use obd2_db::models::{
    CapabilityContext, CapabilityLoad, CapabilityRecord, CapabilitySetReplacement, OutcomeUpdate,
};
use tokio::sync::{mpsc, oneshot, watch};

const RECORDING_QUEUE_CAPACITY: usize = 256;

/// File ownership is deliberately separate from the mode-runner task.  A
/// saturated recording queue drops a sample rather than extending a serial
/// request's latency; start/stop acknowledgements are never dropped.
enum RecordingCommand {
    Start {
        directory: PathBuf,
        snapshot: RunnerSnapshot,
        acknowledgement: oneshot::Sender<Result<String, String>>,
    },
    Stop {
        acknowledgement: oneshot::Sender<Result<String, String>>,
    },
    Snapshot(RunnerSnapshot),
}

/// A failed local database must not prevent a live diagnostic session.  The
/// lifecycle treats this store's typed errors as session-only persistence and
/// switches to its conservative fallback; the error text is retained here for
/// support logs without exposing database internals to Tauri commands.
#[derive(Clone)]
enum GuiCapabilityStore {
    Sqlite(SqliteCapabilityStore),
    Disabled(Arc<str>),
}

#[async_trait]
impl CapabilityStore for GuiCapabilityStore {
    async fn load(&self, vin: &str, context: &CapabilityContext) -> Result<CapabilityLoad> {
        match self {
            Self::Sqlite(store) => store.load(vin, context).await,
            Self::Disabled(reason) => Err(anyhow!("capability storage disabled: {reason}")),
        }
    }

    async fn replace(&self, replacement: &CapabilitySetReplacement) -> Result<String> {
        match self {
            Self::Sqlite(store) => store.replace(replacement).await,
            Self::Disabled(reason) => Err(anyhow!("capability storage disabled: {reason}")),
        }
    }

    async fn update_outcomes(
        &self,
        vin: &str,
        set_id: &str,
        records: &[CapabilityRecord],
    ) -> Result<OutcomeUpdate> {
        match self {
            Self::Sqlite(store) => store.update_outcomes(vin, set_id, records).await,
            Self::Disabled(reason) => Err(anyhow!("capability storage disabled: {reason}")),
        }
    }

    async fn load_exact_vehicle_fuel_type(&self, vin: &str) -> Result<Option<String>> {
        match self {
            Self::Sqlite(store) => store.load_exact_vehicle_fuel_type(vin).await,
            Self::Disabled(reason) => Err(anyhow!("capability storage disabled: {reason}")),
        }
    }
}

/// Runner bootstrap inputs resolved by Tauri setup code.  `capability_db_path`
/// is deliberately a path rather than an app handle so this module remains
/// transport/UI independent and is directly testable with a temporary path.
#[derive(Debug, Clone)]
pub struct RunnerStateConfig {
    pub capability_db_path: PathBuf,
    /// A completed runner cycle is followed by this delay.  The delay is in
    /// the session-owning task, so Tauri's 500 ms snapshot reader can never
    /// create overlapping serial work.
    pub cycle_delay: Duration,
}

impl RunnerStateConfig {
    pub fn new(capability_db_path: PathBuf) -> Self {
        Self {
            capability_db_path,
            cycle_delay: Duration::from_millis(250),
        }
    }
}

/// Tauri-managed runner surface.  A snapshot read clones cached state only;
/// it cannot take the Session or issue transport I/O.
#[derive(Clone)]
pub struct RunnerState {
    control: RunnerControl,
    snapshot_rx: Arc<watch::Receiver<RunnerSnapshot>>,
    recording_tx: mpsc::Sender<RecordingCommand>,
}

impl RunnerState {
    /// Return the GUI handle immediately and bootstrap the single
    /// session-owning task in the background.  SQLite open/migration happens
    /// behind `SqliteCapabilityStore::open`'s `spawn_blocking` boundary, so
    /// Tauri setup never opens a database or serial port itself.
    ///
    /// Commands accepted while SQLite opens remain in the bounded control
    /// queue and are applied when the runner starts.  If SQLite cannot open,
    /// the producer is dropped so callers receive `Closed` rather than an
    /// acknowledgement for a runner that does not exist.
    pub fn bootstrap<C>(connector: C, config: RunnerStateConfig) -> Self
    where
        C: SessionConnector + 'static,
        C::Adapter: Send + 'static,
    {
        let (control, receiver) = control_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(RunnerSnapshot::empty());
        let recording_tx = start_recording_worker();
        let runner_recording_tx = recording_tx.clone();
        let database_path = config.capability_db_path;
        let cycle_delay = config.cycle_delay;

        tauri::async_runtime::spawn(async move {
            let store = match SqliteCapabilityStore::open(&database_path).await {
                Ok(store) => GuiCapabilityStore::Sqlite(store),
                Err(error) => {
                    eprintln!(
                        "OBD runner will use session-only capabilities after database open failure {}: {error:#}",
                        database_path.display()
                    );
                    GuiCapabilityStore::Disabled(Arc::from(error.to_string()))
                }
            };
            if let Err(error) = run_started_runner(
                connector,
                store,
                receiver,
                snapshot_tx,
                runner_recording_tx,
                cycle_delay,
            )
            .await
            {
                eprintln!("OBD runner stopped: {error:#}");
            }
        });

        Self {
            control,
            snapshot_rx: Arc::new(snapshot_rx),
            recording_tx,
        }
    }

    /// The GUI submits discrete work through the bounded, non-blocking queue.
    /// The returned acknowledgement says whether the runner accepted it at a
    /// request boundary; it is not an indication that diagnostic work has
    /// completed.
    pub fn submit(&self, command: ControlCommand) -> oneshot::Receiver<CommandReply> {
        self.control.submit(command)
    }

    /// Replace the active scheduler view without queueing stale tab changes.
    pub fn set_active_view(&self, view: obd2_dash::mode_runner::ViewId) {
        self.control.set_active_view(view);
    }

    /// A cached, allocation-free-at-the-transport-boundary snapshot clone.
    /// The clone duplicates only the small outer snapshot; its signal and
    /// diagnostic collections remain `Arc` shared with the runner.
    pub fn snapshot(&self) -> RunnerSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    /// Start append-only recording without involving the Tauri or runner
    /// threads in filesystem I/O.
    pub async fn start_recording(&self, directory: PathBuf) -> Result<String, String> {
        let (acknowledgement, receiver) = oneshot::channel();
        self.recording_tx
            .try_send(RecordingCommand::Start {
                directory,
                snapshot: self.snapshot(),
                acknowledgement,
            })
            .map_err(|error| format!("recording service unavailable: {error}"))?;
        receiver
            .await
            .map_err(|_| "recording service stopped before acknowledging start".to_string())?
    }

    /// Flush the append-only recording before returning its completed path.
    pub async fn stop_recording(&self) -> Result<String, String> {
        let (acknowledgement, receiver) = oneshot::channel();
        self.recording_tx
            .send(RecordingCommand::Stop { acknowledgement })
            .await
            .map_err(|_| "recording service is stopped".to_string())?;
        receiver
            .await
            .map_err(|_| "recording service stopped before flushing".to_string())?
    }

    /// Orderly shutdown.  The reply resolves only after the runner has
    /// dropped its Session and flushed accepted capability persistence.
    pub async fn shutdown(&self) -> CommandReply {
        let reply = match self.submit(ControlCommand::Shutdown).await {
            Ok(reply) => reply,
            Err(_) => CommandReply::Closed,
        };
        if let Err(error) = self.stop_recording().await {
            // Idle is the usual case.  Do not turn a successful serial
            // shutdown into a failure merely because no recording was open.
            if error != "no recording is active" {
                eprintln!("recording shutdown flush failed: {error}");
            }
        }
        reply
    }
}

/// Start a fully constructed runner and mirror its watch stream to the GUI's
/// stable receiver.  The GUI receiver exists before capability-store open;
/// this relay keeps that lifetime independent from the worker's construction
/// order without exposing the Session.
async fn run_started_runner<C, S>(
    connector: C,
    store: S,
    receiver: obd2_dash::mode_runner::RunnerControlReceiver,
    snapshot_tx: watch::Sender<RunnerSnapshot>,
    recording_tx: mpsc::Sender<RecordingCommand>,
    cycle_delay: Duration,
) -> Result<()>
where
    C: SessionConnector + 'static,
    C::Adapter: Send + 'static,
    S: CapabilityStore + Clone + 'static,
{
    let mut runner = ModeRunner::new(connector, store);
    let runner_snapshots = runner.subscribe();
    runner.attach_control(receiver);
    let relay =
        tauri::async_runtime::spawn(relay_snapshots(runner_snapshots, snapshot_tx, recording_tx));
    let result = drive_runner(runner, cycle_delay).await;
    // Drop the only runner-owned watch sender before joining the relay.  This
    // makes the outer receiver retain its final coherent snapshot rather than
    // waiting forever after shutdown.
    let _ = relay.await;
    result
}

async fn relay_snapshots(
    mut source: watch::Receiver<RunnerSnapshot>,
    destination: watch::Sender<RunnerSnapshot>,
    recording_tx: mpsc::Sender<RecordingCommand>,
) {
    let initial = source.borrow_and_update().clone();
    destination.send_replace(initial.clone());
    let _ = recording_tx.try_send(RecordingCommand::Snapshot(initial));
    while source.changed().await.is_ok() {
        let snapshot = source.borrow_and_update().clone();
        destination.send_replace(snapshot.clone());
        let _ = recording_tx.try_send(RecordingCommand::Snapshot(snapshot));
    }
}

fn start_recording_worker() -> mpsc::Sender<RecordingCommand> {
    let (sender, mut receiver) = mpsc::channel(RECORDING_QUEUE_CAPACITY);
    let worker = thread::Builder::new()
        .name("obd2-recording".to_string())
        .spawn(move || {
            let mut active: Option<(RecordingWriter, Instant, std::collections::BTreeSet<String>)> =
                None;
            while let Some(command) = receiver.blocking_recv() {
                match command {
                    RecordingCommand::Start {
                        directory,
                        snapshot,
                        acknowledgement,
                    } => {
                        let result = if active.is_some() {
                            Err("a recording is already active".to_string())
                        } else {
                            start_writer(&directory).and_then(|mut writer| {
                                let mut recorded_evidence = std::collections::BTreeSet::new();
                                write_snapshot(
                                    &mut writer,
                                    Instant::now(),
                                    &mut recorded_evidence,
                                    &snapshot,
                                )
                                .map_err(|error| {
                                    format!("failed to write initial recording frame: {error}")
                                })?;
                                let path = writer.file_path.display().to_string();
                                active = Some((writer, Instant::now(), recorded_evidence));
                                Ok(path)
                            })
                        };
                        let _ = acknowledgement.send(result);
                    }
                    RecordingCommand::Stop { acknowledgement } => {
                        let result = active
                            .take()
                            .ok_or_else(|| "no recording is active".to_string())
                            .and_then(|(writer, _, _)| {
                                writer
                                    .finish()
                                    .map(|path| path.display().to_string())
                                    .map_err(|error| {
                                        format!("failed to finalize recording: {error}")
                                    })
                            });
                        let _ = acknowledgement.send(result);
                    }
                    RecordingCommand::Snapshot(snapshot) => {
                        if let Some((writer, started, recorded_evidence)) = active.as_mut() {
                            if let Err(error) =
                                write_snapshot(writer, *started, recorded_evidence, &snapshot)
                            {
                                eprintln!("recording write failed; stopping recording: {error}");
                                active = None;
                            }
                        }
                    }
                }
            }
            if let Some((writer, _, _)) = active {
                if let Err(error) = writer.finish() {
                    eprintln!("recording flush after channel close failed: {error}");
                }
            }
        });
    if let Err(error) = worker {
        eprintln!("failed to spawn OBD recording worker: {error}");
    }
    sender
}

fn start_writer(directory: &std::path::Path) -> Result<RecordingWriter, String> {
    let session_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock is before UNIX epoch: {error}"))?
        .as_millis();
    RecordingWriter::new_v3(directory, &format!("gui-{session_id}"), None, None, 250)
        .map_err(|error| format!("failed to start recording: {error}"))
}

fn write_snapshot(
    writer: &mut RecordingWriter,
    started: Instant,
    recorded_evidence: &mut std::collections::BTreeSet<String>,
    snapshot: &RunnerSnapshot,
) -> std::io::Result<()> {
    let offset_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    if let Some(voltage) = snapshot.signals.get("0142") {
        writer.write_voltage(offset_ms, *voltage)?;
    }
    for (key, value) in snapshot.signals.iter() {
        if key == "0142" {
            continue;
        }
        let Some(pid) = key
            .strip_prefix("01")
            .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        else {
            continue;
        };
        writer.write_pid(offset_ms, pid, *value, &[])?;
    }
    for dtc in snapshot
        .diagnostic
        .standard_dtcs
        .iter()
        .chain(snapshot.diagnostic.profile_dtcs.iter())
    {
        writer.write_dtc(offset_ms, &dtc.key.code)?;
    }
    for evidence in &snapshot.diagnostic.profile_evidence {
        let key = format!(
            "{}:{}:{}",
            evidence.timestamp, evidence.profile_id, evidence.capability_id
        );
        if recorded_evidence.insert(key) {
            let _ = writer.write_profile_evidence(offset_ms, evidence)?;
        }
    }
    Ok(())
}

async fn drive_runner<C, S>(mut runner: ModeRunner<C, S>, cycle_delay: Duration) -> Result<()>
where
    C: SessionConnector + 'static,
    C::Adapter: Send + 'static,
    S: CapabilityStore + Clone + 'static,
{
    // A failed initial connection is handled by the same reconnect cadence as
    // a later transport failure.  Do not use `drive_reconnect`: this loop
    // checks the control receiver between attempts so dropping the GUI state
    // remains an orderly shutdown rather than an immortal reconnect task.
    let mut initial_attempt = true;
    loop {
        runner.process_control_boundary().await?;
        if is_shutdown(&runner) {
            return Ok(());
        }

        let connected = if initial_attempt {
            initial_attempt = false;
            runner.connect().await
        } else {
            runner.reconnect().await
        };
        match connected {
            Ok(()) => break,
            Err(error) => {
                eprintln!("OBD runner connection attempt failed: {error:#}");
            }
        }
    }

    loop {
        let result = runner.run_once().await;
        if is_shutdown(&runner) {
            return Ok(());
        }
        if let Err(error) = result {
            // The lifecycle runner drops a failed session before reconnecting.
            // Keep retry policy in this owner so every reconnect still uses
            // the same control plane and never gives a Session to Tauri.
            eprintln!("OBD runner cycle failed; reconnecting: {error:#}");
            loop {
                runner.process_control_boundary().await?;
                if is_shutdown(&runner) {
                    return Ok(());
                }
                match runner.reconnect().await {
                    Ok(()) => break,
                    Err(error) => eprintln!("OBD runner reconnect failed: {error:#}"),
                }
            }
            continue;
        }

        // A zero delay is permitted for deterministic harnesses; production
        // passes a finite cadence to prevent an idle session from busy-looping.
        if !cycle_delay.is_zero() {
            tokio::time::sleep(cycle_delay).await;
        }
    }
}

fn is_shutdown<C, S>(runner: &ModeRunner<C, S>) -> bool
where
    C: SessionConnector,
    S: CapabilityStore + Clone,
{
    matches!(runner.snapshot().mode, ModeState::ShuttingDown)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn snapshot_recording_writes_standard_pid_and_voltage_frames() {
        let directory = std::env::temp_dir().join(format!(
            "obd2-gui-runner-recording-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after Unix epoch")
                .as_nanos()
        ));
        fs::create_dir_all(&directory).expect("create recording test directory");

        let mut signals = BTreeMap::new();
        signals.insert("010C".to_string(), 812.5);
        signals.insert("0142".to_string(), 13.8);
        let snapshot = RunnerSnapshot {
            signals: Arc::new(signals),
            ..RunnerSnapshot::empty()
        };
        let mut writer = start_writer(&directory).expect("start writer");
        write_snapshot(
            &mut writer,
            Instant::now(),
            &mut std::collections::BTreeSet::new(),
            &snapshot,
        )
        .expect("write snapshot");
        let path = writer.finish().expect("finish writer");
        let (_header, frames) =
            obd2_dash::recording::reader::read_recording(&path).expect("read written recording");

        assert!(frames.iter().any(|frame| frame.pid_code == 0x0C));
        assert!(frames.iter().any(|frame| frame.frame_type == 0x02));
        fs::remove_dir_all(directory).expect("remove recording test directory");
    }
}
