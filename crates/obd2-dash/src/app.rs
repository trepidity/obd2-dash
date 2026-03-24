use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

use crate::widget::config::DashboardConfig;
use crate::widget::edit_mode::EditModeState;
use crate::ai::{AiConfig, AiInsights};
use crate::nhtsa::NhtsaVehicle;
use crate::domain::{ConnectionState, DomainMessage, DomainState, O2Reading};
use crate::scanner::{DeviceKind, DiscoveredDevice, ScanEvent};

use obd2_core::adapter::AdapterInfo;
use obd2_core::protocol::dtc::Dtc;
use obd2_core::protocol::enhanced::Reading;
use obd2_core::protocol::pid::Pid;
use obd2_core::transport::CaptureMetadata;

/// Commands sent from the UI thread to the session task for raw capture control.
pub enum CaptureCommand {
    Start { path: PathBuf, metadata: CaptureMetadata },
    Stop,
}

/// Shared handle for raw protocol capture state.
/// Arc-wrapped so the session task and main thread can coordinate.
#[derive(Clone)]
pub struct CaptureHandle {
    active: Arc<AtomicBool>,
    recordings_dir: Arc<PathBuf>,
}

impl CaptureHandle {
    pub fn new(recordings_dir: PathBuf) -> Self {
        Self {
            active: Arc::new(AtomicBool::new(false)),
            recordings_dir: Arc::new(recordings_dir),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    pub fn recordings_dir(&self) -> &Path {
        &self.recordings_dir
    }
}

/// Messages flowing into the app state (TEA / Elm-style).
#[derive(Debug)]
pub enum Message {
    // Domain (forwarded to DomainState)
    PidUpdate(Pid, Reading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    ConnectionStatus(ConnectionState),
    AdapterDetected(AdapterInfo),
    Error(String),
    // UI-only
    VinDetected(String),
    DeviceFound(DiscoveredDevice),
    ScanComplete,
    StartConnect(DeviceKind),
    Tick,
    Quit,
    // Vehicle identification
    NhtsaResult(String, NhtsaVehicle),
    // AI analysis
    AiAnalysisComplete(AiInsights),
    AiAnalysisError(String),
    // Enhanced PIDs
    EnhancedPidUpdate {
        did: u16,
        module: String,
        name: String,
        value: f64,
        unit: String,
    },
    // O2 sensor monitoring
    O2MonitoringUpdate(Vec<O2Reading>),
    // Raw protocol capture
    RawCaptureStarted,
    RawCaptureStopped(PathBuf),
    RawCaptureError(String),
}

impl Message {
    /// Convert a ScanEvent into a Message.
    pub fn from_scan_event(event: ScanEvent) -> Self {
        match event {
            ScanEvent::DeviceFound(dev) => Message::DeviceFound(dev),
            ScanEvent::ScanComplete => Message::ScanComplete,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Idle,
    Scanning,
    Picking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DashboardLayout {
    Compact,
    Full,
}

#[derive(Debug, Clone)]
pub struct PopupState {
    pub title: String,
    pub body: Vec<String>,
}

pub struct AppState {
    /// Domain state (vehicle data, analysis, recording, thresholds, etc.)
    pub domain: DomainState,
    // UI-only fields
    pub running: bool,
    pub paused: bool,
    pub layout: DashboardLayout,
    pub focused_panel: Option<usize>,
    pub panel_selections: HashMap<usize, usize>,
    pub popup: Option<PopupState>,
    // Widget-based dashboard
    pub dashboard_config: DashboardConfig,
    pub config_path: Option<PathBuf>,
    pub focused_widget: Option<usize>,
    pub widget_selections: HashMap<usize, usize>,
    pub edit_mode: Option<EditModeState>,
    // Session picker
    pub show_session_picker: bool,
    pub session_picker_selected: usize,
    // Device scanning
    pub scan_mode: ScanMode,
    pub scan_devices: Vec<DiscoveredDevice>,
    pub scan_selected: usize,
    pub pending_connect: Option<DeviceKind>,
    pub scan_requested: bool,
    // Debug log viewer
    pub show_debug_log: bool,
    pub debug_log_scroll: usize,
    // AI analysis
    pub ai_config: Option<AiConfig>,
    pub ai_insights: Option<AiInsights>,
    pub ai_analyzing: bool,
    pub ai_scroll: usize,
    pub show_ai_insights: bool,
    // Raw protocol capture
    pub capture_handle: Option<CaptureHandle>,
    pub capture_tx: Option<mpsc::UnboundedSender<CaptureCommand>>,
}

impl AppState {
    pub fn new(poll_ms: u64) -> Self {
        Self {
            domain: DomainState::new(poll_ms),
            running: true,
            paused: false,
            layout: DashboardLayout::Full,
            focused_panel: None,
            panel_selections: HashMap::new(),
            popup: None,
            dashboard_config: DashboardConfig::default(),
            config_path: None,
            focused_widget: None,
            widget_selections: HashMap::new(),
            edit_mode: None,
            show_session_picker: false,
            session_picker_selected: 0,
            scan_mode: ScanMode::Idle,
            scan_devices: Vec::new(),
            scan_selected: 0,
            pending_connect: None,
            scan_requested: false,
            show_debug_log: false,
            debug_log_scroll: 0,
            ai_config: None,
            ai_insights: None,
            ai_analyzing: false,
            ai_scroll: 0,
            show_ai_insights: false,
            capture_handle: None,
            capture_tx: None,
        }
    }

    pub fn update(&mut self, msg: Message) {
        match msg {
            Message::PidUpdate(pid, reading) => {
                self.domain.update(DomainMessage::PidUpdate(pid, reading));
            }
            Message::VoltageUpdate(v) => {
                self.domain.update(DomainMessage::VoltageUpdate(v));
            }
            Message::DtcUpdate(dtcs) => {
                self.domain.update(DomainMessage::DtcUpdate(dtcs));
            }
            Message::ConnectionStatus(state) => {
                self.domain
                    .update(DomainMessage::ConnectionStatus(state));
            }
            Message::AdapterDetected(info) => {
                self.domain.update(DomainMessage::AdapterDetected(info));
            }
            Message::Error(e) => {
                self.domain.update(DomainMessage::Error(e));
            }
            Message::DeviceFound(dev) => {
                self.scan_devices.push(dev);
                if self.scan_mode == ScanMode::Scanning {
                    self.scan_mode = ScanMode::Picking;
                }
            }
            Message::ScanComplete => {
                if self.scan_mode == ScanMode::Scanning {
                    self.scan_mode = ScanMode::Picking;
                }
            }
            Message::StartConnect(kind) => {
                self.pending_connect = Some(kind);
                self.scan_mode = ScanMode::Idle;
            }
            Message::VinDetected(_) => {
                // Handled in main loop
            }
            Message::NhtsaResult(_, _) => {
                // Handled in main loop
            }
            Message::AiAnalysisComplete(insights) => {
                self.ai_insights = Some(insights);
                self.ai_analyzing = false;
            }
            Message::AiAnalysisError(e) => {
                tracing::warn!("AI analysis error: {}", e);
                self.ai_analyzing = false;
                self.show_ai_insights = false;
            }
            Message::EnhancedPidUpdate {
                did,
                module,
                name,
                value,
                unit,
            } => {
                self.domain.update(DomainMessage::EnhancedPidUpdate {
                    did,
                    module,
                    name,
                    value,
                    unit,
                });
            }
            Message::O2MonitoringUpdate(readings) => {
                self.domain
                    .update(DomainMessage::O2MonitoringUpdate(readings));
            }
            Message::Tick => {
                // UI tick -- nothing to do here
            }
            Message::RawCaptureStarted => {
                if let Some(ref handle) = self.capture_handle {
                    handle.set_active(true);
                }
                tracing::info!("Raw protocol capture started");
            }
            Message::RawCaptureStopped(path) => {
                if let Some(ref handle) = self.capture_handle {
                    handle.set_active(false);
                }
                let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                tracing::info!("Raw capture saved: {} ({} bytes)", path.display(), file_size);
            }
            Message::RawCaptureError(e) => {
                if let Some(ref handle) = self.capture_handle {
                    handle.set_active(false);
                }
                tracing::warn!("Raw capture error: {}", e);
                self.domain.last_error = Some(format!("Raw capture error: {}", e));
            }
            Message::Quit => {
                self.running = false;
            }
        }
    }
}
