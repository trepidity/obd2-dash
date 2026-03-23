mod analysis;
mod ai;
mod app;
mod connection_prefs;
mod debug_log;
mod domain;
mod mock_profile;
mod nhtsa;
pub mod recording;
mod scanner;
mod tui;
pub mod vehicle_data;
mod widget;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use tokio::sync::mpsc;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use app::{AppState, DashboardLayout, Message, PopupState, ScanMode};
use ai::{AiClient, AiConfig};
use connection_prefs::ConnectionPrefs;
use domain::{ConnectionState, SpeedUnit, TemperatureUnit};
use mock_profile::mock_vin;
use recording::RecordingState;
use recording::storage::{StorageConfig, StorageManager};
use scanner::{DeviceKind, ScanEvent};
use tui::{
    event::{key_to_message, Event, EventHandler},
    panel as tui_panel, Tui,
};
use widget::config::DashboardConfig;
use widget::edit_mode::{EditModeState, EditPhase};

// New obd2-core imports
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::enhanced::Reading;
use obd2_core::adapter::Adapter;
use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::mock::MockAdapter;
use obd2_core::session::Session;
use obd2_core::vehicle::ModuleId;
use obd2_core::error::Obd2Error;

use domain::O2Reading;

/// Number of mock DTC scenarios (used by dtc cycling key).
const DTC_SCENARIO_COUNT: u8 = 4;

#[derive(Parser, Debug)]
#[command(name = "obd2-dash", about = "OBD2 vehicle diagnostics TUI dashboard")]
struct Cli {
    /// Serial port path (e.g. /dev/ttyUSB0, /dev/cu.usbserial-*)
    #[arg(short, long)]
    port: Option<String>,

    /// Baud rate for serial connection
    #[arg(short, long, default_value = "115200")]
    baud: u32,

    /// Use mock data instead of a real OBD2 adapter
    #[arg(long)]
    mock: bool,

    /// Mock vehicle profile: mini, chevy, honda, or generic (default)
    #[arg(long, default_value = "generic")]
    mock_vehicle: String,

    /// Polling interval in milliseconds
    #[arg(long, default_value = "250")]
    poll_ms: u64,

    /// SQLite database path
    #[arg(long, default_value = "obd2-dash.db")]
    db_path: String,

    /// Run in headless mode (no TUI, prints to stdout). Auto-enabled when stdout is not a TTY.
    #[arg(long)]
    headless: bool,

    /// Dashboard config JSON path
    #[arg(long, default_value = "dashboard.json")]
    config: String,

    /// Recordings directory path
    #[arg(long, default_value = "recordings")]
    recordings_dir: String,

    /// Max recording storage in MB
    #[arg(long, default_value = "500")]
    max_storage_mb: u64,

    /// Connect via BLE instead of serial port
    #[arg(long)]
    ble: bool,

    /// BLE adapter name filter (e.g. "OBDLink CX", "OBDLink MX+")
    #[arg(long)]
    ble_name: Option<String>,

    /// BLE scan timeout in seconds
    #[arg(long, default_value = "5")]
    ble_scan_secs: u64,

    /// Connect to ELM327 emulator (sets baud to 38400)
    #[arg(long)]
    emu: bool,
}

/// Helper: get all pollable PIDs (standard Mode 01 PIDs that are scalar).
fn pollable_pids() -> Vec<Pid> {
    Pid::all()
        .iter()
        .copied()
        .filter(|p| {
            // Skip bitmap/support PIDs and state PIDs
            let code = p.0;
            !matches!(code, 0x00 | 0x01 | 0x03 | 0x1C | 0x20 | 0x40 | 0x60)
        })
        .collect()
}

