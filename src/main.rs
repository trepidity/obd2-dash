mod app;
mod db;
mod debug_log;
mod diagnostics;
mod driving;
mod fuel_economy;
mod obd2;
mod recording;
mod tui;
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

use app::{AppState, ConnectionState, DashboardLayout, Message, ScanMode, SpeedUnit, TemperatureUnit};
use db::models::MockVehicleProfile;
use obd2::connection_prefs::ConnectionPrefs;
use obd2::dtc::DTC_SCENARIO_COUNT;
use obd2::scanner::DeviceKind;
use obd2::{mock::MockObd2, Obd2Connection, Pid};
use recording::RecordingState;
use recording::storage::{StorageConfig, StorageManager};
use widget::config::DashboardConfig;
use widget::edit_mode::{EditModeState, EditPhase};
use tui::{
    event::{key_to_message, Event, EventHandler},
    panel as tui_panel,
    Tui,
};

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

    /// Mock vehicle profile: mini, chevy, or generic (default)
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

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
    let database = db::Database::open(&db_path)?;
    db::seed::seed_all(&database)?;
    tracing::info!("Database initialized at {}", db_path.display());

    // Resolve mock vehicle profile
    let mock_profile = match cli.mock_vehicle.to_lowercase().as_str() {
        "mini" => MockVehicleProfile::mini_2006(),
        "chevy" | "duramax" => MockVehicleProfile::chevy_2004(),
        _ => MockVehicleProfile::generic(),
    };

    // Look up vehicle info if using a known profile
    let vehicle_info = database.get_vehicle(&mock_profile.vin)?;
    let engine_family_code = vehicle_info
        .as_ref()
        .and_then(|v| v.engine_family_code.clone());

    // Resolve thresholds for all known PIDs
    let thresholds = database.resolve_all_thresholds(
        vehicle_info.as_ref().map(|v| v.vin.as_str()),
        engine_family_code.as_deref(),
        &Pid::all_known_codes(),
    )?;

    tracing::info!(
        "Resolved {} thresholds for vehicle {:?}",
        thresholds.len(),
        vehicle_info.as_ref().map(|v| v.display_name())
    );

    // Shared DTC scenario selector (mock only — incremented by 'd' key)
    let dtc_scenario = Arc::new(AtomicU8::new(0));

    // Channel for OBD2 poll results → main loop
    let (obd_tx, mut obd_rx) = mpsc::unbounded_channel::<Message>();

    // Connection prefs path (same directory as dashboard config)
    let prefs_path = PathBuf::from("connection.json");
    let connection_prefs = ConnectionPrefs::load(&prefs_path);

    // Spawn OBD2 polling task (or start disconnected)
    let poll_ms = cli.poll_ms;
    let obd_handle: Option<tokio::task::JoinHandle<()>> = if cli.mock {
        let mock = MockObd2::with_profile(mock_profile, Arc::clone(&dtc_scenario));
        let mock_info = MockObd2::mock_adapter_info();
        let tx_clone = obd_tx.clone();
        let handle = spawn_obd2_poll(
            Box::new(mock) as Box<dyn Obd2Connection>,
            poll_ms,
            obd_tx.clone(),
        );
        // Send mock adapter info after spawning
        let _ = tx_clone.send(Message::AdapterDetected(mock_info));
        Some(handle)
    } else if cli.ble {
        // Explicit --ble flag: connect via BLE immediately
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
    } else if let Some(ref port) = cli.port {
        // Explicit --port flag: connect via serial immediately
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
        // Try last-used device
        tracing::info!("Reconnecting to last device: {:?}", last_device);
        Some(spawn_connect_and_poll(
            last_device.clone(),
            cli.baud,
            poll_ms,
            obd_tx.clone(),
            prefs_path.clone(),
            cli.ble_scan_secs,
        ))
    } else {
        // No flags, no saved device — start disconnected
        tracing::info!("No device specified, starting disconnected (press 's' to scan)");
        None
    };

    let headless = cli.headless || !std::io::stdout().is_terminal();

    // Load dashboard config
    let config_path = PathBuf::from(&cli.config);
    let dashboard_config = DashboardConfig::load(&config_path);

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
            obd_handle.expect("headless mode requires --mock, --port, or --ble"),
            vehicle_info,
            thresholds,
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
        )
        .await
    }
}

