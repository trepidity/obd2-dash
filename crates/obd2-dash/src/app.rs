use std::collections::HashMap;
use std::path::PathBuf;

use obd2_core::{
    AdapterInfo, ConnectionState, DeviceKind, DiscoveredDevice, DomainMessage, DomainState,
    Dtc, Pid, PidReading, ScanEvent,
};
use crate::widget::config::DashboardConfig;
use crate::widget::edit_mode::EditModeState;

/// Messages flowing into the app state (TEA / Elm-style).
#[derive(Debug)]
pub enum Message {
    // Domain (forwarded to DomainState)
    PidUpdate(Pid, PidReading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    ConnectionStatus(ConnectionState),
    AdapterDetected(AdapterInfo),
    Error(String),
    // UI-only
    DeviceFound(DiscoveredDevice),
    ScanComplete,
    StartConnect(DeviceKind),
    Tick,
    Quit,
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
}

impl AppState {
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            domain: DomainState::new(poll_interval_ms),
            running: true,
            paused: false,
            layout: DashboardLayout::Compact,
            focused_panel: None,
            panel_selections: HashMap::new(),
            popup: None,
            dashboard_config: DashboardConfig::default_layout(),
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
        }
    }

    /// Process a message and update state.
    pub fn update(&mut self, msg: Message) {
        // Try to convert to domain message and delegate
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
                self.domain.update(DomainMessage::ConnectionStatus(state));
            }
            Message::AdapterDetected(info) => {
                self.domain.update(DomainMessage::AdapterDetected(info));
            }
            Message::Error(e) => {
                self.domain.update(DomainMessage::Error(e));
            }
            // UI-only messages handled here
            Message::DeviceFound(dev) => {
                // Deduplicate by kind
                if !self.scan_devices.iter().any(|d| d.kind == dev.kind) {
                    self.scan_devices.push(dev);
                }
            }
            Message::ScanComplete => {
                if self.scan_mode == ScanMode::Scanning {
                    if self.scan_devices.is_empty() {
                        self.scan_mode = ScanMode::Idle;
                        self.domain.last_error = Some("No devices found".into());
                    } else {
                        self.scan_mode = ScanMode::Picking;
                    }
                }
            }
            Message::StartConnect(kind) => {
                self.pending_connect = Some(kind);
                self.domain.connection = ConnectionState::Connecting;
                self.scan_mode = ScanMode::Idle;
            }
            Message::Tick => {
                // Tick is used to trigger redraws — no state change needed
            }
            Message::Quit => {
                self.running = false;
            }
        }
    }
}