/// Helper: get all known PID codes for threshold resolution.
fn all_known_codes() -> Vec<u8> {
    pollable_pids().iter().map(|p| p.0).collect()
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut cli = Cli::parse();

    // Emulator mode: override baud to 38400 (Ircama ELM327-emulator default)
    if cli.emu {
        cli.baud = 38400;
        tracing::info!("Emulator mode: baud=38400");
    }

    // Set up file-based logging (stdout is owned by the TUI)
    let log_buffer = debug_log::LogBuffer::new();
    let file_appender = rolling::daily("logs", "obd2-dash.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("debug")))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .with(debug_log::BufferLayer::new(log_buffer.clone()))
        .init();

    tracing::info!("Starting obd2-dash");

    // Initialize database
    let db_path = PathBuf::from(&cli.db_path);
    let database = obd2_db::Database::open(&db_path)?;
    obd2_db::seed::seed_all(&database)?;
    tracing::info!("Database initialized at {}", db_path.display());

    // Resolve mock VIN from CLI profile name
    let mock_vehicle_vin = mock_vin(&cli.mock_vehicle);

    // Look up vehicle info if using a known profile
    let vehicle_info = database.get_vehicle(mock_vehicle_vin)?;
    let engine_family_code = vehicle_info
        .as_ref()
        .and_then(|v| v.engine_family_code.clone());

    // Resolve thresholds for all known PIDs
    let thresholds = database.resolve_all_thresholds(
        vehicle_info.as_ref().map(|v| v.vin.as_str()),
        engine_family_code.as_deref(),
        &all_known_codes(),
    )?;

    tracing::info!(
        "Resolved {} thresholds for vehicle {:?}",
        thresholds.len(),
        vehicle_info.as_ref().map(|v| v.display_name())
    );

    // Shared DTC scenario selector (mock only -- incremented by 'd' key)
    let dtc_scenario = Arc::new(AtomicU8::new(0));

    // Channel for OBD2 poll results -> main loop
    let (obd_tx, mut obd_rx) = mpsc::unbounded_channel::<Message>();

    // Connection prefs path
    let prefs_path = PathBuf::from("connection.json");
    let connection_prefs = ConnectionPrefs::load(&prefs_path);

    // Spawn OBD2 polling task (or start disconnected)
    let poll_ms = cli.poll_ms;
    let obd_handle: Option<tokio::task::JoinHandle<()>> = if cli.mock {
        let _tx_clone = obd_tx.clone();
        let handle = spawn_mock_poll(mock_vehicle_vin, Arc::clone(&dtc_scenario), poll_ms, obd_tx.clone());
        Some(handle)
    } else if cli.ble {
        let name = cli.ble_name.clone().unwrap_or_default();
        let device = DeviceKind::Ble { name };
        Some(spawn_connect_and_poll(
            device,
            cli.baud,
            poll_ms,
            obd_tx.clone(),
            prefs_path.clone(),
            cli.ble_scan_secs,
        ))
    } else if cli.emu {
        #[cfg(unix)]
        {
            if let Some(ref port) = cli.port {
                let port_path = port.clone();
                let tx = obd_tx.clone();
                Some(tokio::spawn(async move {
                    let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connecting));

                    // Open PTY transport
                    let transport = match obd2_core::transport::serial::SerialTransport::new(&port_path, 38400) {
                        Ok(t) => t,
                        Err(e) => {
                            let msg = format!("Failed to open PTY {}: {}", port_path, e);
                            tracing::error!("{}", msg);
                            let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(msg.clone())));
                            let _ = tx.send(Message::Error(msg));
                            return;
                        }
                    };
                    tracing::info!("Emulator transport opened: {}", port_path);

                    let adapter = Elm327Adapter::new(Box::new(transport));
                    let mut session = Session::new(adapter);

                    run_session_poll_loop(&mut session, poll_ms, &tx).await;
                }))
            } else {
                tracing::error!("--emu requires --port <PTY path>");
                None
            }
        }
        #[cfg(not(unix))]
        {
            tracing::error!("--emu mode is only supported on Unix (macOS/Linux)");
            None
        }
    } else if let Some(ref port) = cli.port {
        let device = DeviceKind::Serial {
            port_path: port.clone(),
            baud: cli.baud,
        };
        Some(spawn_connect_and_poll(
            device,
            cli.baud,
            poll_ms,
            obd_tx.clone(),
            prefs_path.clone(),
            cli.ble_scan_secs,
        ))
    } else if let Some(ref last_device) = connection_prefs.last_device {
        tracing::info!("Reconnecting to last device: {:?}", last_device);
        Some(spawn_connect_and_poll(
            last_device.clone(),
            cli.baud,
            poll_ms,
            obd_tx.clone(),
            prefs_path.clone(),
            cli.ble_scan_secs,
        ))
    } else if let Ok(port) = auto_detect_port() {
        tracing::info!("Auto-detected serial port: {}", port);
        let device = DeviceKind::Serial {
            port_path: port,
            baud: cli.baud,
        };
        Some(spawn_connect_and_poll(
            device,
            cli.baud,
            poll_ms,
            obd_tx.clone(),
            prefs_path.clone(),
            cli.ble_scan_secs,
        ))
    } else {
        tracing::info!("No device specified, starting disconnected (press 's' to scan)");
        None
    };

    let headless = cli.headless || !std::io::stdout().is_terminal();

    // Load dashboard config
    let config_path = PathBuf::from(&cli.config);
    let dashboard_config = DashboardConfig::load(&config_path);

    // Load AI config (optional)
    let ai_config_path = PathBuf::from("ai.json");
    let ai_config = AiConfig::load(&ai_config_path);

    // Initialize storage manager
    let storage_config = StorageConfig {
        recordings_dir: PathBuf::from(&cli.recordings_dir),
        max_total_bytes: cli.max_storage_mb * 1024 * 1024,
        ..StorageConfig::default()
    };
    let storage_manager = StorageManager::new(storage_config);

    if headless {
        run_headless(
            cli.poll_ms,
            &mut obd_rx,
            obd_tx.clone(),
            obd_handle.expect("headless mode requires --mock, --port, or --ble"),
            vehicle_info,
            thresholds,
            database,
        )
        .await
    } else {
        run_tui(
            cli.poll_ms,
            &mut obd_rx,
            obd_handle,
            obd_tx,
            vehicle_info,
            thresholds,
            dtc_scenario,
            dashboard_config,
            config_path,
            storage_manager,
            prefs_path,
            cli.baud,
            cli.ble_scan_secs,
            log_buffer,
            database,
            ai_config,
        )
        .await
    }
}

/// Run the Session-based poll loop (shared between serial/BLE/emu paths).
async fn run_session_poll_loop<A: Adapter>(
    session: &mut Session<A>,
    poll_ms: u64,
    tx: &mpsc::UnboundedSender<Message>,
) {
    // Initialize adapter via identify_vehicle (reads VIN, supported PIDs)
    match session.read_pid(Pid::ENGINE_RPM).await {
        Ok(_) => {
            let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connected));
            let info = session.adapter_info().clone();
            let _ = tx.send(Message::AdapterDetected(info));
        }
        Err(e) => {
            let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(e.to_string())));
            let _ = tx.send(Message::Error(format!("Init failed: {e}")));
            return;
        }
    }

    // Read VIN and identify vehicle (matches spec for enhanced PIDs)
    match session.identify_vehicle().await {
        Ok(profile) => {
            let _ = tx.send(Message::VinDetected(profile.vin.clone()));
        }
        Err(e) => {
            tracing::warn!("Could not identify vehicle: {e}");
            // Fall back to just reading VIN
            match session.read_vin().await {
                Ok(vin) => {
                    let _ = tx.send(Message::VinDetected(vin));
                }
                Err(e2) => {
                    tracing::warn!("Could not read VIN: {e2}");
                }
            }
        }
    }

    // Read initial voltage
    match session.battery_voltage().await {
        Ok(Some(v)) => {
            let _ = tx.send(Message::VoltageUpdate(v));
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("Could not read voltage: {e}");
        }
    }

    // Get supported PIDs
    let supported = session.supported_pids().await.unwrap_or_default();
    let pids_to_poll = pollable_pids();

    // Cache enhanced PID list from matched spec (if any)
    let enhanced_pid_list: Vec<obd2_core::protocol::enhanced::EnhancedPid> =
        if let Some(spec) = session.spec() {
            spec.enhanced_pids.clone()
        } else {
            vec![]
        };
    if !enhanced_pid_list.is_empty() {
        tracing::info!(
            "Found {} enhanced PIDs from spec",
            enhanced_pid_list.len()
        );
    }

    let mut interval = tokio::time::interval(Duration::from_millis(poll_ms));
    let mut voltage_counter = 0u32;

    loop {
        interval.tick().await;

        for &pid in &pids_to_poll {
            if !supported.is_empty() && !supported.contains(&pid) {
                continue;
            }
            match session.read_pid(pid).await {
                Ok(reading) => {
                    if tx.send(Message::PidUpdate(pid, reading)).is_err() {
                        return;
                    }
                }
                Err(Obd2Error::NoData) => {
                    // PID not supported by this vehicle, skip silently
                }
                Err(e) => {
                    tracing::warn!("PID {} query failed: {e}", pid);
                    let _ = tx.send(Message::Error(format!("{}: {e}", pid)));
                }
            }
        }

        voltage_counter += 1;

        // Poll enhanced PIDs every 5th iteration
        if !enhanced_pid_list.is_empty() && voltage_counter % 5 == 0 {
            for epid in &enhanced_pid_list {
                let module = ModuleId::new(&epid.module);
                match session.read_enhanced(epid.did, module).await {
                    Ok(reading) => {
                        let val = reading.value.as_f64().unwrap_or(0.0);
                        let _ = tx.send(Message::EnhancedPidUpdate {
                            did: epid.did,
                            module: epid.module.clone(),
                            name: epid.name.clone(),
                            value: val,
                            unit: epid.unit.clone(),
                        });
                    }
                    Err(_) => {
                        // Skip failed enhanced reads silently
                    }
                }
            }
        }

        // Voltage, DTCs, and O2 monitoring on slower cadence
        if voltage_counter % 10 == 0 {
            if let Ok(Some(v)) = session.battery_voltage().await {
                let _ = tx.send(Message::VoltageUpdate(v));
            }
            if let Ok(mut dtcs) = session.read_dtcs().await {
                obd2_core::session::diagnostics::enrich_dtcs(&mut dtcs, session.spec());
                let _ = tx.send(Message::DtcUpdate(dtcs));
            }
        }

        // O2 monitoring every 20th iteration (changes slowly)
        if voltage_counter % 20 == 0 {
            if let Ok(results) = session.read_all_o2_monitoring().await {
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
        }
    }
}