async fn run_tui(
    poll_ms: u64,
    obd_rx: &mut mpsc::UnboundedReceiver<Message>,
    mut obd_handle: Option<tokio::task::JoinHandle<()>>,
    obd_tx: mpsc::UnboundedSender<Message>,
    vehicle_info: Option<db::models::VehicleInfo>,
    thresholds: std::collections::HashMap<u8, db::models::ResolvedThreshold>,
    dtc_scenario: Arc<AtomicU8>,
    dashboard_config: DashboardConfig,
    config_path: PathBuf,
    storage_manager: StorageManager,
    prefs_path: PathBuf,
    default_baud: u32,
    ble_scan_secs: u64,
    log_buffer: debug_log::LogBuffer,
) -> Result<()> {
    let mut tui = Tui::new()?;
    tui.enter()?;

    let mut event_handler = EventHandler::new(poll_ms, 50);
    let mut state = AppState::new(poll_ms);
    state.vehicle_info = vehicle_info;
    state.thresholds_cache = thresholds;
    state.fuel_economy.configure(
        state.vehicle_info.as_ref().and_then(|v| v.displacement_l),
        state.vehicle_info.as_ref().and_then(|v| v.fuel_type.as_deref()),
    );
    state.dashboard_config = dashboard_config;
    state.config_path = Some(config_path);
    state.storage_manager = Some(storage_manager);

    // If we have an obd_handle, we're connecting/connected; otherwise disconnected
    if obd_handle.is_some() {
        state.connection = ConnectionState::Connecting;
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
            msg = obd_rx.recv(), if !state.recording.is_replaying() => {
                match msg {
                    Some(m) => state.update(m),
                    None => break,
                }
            }
            _ = replay_interval.tick(), if state.recording.is_replaying() => {
                // Collect replay frames first, then process them to avoid borrow conflict
                let (frames, finished) = if let RecordingState::Replaying(ref mut controller) = state.recording {
                    let frames = controller.next_frames();
                    let finished = controller.is_finished();
                    (frames, finished)
                } else {
                    (vec![], false)
                };

                // Now process the collected frames
                for frame in frames {
                    if recording::replay::ReplayController::is_pid_frame(&frame) {
                        if let Some(pid) = Pid::from_code(frame.pid_code) {
                            let reading = obd2::PidReading::new(frame.value, pid.unit());
                            state.update(Message::PidUpdate(pid, reading));
                        }
                    } else if recording::replay::ReplayController::is_voltage_frame(&frame) {
                        state.update(Message::VoltageUpdate(frame.value));
                    }
                }

                if finished {
                    state.recording = RecordingState::Idle;
                }
            }
        }

        // ── Handle scan/connect requests after select ────────────────────
        if state.scan_requested {
            state.scan_requested = false;
            if let Some(h) = scan_handle.take() {
                h.abort();
            }
            scan_handle = Some(obd2::scanner::spawn_scan(
                obd_tx.clone(),
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
    obd_handle: tokio::task::JoinHandle<()>,
    vehicle_info: Option<db::models::VehicleInfo>,
    thresholds: std::collections::HashMap<u8, db::models::ResolvedThreshold>,
) -> Result<()> {
    let mut state = AppState::new(poll_ms);
    state.vehicle_info = vehicle_info;
    state.thresholds_cache = thresholds;
    let mut print_interval = tokio::time::interval(Duration::from_millis(500));
    let mut cycles = 0u64;

    let vehicle_name = state
        .vehicle_info
        .as_ref()
        .map(|v| v.display_name())
        .unwrap_or_else(|| "Unknown Vehicle".to_string());

    println!("obd2-dash headless mode — {}", vehicle_name);
    println!("-------------------------------------------");

    loop {
        tokio::select! {
            msg = obd_rx.recv() => {
                match msg {
                    Some(m) => state.update(m),
                    None => break,
                }
            }
            _ = print_interval.tick() => {
                cycles += 1;
                let rpm = state.vehicle.rpm.as_ref().map(|r| r.value);
                let speed = state.vehicle.speed.as_ref().map(|r| r.value);
                let temp = state.vehicle.coolant_temp.as_ref().map(|r| r.value);
                let load = state.vehicle.engine_load.as_ref().map(|r| r.value);
                let volts = state.vehicle.battery_voltage;

                let alert_str = if state.active_alerts.is_empty() {
                    String::new()
                } else {
                    format!("  ALERTS: {}", state.active_alerts.iter()
                        .map(|a| a.to_string())
                        .collect::<Vec<_>>()
                        .join(", "))
                };

                println!(
                    "[{:>4}] RPM: {:>6.0}  Speed: {:>5.0} km/h  Coolant: {:>5.1}°C  Load: {:>5.1}%  Batt: {:.1}V  [{}]{}",
                    cycles,
                    rpm.unwrap_or(0.0),
                    speed.unwrap_or(0.0),
                    temp.unwrap_or(0.0),
                    load.unwrap_or(0.0),
                    volts.unwrap_or(0.0),
                    match &state.connection {
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

/// Handle keys that don't map to simple Messages.
fn handle_key(state: &mut AppState, key: crossterm::event::KeyEvent, dtc_scenario: &Arc<AtomicU8>) {
    use crossterm::event::KeyCode;

    // ─── Device picker mode ──────────────────────────────────────────────
    if state.scan_mode != ScanMode::Idle {
        handle_device_picker_key(state, key);
        return;
    }

    // ─── Debug log viewer ─────────────────────────────────────────────────
    if state.show_debug_log {
        handle_debug_log_key(state, key);
        return;
    }

    // ─── Session picker mode ─────────────────────────────────────────────
    if state.show_session_picker {
        handle_session_picker_key(state, key);
        return;
    }

    // ─── Replay mode keys ────────────────────────────────────────────────
    if state.recording.is_replaying() {
        match key.code {
            KeyCode::Char(' ') => {
                if let RecordingState::Replaying(ref mut c) = state.recording {
                    c.toggle_pause();
                }
            }
            KeyCode::Char('[') => {
                if let RecordingState::Replaying(ref mut c) = state.recording {
                    c.seek_backward(30_000);
                }
            }
            KeyCode::Char(']') => {
                if let RecordingState::Replaying(ref mut c) = state.recording {
                    c.seek_forward(30_000);
                }
            }
            KeyCode::Char('s') => {
                if let RecordingState::Replaying(ref mut c) = state.recording {
                    c.cycle_speed();
                }
            }
            KeyCode::Esc => {
                state.recording = RecordingState::Idle;
            }
            KeyCode::Char('q') => {
                state.update(Message::Quit);
            }
            _ => {}
        }
        return;
    }

    // ─── Edit mode keys ──────────────────────────────────────────────────
    if state.edit_mode.is_some() {
        handle_edit_mode_key(state, key);
        return;
    }

    // ─── Normal mode keys ────────────────────────────────────────────────
    match key.code {
        KeyCode::Char('d') => {
            let prev = dtc_scenario.load(Ordering::Relaxed);
            dtc_scenario.store((prev + 1) % DTC_SCENARIO_COUNT, Ordering::Relaxed);
        }
        KeyCode::Char('p') => {
            state.paused = !state.paused;
        }
        KeyCode::Char('u') => {
            state.temp_unit = match state.temp_unit {
                TemperatureUnit::Celsius => TemperatureUnit::Fahrenheit,
                TemperatureUnit::Fahrenheit => TemperatureUnit::Celsius,
            };
            state.speed_unit = match state.speed_unit {
                SpeedUnit::Kmh => SpeedUnit::Mph,
                SpeedUnit::Mph => SpeedUnit::Kmh,
            };
        }
        KeyCode::Char('+') | KeyCode::Char('=') => {
            if state.poll_interval_ms > 50 {
                state.poll_interval_ms -= 50;
            }
        }
        KeyCode::Char('-') => {
            if state.poll_interval_ms < 2000 {
                state.poll_interval_ms += 50;
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
            // Toggle recording
            if !state.recording.is_replaying() {
                handle_toggle_recording(state);
            }
        }
        KeyCode::Char('R') => {
            // Open session picker for replay
            if state.recording.is_idle() {
                state.show_session_picker = true;
                state.session_picker_selected = 0;
            }
        }
        KeyCode::Char('l') => {
            state.show_debug_log = true;
            state.debug_log_scroll = 0;
        }
        KeyCode::Char('s') | KeyCode::Char('S') if !state.recording.is_replaying() && state.edit_mode.is_none() => {
            // Start device scan
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
                    // Sync legacy focused_panel for backward compat
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
                        // Sync legacy
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

/// Handle keys in edit mode.
fn handle_edit_mode_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    let edit = state.edit_mode.as_mut().unwrap();

    match &edit.phase {
        EditPhase::Browse => match key.code {
            KeyCode::Char('a') => edit.start_add(),
            KeyCode::Char('x') => edit.delete_at_cursor(),
            KeyCode::Char('s') => {
                // Save config
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

/// Handle keys in session picker.
fn handle_session_picker_key(state: &mut AppState, key: crossterm::event::KeyEvent) {
    use crossterm::event::KeyCode;

    let session_count = state
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
            // Start replay of selected session
            if let Some(ref storage) = state.storage_manager {
                let sessions = storage.index.sessions_sorted();
                if let Some(session) = sessions.get(state.session_picker_selected) {
                    match recording::reader::read_recording(&session.file_path) {
                        Ok((_header, frames)) => {
                            let controller =
                                recording::replay::ReplayController::new((*session).clone(), frames);
                            state.recording = RecordingState::Replaying(controller);
                            tracing::info!(
                                "Starting replay of session {}",
                                session.session_id
                            );
                        }
                        Err(e) => {
                            tracing::warn!("Failed to read recording: {}", e);
                            state.last_error =
                                Some(format!("Failed to load recording: {}", e));
                        }
                    }
                }
            }
            state.show_session_picker = false;
        }
        KeyCode::Char('d') => {
            // Delete selected session
            if let Some(ref mut storage) = state.storage_manager {
                let sessions = storage.index.sessions_sorted();
                if let Some(session) = sessions.get(state.session_picker_selected) {
                    let sid = session.session_id.clone();
                    if let Err(e) = storage.delete_session(&sid) {
                        tracing::warn!("Failed to delete session: {}", e);
                    }
                    // Adjust selection
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

/// Handle keys in device picker (scan mode).
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

/// Handle keys in debug log viewer.
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
            // Scroll to the top (max offset)
            state.debug_log_scroll = usize::MAX;
        }
        KeyCode::End => {
            // Scroll to bottom (auto-scroll)
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

/// Toggle recording on/off.
fn handle_toggle_recording(state: &mut AppState) {
    match &state.recording {
        RecordingState::Idle => {
            // Start recording
            if let Some(ref storage) = state.storage_manager {
                let session_id = uuid::Uuid::new_v4().to_string();
                let vin = state.vehicle_info.as_ref().map(|v| v.vin.clone());
                let vehicle_name = state.vehicle_info.as_ref().map(|v| v.display_name());
                let recordings_dir = storage.recordings_dir().to_path_buf();

                match recording::writer::RecordingWriter::new(
                    &recordings_dir,
                    &session_id,
                    vin,
                    vehicle_name,
                    state.poll_interval_ms,
                ) {
                    Ok(writer) => {
                        tracing::info!("Recording started: {}", session_id);
                        state.recording = RecordingState::Recording {
                            writer,
                            start_instant: Instant::now(),
                        };
                    }
                    Err(e) => {
                        tracing::warn!("Failed to start recording: {}", e);
                        state.last_error = Some(format!("Failed to start recording: {}", e));
                    }
                }
            }
        }
        RecordingState::Recording { .. } => {
            // Stop recording — take ownership of writer to finish it
            let old = std::mem::replace(&mut state.recording, RecordingState::Idle);
            if let RecordingState::Recording {
                writer,
                start_instant,
            } = old
            {
                let duration_secs = start_instant.elapsed().as_secs();
                let frame_count = writer.frame_count;
                let session_id = writer.session_id.clone();
                let _file_path = writer.file_path.clone();

                match writer.finish() {
                    Ok(path) => {
                        let file_size = std::fs::metadata(&path)
                            .map(|m| m.len())
                            .unwrap_or(0);

                        let entry = recording::index::SessionEntry {
                            session_id: session_id.clone(),
                            start_time: chrono::Utc::now()
                                - chrono::Duration::seconds(duration_secs as i64),
                            vin: state.vehicle_info.as_ref().map(|v| v.vin.clone()),
                            vehicle_name: state
                                .vehicle_info
                                .as_ref()
                                .map(|v| v.display_name()),
                            duration_secs,
                            frame_count,
                            file_path: path,
                            file_size_bytes: file_size,
                            compressed: false,
                        };

                        if let Some(ref mut storage) = state.storage_manager {
                            if let Err(e) = storage.register_session(entry) {
                                tracing::warn!("Failed to register session: {}", e);
                            }
                            // Run storage maintenance (compress/trim)
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
                        state.last_error =
                            Some(format!("Error finishing recording: {}", e));
                    }
                }
            }
        }
        RecordingState::Replaying(_) => {
            // Can't toggle recording while replaying
        }
    }
}

/// Get item count for a widget at a flat index (for navigation).
fn widget_item_count(flat_idx: usize, state: &AppState) -> usize {
    if let Some(slot) = state.dashboard_config.widget_at(flat_idx) {
        widget_kind_item_count(slot.kind, state)
    } else {
        0
    }
}

/// Get the number of selectable items within a widget kind.
fn widget_kind_item_count(kind: widget::WidgetKind, state: &AppState) -> usize {
    use widget::WidgetKind;
    match kind {
        WidgetKind::GaugesAndEngine => tui_panel::panel_item_count(0, state),
        WidgetKind::TemperaturesPanel => tui_panel::panel_item_count(1, state),
        WidgetKind::FuelSystemPanel => tui_panel::panel_item_count(2, state),
        WidgetKind::SystemInfoPanel => tui_panel::panel_item_count(3, state),
        WidgetKind::DtcPanel => tui_panel::panel_item_count(4, state),
        WidgetKind::FuelEconomyPanel => tui_panel::panel_item_count(5, state),
        // Individual widgets don't have sub-items
        _ => 0,
    }
}

/// Build a popup for a widget at a flat index.
fn widget_build_popup(flat_idx: usize, item_idx: usize, state: &AppState) -> Option<app::PopupState> {
    if let Some(slot) = state.dashboard_config.widget_at(flat_idx) {
        widget_kind_build_popup(slot.kind, item_idx, state)
    } else {
        None
    }
}

/// Build a popup for a specific widget kind.
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
        _ => None,
    }
}

/// Spawn the OBD2 polling task. Queries each PID sequentially in a loop.
fn spawn_obd2_poll(
    mut conn: Box<dyn Obd2Connection>,
    poll_ms: u64,
    tx: mpsc::UnboundedSender<Message>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // Initialize
        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connecting));

        match conn.initialize().await {
            Ok(_) => {
                let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connected));
                // Send adapter info if available
                if let Some(info) = conn.adapter_info() {
                    let _ = tx.send(Message::AdapterDetected(info.clone()));
                }
            }
            Err(e) => {
                let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(
                    e.to_string(),
                )));
                let _ = tx.send(Message::Error(format!("Init failed: {e}")));
                return;
            }
        }

        // Read initial voltage
        match conn.read_voltage().await {
            Ok(v) => {
                let _ = tx.send(Message::VoltageUpdate(v));
            }
            Err(e) => {
                tracing::warn!("Could not read voltage: {e}");
            }
        }

        // Poll loop
        let mut interval = tokio::time::interval(Duration::from_millis(poll_ms));
        let mut voltage_counter = 0u32;

        loop {
            interval.tick().await;

            for &pid in Pid::all() {
                match conn.query_pid(pid).await {
                    Ok(reading) => {
                        if tx.send(Message::PidUpdate(pid, reading)).is_err() {
                            return; // receiver dropped
                        }
                    }
                    Err(e) => {
                        tracing::warn!("PID {pid:?} query failed: {e}");
                        let _ = tx.send(Message::Error(format!("{pid:?}: {e}")));
                    }
                }
            }

            // Read voltage and DTCs every 10 cycles (~2.5s)
            voltage_counter += 1;
            if voltage_counter % 10 == 0 {
                if let Ok(v) = conn.read_voltage().await {
                    let _ = tx.send(Message::VoltageUpdate(v));
                }
                if let Ok(dtcs) = conn.read_dtcs().await {
                    let _ = tx.send(Message::DtcUpdate(dtcs));
                }
            }
        }
    })
}

/// Spawn a task that connects to a device, then enters the poll loop.
/// On failure, sends `ConnectionStatus(Error(...))` and returns.
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

        let mut conn: Box<dyn Obd2Connection> = match &device {
            DeviceKind::Serial { port_path, baud: device_baud } => {
                let actual_baud = if *device_baud > 0 { *device_baud } else { baud };
                tracing::info!("Opening serial port: {} @ {} baud", port_path, actual_baud);

                const MAX_ATTEMPTS: u32 = 3;
                let mut last_error = String::new();
                let mut result: Option<Box<dyn Obd2Connection>> = None;

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

                    let builder = tokio_serial::new(port_path, actual_baud)
                        .timeout(Duration::from_secs(10));
                    let stream = match tokio_serial::SerialStream::open(&builder) {
                        Ok(s) => s,
                        Err(e) => {
                            last_error = format!("Failed to open {}: {}", port_path, e);
                            tracing::warn!(
                                "{} (attempt {}/{})",
                                last_error,
                                attempt,
                                MAX_ATTEMPTS
                            );
                            continue;
                        }
                    };

                    // Post-open delay: macOS USB-serial drivers need time to configure
                    tokio::time::sleep(Duration::from_millis(500)).await;

                    let serial = obd2::serial_transport::SerialTransport::new(stream);
                    let mut conn: Box<dyn Obd2Connection> =
                        Box::new(obd2::elm327::Elm327::new(Box::new(serial)));

                    match conn.initialize().await {
                        Ok(_) => {
                            result = Some(conn);
                            break;
                        }
                        Err(e) => {
                            last_error = format!("Init failed: {}", e);
                            tracing::warn!(
                                "{} (attempt {}/{})",
                                last_error,
                                attempt,
                                MAX_ATTEMPTS
                            );
                            continue;
                        }
                    }
                }

                match result {
                    Some(conn) => conn,
                    None => {
                        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(
                            last_error.clone(),
                        )));
                        let _ = tx.send(Message::Error(last_error));
                        return;
                    }
                }
            }
            DeviceKind::Ble { name } => {
                tracing::info!("Scanning for BLE adapter: {}", name);
                let name_filter = if name.is_empty() { None } else { Some(name.as_str()) };
                match obd2::ble_transport::scan_for_adapter(
                    name_filter,
                    Duration::from_secs(ble_scan_secs),
                )
                .await
                {
                    Ok(peripheral) => {
                        match obd2::ble_transport::BleTransport::connect(peripheral).await {
                            Ok(ble) => {
                                let mut conn: Box<dyn Obd2Connection> =
                                    Box::new(obd2::elm327::Elm327::new(Box::new(ble)));
                                match conn.initialize().await {
                                    Ok(_) => conn,
                                    Err(e) => {
                                        let msg = format!("BLE init failed: {}", e);
                                        tracing::warn!("{}", msg);
                                        let _ = tx.send(Message::ConnectionStatus(
                                            ConnectionState::Error(msg.clone()),
                                        ));
                                        let _ = tx.send(Message::Error(msg));
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                let msg = format!("BLE connect failed: {}", e);
                                tracing::warn!("{}", msg);
                                let _ = tx.send(Message::ConnectionStatus(
                                    ConnectionState::Error(msg.clone()),
                                ));
                                let _ = tx.send(Message::Error(msg));
                                return;
                            }
                        }
                    }
                    Err(e) => {
                        let msg = format!("BLE scan failed: {}", e);
                        tracing::warn!("{}", msg);
                        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Error(
                            msg.clone(),
                        )));
                        let _ = tx.send(Message::Error(msg));
                        return;
                    }
                }
            }
        };

        // Connection initialized successfully
        let _ = tx.send(Message::ConnectionStatus(ConnectionState::Connected));
        if let Some(info) = conn.adapter_info() {
            let _ = tx.send(Message::AdapterDetected(info.clone()));
        }
        let prefs = ConnectionPrefs {
            last_device: Some(device),
        };
        if let Err(e) = prefs.save(&prefs_path) {
            tracing::warn!("Failed to save connection prefs: {}", e);
        }

        // Read initial voltage
        match conn.read_voltage().await {
            Ok(v) => {
                let _ = tx.send(Message::VoltageUpdate(v));
            }
            Err(e) => {
                tracing::warn!("Could not read voltage: {e}");
            }
        }

        // Poll loop
        let mut interval = tokio::time::interval(Duration::from_millis(poll_ms));
        let mut voltage_counter = 0u32;

        loop {
            interval.tick().await;

            for &pid in Pid::all() {
                match conn.query_pid(pid).await {
                    Ok(reading) => {
                        if tx.send(Message::PidUpdate(pid, reading)).is_err() {
                            return;
                        }
                    }
                    Err(e) => {
                        tracing::warn!("PID {pid:?} query failed: {e}");
                        let _ = tx.send(Message::Error(format!("{pid:?}: {e}")));
                    }
                }
            }

            voltage_counter += 1;
            if voltage_counter % 10 == 0 {
                if let Ok(v) = conn.read_voltage().await {
                    let _ = tx.send(Message::VoltageUpdate(v));
                }
                if let Ok(dtcs) = conn.read_dtcs().await {
                    let _ = tx.send(Message::DtcUpdate(dtcs));
                }
            }
        }
    })
}

/// Try to auto-detect a serial port that looks like an ELM327 adapter.
fn auto_detect_port() -> Result<String> {
    let ports = serialport::available_ports()
        .map_err(|e| anyhow::anyhow!("failed to enumerate serial ports: {e}"))?;

    if ports.is_empty() {
        anyhow::bail!(
            "no serial ports found. Connect an ELM327 adapter or specify --port manually."
        );
    }

    // Prefer USB serial ports
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

    // Fall back to first available port
    tracing::warn!(
        "No USB serial ports found, using first available: {}",
        ports[0].port_name
    );
    Ok(ports[0].port_name.clone())
}