/// Spawn mock adapter polling task.
fn spawn_mock_poll(
    vin: &'static str,
    _dtc_scenario: Arc<AtomicU8>,
    poll_ms: u64,
    tx: mpsc::UnboundedSender<Message>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let adapter = MockAdapter::with_vin(vin);
        let info = adapter.info().clone();
        let mut session = Session::new(adapter);

        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connected));
        let _ = tx.send(Message::AdapterDetected(info));

        run_session_poll_loop(&mut session, poll_ms, &tx).await;
    })
}

/// Spawn a task that connects to a device, then enters the poll loop.
fn spawn_connect_and_poll(
    device: DeviceKind,
    baud: u32,
    poll_ms: u64,
    tx: mpsc::UnboundedSender<Message>,
    prefs_path: PathBuf,
    ble_scan_secs: u64,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connecting));

        match &device {
            DeviceKind::Serial {
                port_path,
                baud: device_baud,
            } => {
                let actual_baud = if *device_baud > 0 { *device_baud } else { baud };
                tracing::info!("Opening serial port: {} @ {} baud", port_path, actual_baud);

                const MAX_ATTEMPTS: u32 = 3;
                let mut last_error = String::new();

                for attempt in 1..=MAX_ATTEMPTS {
                    if attempt > 1 {
                        tracing::info!(
                            "Retrying serial connection (attempt {}/{})",
                            attempt,
                            MAX_ATTEMPTS
                        );
                        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connecting));
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }

                    let transport = match obd2_core::transport::serial::SerialTransport::new(port_path, actual_baud) {
                        Ok(t) => t,
                        Err(e) => {
                            last_error = format!("Failed to open {}: {}", port_path, e);
                            tracing::warn!("{} (attempt {}/{})", last_error, attempt, MAX_ATTEMPTS);
                            continue;
                        }
                    };

                    // Post-open delay: macOS USB-serial drivers need time to configure
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    let adapter = Elm327Adapter::new(Box::new(transport));
                    let mut session = Session::new(adapter);

                    // Try a test read to verify connection
                    match session.read_pid(Pid::ENGINE_RPM).await {
                        Ok(_) => {
                            let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connected));
                            let info = session.adapter_info().clone();
                            let _ = tx.send(Message::AdapterDetected(info));

                            // Save connection prefs
                            let prefs = ConnectionPrefs {
                                last_device: Some(device.clone()),
                            };
                            if let Err(e) = prefs.save(&prefs_path) {
                                tracing::warn!("Failed to save connection prefs: {}", e);
                            }

                            run_session_poll_loop(&mut session, poll_ms, &tx).await;
                            return;
                        }
                        Err(e) => {
                            last_error = format!("Init failed: {}", e);
                            tracing::warn!("{} (attempt {}/{})", last_error, attempt, MAX_ATTEMPTS);
                            continue;
                        }
                    }
                }

                let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(last_error.clone())));
                let _ = tx.send(Message::Error(last_error));
            }
            DeviceKind::Ble { name } => {
                tracing::info!("Scanning for BLE adapter: {}", name);
                let filter = if name.is_empty() { None } else { Some(name.as_str()) };
                let scan_dur = std::time::Duration::from_secs(ble_scan_secs);
                match obd2_core::transport::ble::BleTransport::scan_and_connect(filter, scan_dur).await {
                    Ok(ble_transport) => {
                        let adapter = Elm327Adapter::new(Box::new(ble_transport));
                        let mut session = Session::new(adapter);
                        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connecting));
                        run_session_poll_loop(&mut session, poll_ms, &tx).await;
                    }
                    Err(e) => {
                        let msg = format!("BLE connection failed: {}", e);
                        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(msg.clone())));
                        let _ = tx.send(Message::Error(msg));
                    }
                }
            }
        }
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_tui(
    poll_ms: u64,
    obd_rx: &mut mpsc::UnboundedReceiver<Message>,
    mut obd_handle: Option<tokio::task::JoinHandle<()>>,
    obd_tx: mpsc::UnboundedSender<Message>,
    vehicle_info: Option<obd2_db::models::VehicleInfo>,
    thresholds: std::collections::HashMap<u8, obd2_db::models::ResolvedThreshold>,
    dtc_scenario: Arc<AtomicU8>,
    dashboard_config: DashboardConfig,
    config_path: PathBuf,
    storage_manager: StorageManager,
    prefs_path: PathBuf,
    default_baud: u32,
    ble_scan_secs: u64,
    log_buffer: debug_log::LogBuffer,
    database: obd2_db::Database,
    ai_config: Option<AiConfig>,
) -> Result<()> {
    let mut tui = Tui::new()?;
    tui.enter()?;

    let mut event_handler = EventHandler::new(poll_ms, 50);
    let mut state = AppState::new(poll_ms);
    state.domain.vehicle_info = vehicle_info;
    state.domain.thresholds_cache = thresholds;
    state.domain.fuel_economy.configure(
        state
            .domain
            .vehicle_info
            .as_ref()
            .and_then(|v| v.displacement_l),
        state
            .domain
            .vehicle_info
            .as_ref()
            .and_then(|v| v.fuel_type.as_deref()),
    );
    state.dashboard_config = dashboard_config;
    state.config_path = Some(config_path);
    state.domain.storage_manager = Some(storage_manager);
    state.ai_config = ai_config;

    // Set global AI sender so background analysis tasks can send results
    let _ = AI_TX.set(obd_tx.clone());

    // If we have an obd_handle, we're connecting/connected; otherwise disconnected
    if obd_handle.is_some() {
        state.domain.connection = ConnectionState::Connecting;
    }

    let mut scan_handle: Option<tokio::task::JoinHandle<()>> = None;

    // Replay tick interval (50ms for smooth playback)
    let mut replay_interval = tokio::time::interval(Duration::from_millis(50));

    loop {
        tokio::select! {
            event = event_handler.next() => {
                match event {
                    Some(Event::Key(key)) => {
                        if let Some(msg) = key_to_message(key) {
                            state.update(msg);
                        } else {
                            handle_key(&mut state, key, &dtc_scenario);
                        }
                    }
                    Some(Event::Render) => {
                        tui.draw(&state, &log_buffer)?;
                    }
                    Some(Event::Tick) => {
                        state.update(Message::Tick);
                    }
                    None => break,
                }
            }
            msg = obd_rx.recv(), if !state.domain.recording.is_replaying() => {
                match msg {
                    Some(Message::VinDetected(vin)) => {
                        handle_vin_detected(&mut state, &vin, &database, &obd_tx);
                    }
                    Some(Message::NhtsaResult(vin, nhtsa)) => {
                        handle_nhtsa_result(&mut state, &vin, nhtsa, &database);
                    }
                    Some(m) => state.update(m),
                    None => break,
                }
            }
            _ = replay_interval.tick(), if state.domain.recording.is_replaying() => {
                let (frames, finished) = if let RecordingState::Replaying(ref mut controller) = state.domain.recording {
                    let frames = controller.next_frames();
                    let finished = controller.is_finished();
                    (frames, finished)
                } else {
                    (vec![], false)
                };

                for frame in frames {
                    if recording::replay::ReplayController::is_pid_frame(&frame) {
                        let pid = Pid::from_code(frame.pid_code);
                        let reading = Reading {
                            value: obd2_core::protocol::enhanced::Value::Scalar(frame.value),
                            unit: pid.unit(),
                            timestamp: Instant::now(),
                            raw_bytes: vec![],
                            source: obd2_core::protocol::enhanced::ReadingSource::Replay,
                        };
                        state.update(Message::PidUpdate(pid, reading));
                    } else if recording::replay::ReplayController::is_voltage_frame(&frame) {
                        state.update(Message::VoltageUpdate(frame.value));
                    }
                }

                if finished {
                    state.domain.recording = RecordingState::Idle;
                }
            }
        }

        // ── Handle scan/connect requests after select ────────────────────
        if state.scan_requested {
            state.scan_requested = false;
            if let Some(h) = scan_handle.take() {
                h.abort();
            }
            let (scan_tx, mut scan_rx) = mpsc::unbounded_channel::<ScanEvent>();
            let bridge_tx = obd_tx.clone();
            tokio::spawn(async move {
                while let Some(event) = scan_rx.recv().await {
                    let msg = Message::from_scan_event(event);
                    if bridge_tx.send(msg).is_err() {
                        break;
                    }
                }
            });
            scan_handle = Some(scanner::spawn_scan(
                scan_tx,
                default_baud,
                Duration::from_secs(ble_scan_secs),
            ));
        }
        if let Some(device) = state.pending_connect.take() {
            if let Some(h) = obd_handle.take() {
                h.abort();
            }
            if let Some(h) = scan_handle.take() {
                h.abort();
            }
            obd_handle = Some(spawn_connect_and_poll(
                device,
                default_baud,
                poll_ms,
                obd_tx.clone(),
                prefs_path.clone(),
                ble_scan_secs,
            ));
        }
        if state.scan_mode == ScanMode::Idle {
            if let Some(h) = scan_handle.take() {
                h.abort();
            }
        }

        if !state.running {
            break;
        }
    }

    tui.exit()?;
    if let Some(h) = obd_handle {
        h.abort();
    }
    if let Some(h) = scan_handle {
        h.abort();
    }
    tracing::info!("obd2-dash exiting");
    Ok(())
}

async fn run_headless(
    poll_ms: u64,
    obd_rx: &mut mpsc::UnboundedReceiver<Message>,
    obd_tx: mpsc::UnboundedSender<Message>,
    obd_handle: tokio::task::JoinHandle<()>,
    vehicle_info: Option<obd2_db::models::VehicleInfo>,
    thresholds: std::collections::HashMap<u8, obd2_db::models::ResolvedThreshold>,
    database: obd2_db::Database,
) -> Result<()> {
    let mut state = AppState::new(poll_ms);
    state.domain.vehicle_info = vehicle_info;
    state.domain.thresholds_cache = thresholds;
    let mut print_interval = tokio::time::interval(Duration::from_millis(500));
    let mut cycles = 0u64;

    let vehicle_name = state
        .domain
        .vehicle_info
        .as_ref()
        .map(|v| v.display_name())
        .unwrap_or_else(|| "Unknown Vehicle".to_string());

    println!("obd2-dash headless mode -- {}", vehicle_name);
    println!("-------------------------------------------");

    loop {
        tokio::select! {
            msg = obd_rx.recv() => {
                match msg {
                    Some(Message::VinDetected(vin)) => {
                        handle_vin_detected(&mut state, &vin, &database, &obd_tx);
                    }
                    Some(Message::NhtsaResult(vin, nhtsa)) => {
                        handle_nhtsa_result(&mut state, &vin, nhtsa, &database);
                    }
                    Some(m) => state.update(m),
                    None => break,
                }
            }
            _ = print_interval.tick() => {
                cycles += 1;
                let rpm = state.domain.vehicle.rpm;
                let speed = state.domain.vehicle.speed;
                let temp = state.domain.vehicle.coolant_temp;
                let load = state.domain.vehicle.engine_load;
                let volts = state.domain.vehicle.battery_voltage;

                let alert_str = if state.domain.active_alerts.is_empty() {
                    String::new()
                } else {
                    format!("  ALERTS: {}", state.domain.active_alerts.iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", "))
                };

                println!(
                    "[{:>4}] RPM: {:>6.0}  Speed: {:>5.0} km/h  Coolant: {:>5.1}\u{00B0}C  Load: {:>5.1}%  Batt: {:.1}V  [{}]{}",
                    cycles,
                    rpm.unwrap_or(0.0),
                    speed.unwrap_or(0.0),
                    temp.unwrap_or(0.0),
                    load.unwrap_or(0.0),
                    volts.unwrap_or(0.0),
                    match &state.domain.connection {
                        ConnectionState::Connected => "connected",
                        ConnectionState::Connecting => "connecting",
                        ConnectionState::Disconnected => "disconnected",
                        ConnectionState::Error(_) => "error",
                    },
                    alert_str,
                );
            }
            _ = tokio::signal::ctrl_c() => {
                println!("\nShutting down...");
                break;
            }
        }
    }

    obd_handle.abort();
    tracing::info!("obd2-dash headless exiting");
    Ok(())
}

/// Handle a VIN detected from the OBD2 connection.
fn handle_vin_detected(
    state: &mut AppState,
    vin: &str,
    database: &obd2_db::Database,
    tx: &mpsc::UnboundedSender<Message>,
) {
    tracing::info!("VIN detected: {}", vin);

    // 1. Try exact VIN match
    match database.get_vehicle(vin) {
        Ok(Some(info)) => {
            tracing::info!("Exact VIN match: {}", info.display_name());
            apply_vehicle_info(state, info, database);
            return;
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("Database error looking up VIN {}: {}", vin, e);
            return;
        }
    }

    // 2. Try VIN pattern match
    match database.get_vehicle_by_vin_pattern(vin) {
        Ok(Some(info)) => {
            tracing::info!(
                "VIN pattern match: {} (from seeded data)",
                info.display_name()
            );
            if let Err(e) = database.upsert_vehicle(&info) {
                tracing::warn!("Failed to save pattern-matched vehicle: {}", e);
            }
            apply_vehicle_info(state, info, database);
            return;
        }
        Ok(None) => {}
        Err(e) => {
            tracing::warn!("Database error in VIN pattern match for {}: {}", vin, e);
        }
    }

    // 3. Spawn async NHTSA VIN lookup
    tracing::info!("Spawning NHTSA VIN lookup for {}", vin);
    let vin_owned = vin.to_string();
    let tx_clone = tx.clone();
    tokio::spawn(async move {
        match nhtsa::decode_vin(&vin_owned).await {
            Ok(Some(nhtsa)) => {
                tracing::info!(
                    "NHTSA lookup: {} {} {} ({:?} {:?}L {}cyl, category={})",
                    nhtsa.year.unwrap_or(0),
                    nhtsa.make.as_deref().unwrap_or("?"),
                    nhtsa.model.as_deref().unwrap_or("?"),
                    nhtsa.fuel_type,
                    nhtsa.displacement_l,
                    nhtsa.cylinders.unwrap_or(0),
                    nhtsa.threshold_category(),
                );
                let _ = tx_clone.send(Message::NhtsaResult(vin_owned, nhtsa));
            }
            Ok(None) => {
                tracing::info!("NHTSA returned no data for VIN {}", vin_owned);
            }
            Err(e) => {
                tracing::info!("NHTSA lookup failed for {}: {}", vin_owned, e);
            }
        }
    });

    // 4. Offline VIN decode fallback
    // The new obd2-core has its own VIN decoder via Session::identify_vehicle(),
    // but for the offline fallback we just create a minimal entry.
    tracing::info!("VIN {} not recognized, creating minimal entry", vin);
    state.domain.vehicle_info = Some(obd2_db::models::VehicleInfo {
        vin: vin.to_string(),
        year: None,
        make: None,
        model: None,
        trim: None,
        engine_family_id: None,
        engine_family_code: None,
        transmission_type: None,
        drive_type: None,
        fuel_type: None,
        displacement_l: None,
        cylinders: None,
    });
}

fn handle_nhtsa_result(
    state: &mut AppState,
    vin: &str,
    nhtsa: nhtsa::NhtsaVehicle,
    database: &obd2_db::Database,
) {
    let info = nhtsa.to_vehicle_info(vin);
    if let Err(e) = database.upsert_vehicle(&info) {
        tracing::warn!("Failed to cache NHTSA vehicle: {}", e);
    }
    tracing::info!("Applying NHTSA vehicle info: {}", info.display_name());
    apply_vehicle_info(state, info, database);
}

fn apply_vehicle_info(
    state: &mut AppState,
    info: obd2_db::models::VehicleInfo,
    database: &obd2_db::Database,
) {
    let engine_family_code = info.engine_family_code.clone();

    match database.resolve_all_thresholds(
        Some(&info.vin),
        engine_family_code.as_deref(),
        &all_known_codes(),
    ) {
        Ok(thresholds) => {
            tracing::info!(
                "Re-resolved {} thresholds for VIN {}",
                thresholds.len(),
                info.vin
            );
            state.domain.thresholds_cache = thresholds;
        }
        Err(e) => {
            tracing::warn!("Failed to resolve thresholds for VIN {}: {}", info.vin, e);
        }
    }

    state
        .domain
        .fuel_economy
        .configure(info.displacement_l, info.fuel_type.as_deref());

    state.domain.vehicle_info = Some(info);
}

/// Handle keys that don't map to simple Messages.
fn handle_key(state: &mut AppState, key: crossterm::event::KeyEvent, dtc_scenario: &Arc<AtomicU8>) {
    use crossterm::event::KeyCode;

    if state.scan_mode != ScanMode::Idle {
        handle_device_picker_key(state, key);
        return;
    }

    if state.show_ai_insights {
        handle_ai_insights_key(state, key);
        return;
    }

    if state.show_debug_log {
        handle_debug_log_key(state, key);
        return;
    }

    if state.show_session_picker {
        handle_session_picker_key(state, key);
        return;
    }

    if state.domain.recording.is_replaying() {
        match key.code {
            KeyCode::Char(' ') => {
                if let RecordingState::Replaying(ref mut c) = state.domain.recording {
                    c.toggle_pause();
                }
            }
            KeyCode::Char('[') => {
                if let RecordingState::Replaying(ref mut c) = state.domain.recording {
                    c.seek_backward(30_000);
                }
            }
            KeyCode::Char(']') => {
                if let RecordingState::Replaying(ref mut c) = state.domain.recording {
                    c.seek_forward(30_000);
                }
            }
            KeyCode::Char('s') => {
                if let RecordingState::Replaying(ref mut c) = state.domain.recording {
                    c.cycle_speed();
                }
            }
            KeyCode::Esc => {
                state.domain.recording = RecordingState::Idle;
            }
            KeyCode::Char('q') => {
                state.update(Message::Quit);
            }
            _ => {}
        }
        return;
    }

    if state.edit_mode.is_some() {
        handle_edit_mode_key(state, key);
        return;
    }

    match key.code {
        KeyCode::Char('d') => {
            let prev = dtc_scenario.load(Ordering::Relaxed);
            dtc_scenario.store((prev + 1) % DTC_SCENARIO_COUNT, Ordering::Relaxed);
        }
        KeyCode::Char('p') => {
            state.paused = !state.paused;
        }
        KeyCode::Char('u') => {
            state.domain.temp_unit = match state.domain.temp_unit {
                TemperatureUnit::Celsius => TemperatureUnit::Fahrenheit,
                TemperatureUnit::Fahrenheit => TemperatureUnit::Celsius,
            };
            state.domain.speed_unit = match state.domain.speed_unit {
                SpeedUnit::Kmh => SpeedUnit::Mph,
                SpeedUnit::Mph => SpeedUnit::Kmh,
            };
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if state.domain.poll_interval_ms > 50 {
                state.domain.poll_interval_ms -= 50;
            }
        }
        KeyCode::Char('-') => {
            if state.domain.poll_interval_ms < 2000 {
                state.domain.poll_interval_ms += 50;
            }
        }
        KeyCode::Char('f') => {
            state.layout = match state.layout {
                DashboardLayout::Compact => DashboardLayout::Full,
                DashboardLayout::Full => DashboardLayout::Compact,
            };
            state.focused_panel = None;
            state.panel_selections.clear();
            state.focused_widget = None;
            state.widget_selections.clear();
            state.popup = None;
        }
        KeyCode::Char('e') => {
            if state.layout == DashboardLayout::Full && state.edit_mode.is_none() {
                state.edit_mode = Some(EditModeState::new(&state.dashboard_config));
            }
        }
        KeyCode::Char('r') => {
            if !state.domain.recording.is_replaying() {
                handle_toggle_recording(state);
            }
        }
        KeyCode::Char('R') => {
            if state.domain.recording.is_idle() {
                state.show_session_picker = true;
                state.session_picker_selected = 0;
            }
        }
        KeyCode::Char('i') => {
            if state.ai_insights.is_some() && !state.ai_analyzing {
                state.show_ai_insights = !state.show_ai_insights;
                state.ai_scroll = 0;
            } else if state.ai_config.is_some() && !state.ai_analyzing {
                start_ai_analysis(state);
            } else if state.ai_config.is_none() {
                state.popup = Some(PopupState {
                    title: "AI Analysis".to_string(),
                    body: vec![
                        "No AI provider configured.".to_string(),
                        String::new(),
                        "Create an ai.json file in the project root:".to_string(),
                        String::new(),
                        r#"  {"#.to_string(),
                        r#"    "provider": "anthropic","#.to_string(),
                        r#"    "api_key": "sk-ant-...","#.to_string(),
                        r#"    "model": "claude-sonnet-4-20250514","#.to_string(),
                        r#"    "max_tokens": 4096"#.to_string(),
                        "  }".to_string(),
                        String::new(),
                        "Supported providers: anthropic, openai".to_string(),
                    ],
                });
            }
        }
        KeyCode::Char('I') => {
            if state.ai_config.is_some() && !state.ai_analyzing {
                start_ai_analysis(state);
            }
        }
        KeyCode::Char('l') => {
            state.show_debug_log = true;
            state.debug_log_scroll = 0;
        }
        KeyCode::Char('s') | KeyCode::Char('S')
            if !state.domain.recording.is_replaying() && state.edit_mode.is_none() =>
        {
            state.scan_mode = ScanMode::Scanning;
            state.scan_devices.clear();
            state.scan_selected = 0;
            state.scan_requested = true;
        }
        KeyCode::Tab => {
            if state.layout == DashboardLayout::Full {
                state.popup = None;
                let count = state.dashboard_config.widget_count();
                if count > 0 {
                    state.focused_widget = Some(match state.focused_widget {
                        None => 0,
                        Some(n) => (n + 1) % count,
                    });
                    state.focused_panel = state.focused_widget;
                }
            }
        }
        KeyCode::BackTab => {
            if state.layout == DashboardLayout::Full {
                state.popup = None;
                let count = state.dashboard_config.widget_count();
                if count > 0 {
                    state.focused_widget = Some(match state.focused_widget {
                        None => count - 1,
                        Some(0) => count - 1,
                        Some(n) => n - 1,
                    });
                    state.focused_panel = state.focused_widget;
                }
            }
        }
        KeyCode::Down => {
            if state.layout == DashboardLayout::Full && state.popup.is_none() {
                if let Some(widget_idx) = state.focused_widget {
                    let count = widget_item_count(widget_idx, state);
                    if count > 0 {
                        let current = state.widget_selections.get(&widget_idx).copied();
                        let next = match current {
                            None => 0,
                            Some(n) => (n + 1) % count,
                        };
                        state.widget_selections.insert(widget_idx, next);
                        if let Some(pi) = state.focused_panel {
                            state.panel_selections.insert(pi, next);
                        }
                    }
                }
            }
        }
        KeyCode::Up => {
            if state.layout == DashboardLayout::Full && state.popup.is_none() {
                if let Some(widget_idx) = state.focused_widget {
                    let count = widget_item_count(widget_idx, state);
                    if count > 0 {
                        let current = state.widget_selections.get(&widget_idx).copied();
                        let next = match current {
                            None => count - 1,
                            Some(0) => count - 1,
                            Some(n) => n - 1,
                        };
                        state.widget_selections.insert(widget_idx, next);
                        if let Some(pi) = state.focused_panel {
                            state.panel_selections.insert(pi, next);
                        }
                    }
                }
            }
        }
        KeyCode::Enter => {
            if state.layout == DashboardLayout::Full && state.popup.is_none() {
                if let Some(widget_idx) = state.focused_widget {
                    if let Some(&item_idx) = state.widget_selections.get(&widget_idx) {
                        state.popup = widget_build_popup(widget_idx, item_idx, state);
                    }
                }
            }
        }
        KeyCode::Esc => {
            if state.popup.is_some() {
                state.popup = None;
            } else if let Some(widget_idx) = state.focused_widget {
                if state.widget_selections.contains_key(&widget_idx) {
                    state.widget_selections.remove(&widget_idx);
                    if let Some(pi) = state.focused_panel {
                        state.panel_selections.remove(&pi);
                    }
                } else {
                    state.focused_widget = None;
                    state.focused_panel = None;
                }
            } else {
                state.update(Message::Quit);
            }
        }
        _ => {}
    }
}

fn handle_edit_mode_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    let edit = state.edit_mode.as_mut().unwrap();

    match &edit.phase {
        EditPhase::Browse => match key.code {
            KeyCode::Char('a') => edit.start_add(),
            KeyCode::Char('x') => edit.delete_at_cursor(),
            KeyCode::Char('s') => {
                state.dashboard_config = edit.working_config.clone();
                if let Some(ref path) = state.config_path {
                    if let Err(e) = state.dashboard_config.save(path) {
                        tracing::warn!("Failed to save dashboard config: {}", e);
                    }
                }
                state.edit_mode = None;
            }
            KeyCode::Up => edit.cursor_prev(),
            KeyCode::Down => edit.cursor_next(),
            KeyCode::Esc => {
                state.edit_mode = None;
            }
            _ => {}
        },
        EditPhase::CategoryPicker { .. } => match key.code {
            KeyCode::Up => edit.picker_up(),
            KeyCode::Down => edit.picker_down(),
            KeyCode::Enter => edit.select_category(),
            KeyCode::Esc => {
                if !edit.go_back() {
                    state.edit_mode = None;
                }
            }
            _ => {}
        },
        EditPhase::WidgetPicker { .. } => match key.code {
            KeyCode::Up => edit.picker_up(),
            KeyCode::Down => edit.picker_down(),
            KeyCode::Enter => edit.select_widget(),
            KeyCode::Esc => {
                edit.go_back();
            }
            _ => {}
        },
        EditPhase::SizePicker { .. } => match key.code {
            KeyCode::Up => edit.picker_up(),
            KeyCode::Down => edit.picker_down(),
            KeyCode::Enter => edit.select_size(),
            KeyCode::Esc => {
                edit.go_back();
            }
            _ => {}
        },
    }
}

fn handle_session_picker_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    let session_count = state
        .domain
        .storage_manager
        .as_ref()
        .map(|s| s.index.sessions.len())
        .unwrap_or(0);

    match key.code {
        KeyCode::Up => {
            if state.session_picker_selected > 0 {
                state.session_picker_selected -= 1;
            }
        }
        KeyCode::Down => {
            if session_count > 0 && state.session_picker_selected < session_count - 1 {
                state.session_picker_selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(ref storage) = state.domain.storage_manager {
                let sessions = storage.index.sessions_sorted();
                if let Some(session) = sessions.get(state.session_picker_selected) {
                    match recording::reader::read_recording(&session.file_path) {
                        Ok((_header, frames)) => {
                            let controller = recording::replay::ReplayController::new(
                                (*session).clone(),
                                frames,
                            );
                            state.domain.recording = RecordingState::Replaying(controller);
                            tracing::info!("Starting replay of session {}", session.session_id);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to read recording: {}", e);
                            state.domain.last_error =
                                Some(format!("Failed to load recording: {}", e));
                        }
                    }
                }
            }
            state.show_session_picker = false;
        }
        KeyCode::Char('d') => {
            if let Some(ref mut storage) = state.domain.storage_manager {
                let sessions = storage.index.sessions_sorted();
                if let Some(session) = sessions.get(state.session_picker_selected) {
                    let sid = session.session_id.clone();
                    if let Err(e) = storage.delete_session(&sid) {
                        tracing::warn!("Failed to delete session: {}", e);
                    }
                    let new_count = storage.index.sessions.len();
                    if new_count > 0 && state.session_picker_selected >= new_count {
                        state.session_picker_selected = new_count - 1;
                    }
                }
            }
        }
        KeyCode::Esc => {
            state.show_session_picker = false;
        }
        _ => {}
    }
}

fn handle_device_picker_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    let device_count = state.scan_devices.len();

    match key.code {
        KeyCode::Up => {
            if state.scan_selected > 0 {
                state.scan_selected -= 1;
            }
        }
        KeyCode::Down => {
            if device_count > 0 && state.scan_selected < device_count - 1 {
                state.scan_selected += 1;
            }
        }
        KeyCode::Enter => {
            if let Some(dev) = state.scan_devices.get(state.scan_selected) {
                state.update(Message::StartConnect(dev.kind.clone()));
            }
        }
        KeyCode::Esc => {
            state.scan_mode = ScanMode::Idle;
        }
        _ => {}
    }
}

fn handle_debug_log_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    match key.code {
        KeyCode::Up => {
            state.debug_log_scroll = state.debug_log_scroll.saturating_add(1);
        }
        KeyCode::Down => {
            state.debug_log_scroll = state.debug_log_scroll.saturating_sub(1);
        }
        KeyCode::PageUp => {
            state.debug_log_scroll = state.debug_log_scroll.saturating_add(20);
        }
        KeyCode::PageDown => {
            state.debug_log_scroll = state.debug_log_scroll.saturating_sub(20);
        }
        KeyCode::Home => {
            state.debug_log_scroll = usize::MAX;
        }
        KeyCode::End => {
            state.debug_log_scroll = 0;
        }
        KeyCode::Char('l') | KeyCode::Esc => {
            state.show_debug_log = false;
        }
        KeyCode::Char('q') => {
            state.update(Message::Quit);
        }
        _ => {}
    }
}

fn handle_ai_insights_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    match key.code {
        KeyCode::Up => {
            state.ai_scroll = state.ai_scroll.saturating_add(1);
        }
        KeyCode::Down => {
            state.ai_scroll = state.ai_scroll.saturating_sub(1);
        }
        KeyCode::PageUp => {
            state.ai_scroll = state.ai_scroll.saturating_add(20);
        }
        KeyCode::PageDown => {
            state.ai_scroll = state.ai_scroll.saturating_sub(20);
        }
        KeyCode::Home => {
            state.ai_scroll = usize::MAX;
        }
        KeyCode::End => {
            state.ai_scroll = 0;
        }
        KeyCode::Char('i') | KeyCode::Esc => {
            state.show_ai_insights = false;
        }
        KeyCode::Char('I') => {
            if state.ai_config.is_some() && !state.ai_analyzing {
                state.show_ai_insights = false;
                start_ai_analysis(state);
            }
        }
        KeyCode::Char('q') => {
            state.update(Message::Quit);
        }
        _ => {}
    }
}

fn start_ai_analysis(state: &mut AppState) {
    use ai::SessionSummary;

    let config = match state.ai_config.clone() {
        Some(c) => c,
        None => return,
    };

    let summary = SessionSummary::from_live(&state.domain);
    let session_id = state
        .domain
        .vehicle_info
        .as_ref()
        .map(|v| v.vin.clone())
        .unwrap_or_else(|| "live".to_string());

    state.ai_analyzing = true;
    state.show_ai_insights = true;
    state.ai_scroll = 0;

    AI_ANALYSIS_PENDING.store(true, Ordering::SeqCst);

    let _ = AI_TX.get().map(|tx| {
        let tx = tx.clone();
        tokio::spawn(async move {
            let client = AiClient::new();
            match client.analyze(&config, &summary, &session_id).await {
                Ok(insights) => {
                    let _ = tx.send(Message::AiAnalysisComplete(insights));
                }
                Err(e) => {
                    let _ = tx.send(Message::AiAnalysisError(format!("{}", e)));
                }
            }
        });
    });
}

static AI_TX: std::sync::OnceLock<mpsc::UnboundedSender<Message>> = std::sync::OnceLock::new();
static AI_ANALYSIS_PENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn handle_toggle_recording(state: &mut AppState) {
    match &state.domain.recording {
        RecordingState::Idle => {
            if let Some(ref storage) = state.domain.storage_manager {
                let session_id = uuid::Uuid::new_v4().to_string();
                let vin = state.domain.vehicle_info.as_ref().map(|v| v.vin.clone());
                let vehicle_name = state.domain.vehicle_info.as_ref().map(|v| v.display_name());
                let recordings_dir = storage.recordings_dir().to_path_buf();

                match recording::writer::RecordingWriter::new(
                    &recordings_dir,
                    &session_id,
                    vin,
                    vehicle_name,
                    state.domain.poll_interval_ms,
                ) {
                    Ok(writer) => {
                        tracing::info!("Recording started: {}", session_id);
                        state.domain.recording = RecordingState::Recording {
                            writer,
                            start_instant: Instant::now(),
                        };
                    }
                    Err(e) => {
                        tracing::warn!("Failed to start recording: {}", e);
                        state.domain.last_error = Some(format!("Failed to start recording: {}", e));
                    }
                }
            }
        }
        RecordingState::Recording { .. } => {
            let old = std::mem::replace(&mut state.domain.recording, RecordingState::Idle);
            if let RecordingState::Recording {
                writer,
                start_instant,
            } = old
            {
                let duration_secs = start_instant.elapsed().as_secs();
                let frame_count = writer.frame_count;
                let session_id = writer.session_id.clone();

                match writer.finish() {
                    Ok(path) => {
                        let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

                        let entry = recording::index::SessionEntry {
                            session_id: session_id.clone(),
                            start_time: chrono::Utc::now()
                                - chrono::Duration::seconds(duration_secs as i64),
                            vin: state.domain.vehicle_info.as_ref().map(|v| v.vin.clone()),
                            vehicle_name: state
                                .domain
                                .vehicle_info
                                .as_ref()
                                .map(|v| v.display_name()),
                            duration_secs,
                            frame_count,
                            file_path: path,
                            file_size_bytes: file_size,
                            compressed: false,
                        };

                        if let Some(ref mut storage) = state.domain.storage_manager {
                            if let Err(e) = storage.register_session(entry) {
                                tracing::warn!("Failed to register session: {}", e);
                            }
                            if let Err(e) = storage.run_maintenance() {
                                tracing::warn!("Storage maintenance error: {}", e);
                            }
                        }

                        tracing::info!(
                            "Recording stopped: {} ({} frames, {}s)",
                            session_id,
                            frame_count,
                            duration_secs,
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Error finishing recording: {}", e);
                        state.domain.last_error = Some(format!("Error finishing recording: {}", e));
                    }
                }
            }
        }
        RecordingState::Replaying(_) => {}
    }
}

fn widget_item_count(flat_idx: usize, state: &AppState) -> usize {
    if let Some(slot) = state.dashboard_config.widget_at(flat_idx) {
        widget_kind_item_count(slot.kind, state)
    } else {
        0
    }
}

fn widget_kind_item_count(kind: widget::WidgetKind, state: &AppState) -> usize {
    use widget::WidgetKind;
    match kind {
        WidgetKind::GaugesAndEngine => tui_panel::panel_item_count(0, state),
        WidgetKind::TemperaturesPanel => tui_panel::panel_item_count(1, state),
        WidgetKind::FuelSystemPanel => tui_panel::panel_item_count(2, state),
        WidgetKind::SystemInfoPanel => tui_panel::panel_item_count(3, state),
        WidgetKind::DtcPanel => tui_panel::panel_item_count(4, state),
        WidgetKind::FuelEconomyPanel => tui_panel::panel_item_count(5, state),
        WidgetKind::EnhancedPidsPanel => tui_panel::panel_item_count(6, state),
        WidgetKind::O2SensorsPanel => tui_panel::panel_item_count(7, state),
        _ => 0,
    }
}

fn widget_build_popup(
    flat_idx: usize,
    item_idx: usize,
    state: &AppState,
) -> Option<app::PopupState> {
    if let Some(slot) = state.dashboard_config.widget_at(flat_idx) {
        widget_kind_build_popup(slot.kind, item_idx, state)
    } else {
        None
    }
}

fn widget_kind_build_popup(
    kind: widget::WidgetKind,
    item_idx: usize,
    state: &AppState,
) -> Option<app::PopupState> {
    use widget::WidgetKind;
    match kind {
        WidgetKind::GaugesAndEngine => tui_panel::build_popup(0, item_idx, state),
        WidgetKind::TemperaturesPanel => tui_panel::build_popup(1, item_idx, state),
        WidgetKind::FuelSystemPanel => tui_panel::build_popup(2, item_idx, state),
        WidgetKind::SystemInfoPanel => tui_panel::build_popup(3, item_idx, state),
        WidgetKind::DtcPanel => tui_panel::build_popup(4, item_idx, state),
        WidgetKind::FuelEconomyPanel => tui_panel::build_popup(5, item_idx, state),
        WidgetKind::EnhancedPidsPanel => tui_panel::build_popup(6, item_idx, state),
        WidgetKind::O2SensorsPanel => tui_panel::build_popup(7, item_idx, state),
        _ => None,
    }
}

fn auto_detect_port() -> Result<String> {
    let ports = serialport::available_ports()
        .map_err(|e| anyhow::anyhow!("failed to enumerate serial ports: {e}"))?;

    if ports.is_empty() {
        anyhow::bail!(
            "no serial ports found. Connect an ELM327 adapter or specify --port manually."
        );
    }

    for port in &ports {
        match &port.port_type {
            serialport::SerialPortType::UsbPort(info) => {
                tracing::info!(
                    "Found USB serial: {} ({})",
                    port.port_name,
                    info.product.as_deref().unwrap_or("unknown")
                );
                return Ok(port.port_name.clone());
            }
            _ => continue,
        }
    }

    tracing::warn!(
        "No USB serial ports found, using first available: {}",
        ports[0].port_name
    );
    Ok(ports[0].port_name.clone())
}
