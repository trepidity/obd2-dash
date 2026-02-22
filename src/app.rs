use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use crate::db::models::{Alert, AlertLevel, ResolvedThreshold, VehicleInfo};
use crate::driving::DrivingBehavior;
use crate::fuel_economy::{FuelEconomyState, SensorSnapshot};
use crate::obd2::{AdapterInfo, Dtc, Pid, PidReading, VehicleData};
use crate::obd2::scanner::{DeviceKind, DiscoveredDevice};
use crate::recording::RecordingState;
use crate::recording::storage::StorageManager;
use crate::widget::config::DashboardConfig;
use crate::widget::edit_mode::EditModeState;

/// Messages flowing into the app state (TEA / Elm-style).
#[derive(Debug)]
pub enum Message {
    PidUpdate(Pid, PidReading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    ConnectionStatus(ConnectionState),
    AdapterDetected(AdapterInfo),
    DeviceFound(DiscoveredDevice),
    ScanComplete,
    StartConnect(DeviceKind),
    Error(String),
    Tick,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Idle,
    Scanning,
    Picking,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemperatureUnit {
    Celsius,
    Fahrenheit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeedUnit {
    Kmh,
    Mph,
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
    pub vehicle: VehicleData,
    pub connection: ConnectionState,
    pub adapter_info: Option<AdapterInfo>,
    pub last_error: Option<String>,
    pub running: bool,
    pub paused: bool,
    pub poll_interval_ms: u64,
    pub temp_unit: TemperatureUnit,
    pub speed_unit: SpeedUnit,
    pub vehicle_info: Option<VehicleInfo>,
    pub layout: DashboardLayout,
    pub active_alerts: Vec<Alert>,
    pub thresholds_cache: HashMap<u8, ResolvedThreshold>,
    pub stored_dtcs: Vec<Dtc>,
    pub focused_panel: Option<usize>,
    pub panel_selections: HashMap<usize, usize>,
    pub popup: Option<PopupState>,
    pub fuel_economy: FuelEconomyState,
    last_pid_update: Instant,
    // Widget-based dashboard
    pub dashboard_config: DashboardConfig,
    pub config_path: Option<PathBuf>,
    pub focused_widget: Option<usize>,
    pub widget_selections: HashMap<usize, usize>,
    pub edit_mode: Option<EditModeState>,
    // Driving behavior analysis
    pub driving: DrivingBehavior,
    // Recording & replay
    pub recording: RecordingState,
    pub storage_manager: Option<StorageManager>,
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
            vehicle: VehicleData::default(),
            connection: ConnectionState::Disconnected,
            adapter_info: None,
            last_error: None,
            running: true,
            paused: false,
            poll_interval_ms,
            temp_unit: TemperatureUnit::Celsius,
            speed_unit: SpeedUnit::Kmh,
            layout: DashboardLayout::Compact,
            vehicle_info: None,
            active_alerts: Vec::new(),
            thresholds_cache: HashMap::new(),
            stored_dtcs: Vec::new(),
            focused_panel: None,
            panel_selections: HashMap::new(),
            popup: None,
            fuel_economy: FuelEconomyState::new(),
            last_pid_update: Instant::now(),
            dashboard_config: DashboardConfig::default_layout(),
            config_path: None,
            focused_widget: None,
            widget_selections: HashMap::new(),
            edit_mode: None,
            driving: DrivingBehavior::new(),
            recording: RecordingState::Idle,
            storage_manager: None,
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
        // Recording interception: capture data before processing
        if let RecordingState::Recording {
            ref mut writer,
            ref start_instant,
            ..
        } = self.recording
        {
            let offset_ms = start_instant.elapsed().as_millis() as u32;
            match &msg {
                Message::PidUpdate(pid, reading) => {
                    let _ = writer.write_pid(offset_ms, pid.code(), reading.value);
                }
                Message::VoltageUpdate(v) => {
                    let _ = writer.write_voltage(offset_ms, *v);
                }
                Message::DtcUpdate(dtcs) => {
                    for dtc in dtcs {
                        let _ = writer.write_dtc(offset_ms, &dtc.code);
                    }
                }
                _ => {}
            }
        }

        match msg {
            Message::PidUpdate(pid, reading) => {
                let value = reading.value;
                match pid {
                    Pid::EngineRpm => {
                        self.vehicle.rpm_history.push(reading.value as u64);
                        self.vehicle.rpm = Some(reading);
                    }
                    Pid::VehicleSpeed => {
                        self.vehicle.speed_history.push(reading.value as u64);
                        let throttle = self.vehicle.throttle_position.as_ref().map(|r| r.value).unwrap_or(0.0);
                        self.driving.update(reading.value, throttle);
                        self.vehicle.speed = Some(reading);
                    }
                    Pid::CoolantTemp => {
                        self.vehicle.coolant_temp = Some(reading);
                    }
                    Pid::EngineLoad => {
                        self.vehicle.load_history.push(reading.value as u64);
                        self.vehicle.engine_load = Some(reading);
                    }
                    Pid::ShortFuelTrimBank1 => {
                        self.vehicle.short_fuel_trim_b1 = Some(reading);
                    }
                    Pid::LongFuelTrimBank1 => {
                        self.vehicle.long_fuel_trim_b1 = Some(reading);
                    }
                    Pid::ShortFuelTrimBank2 => {
                        self.vehicle.short_fuel_trim_b2 = Some(reading);
                    }
                    Pid::LongFuelTrimBank2 => {
                        self.vehicle.long_fuel_trim_b2 = Some(reading);
                    }
                    Pid::FuelPressure => {
                        self.vehicle.fuel_pressure = Some(reading);
                    }
                    Pid::IntakeMap => {
                        self.vehicle.intake_map = Some(reading);
                    }
                    Pid::IntakeAirTemp => {
                        self.vehicle.intake_air_temp = Some(reading);
                    }
                    Pid::Maf => {
                        self.vehicle.maf = Some(reading);
                    }
                    Pid::ThrottlePosition => {
                        self.vehicle.throttle_history.push(reading.value as u64);
                        self.vehicle.throttle_position = Some(reading);
                    }
                    Pid::FuelTankLevel => {
                        self.vehicle.fuel_tank_level = Some(reading);
                    }
                    Pid::BarometricPressure => {
                        self.vehicle.barometric_pressure = Some(reading);
                    }
                    Pid::CatalystTempB1S1 => {
                        self.vehicle.catalyst_temp_b1s1 = Some(reading);
                    }
                    Pid::CatalystTempB2S1 => {
                        self.vehicle.catalyst_temp_b2s1 = Some(reading);
                    }
                    Pid::CatalystTempB1S2 => {
                        self.vehicle.catalyst_temp_b1s2 = Some(reading);
                    }
                    Pid::CatalystTempB2S2 => {
                        self.vehicle.catalyst_temp_b2s2 = Some(reading);
                    }
                    Pid::ControlModuleVoltage => {
                        self.vehicle.control_module_voltage = Some(reading);
                    }
                    Pid::AmbientAirTemp => {
                        self.vehicle.ambient_air_temp = Some(reading);
                    }
                    Pid::EngineOilTemp => {
                        self.vehicle.engine_oil_temp = Some(reading);
                    }
                    Pid::EngineFuelRate => {
                        self.vehicle.engine_fuel_rate = Some(reading);
                    }
                    Pid::TransmissionTemp => {
                        self.vehicle.transmission_temp = Some(reading);
                    }
                    Pid::OilPressure => {
                        self.vehicle.oil_pressure = Some(reading);
                    }
                }
                // Recompute derived boost pressure (MAP - Barometric)
                self.vehicle.boost_pressure = match (
                    &self.vehicle.intake_map,
                    &self.vehicle.barometric_pressure,
                ) {
                    (Some(map), Some(baro)) => Some(map.value - baro.value),
                    _ => None,
                };
                self.check_threshold(pid, value);

                // Recalculate fuel economy
                let now = Instant::now();
                let dt = now.duration_since(self.last_pid_update).as_secs_f64();
                self.last_pid_update = now;
                let snap = SensorSnapshot::from_vehicle(&self.vehicle);
                self.fuel_economy.recalculate(&snap, dt);

                self.last_error = None;
            }
            Message::VoltageUpdate(v) => {
                self.vehicle.battery_voltage = Some(v);
            }
            Message::DtcUpdate(dtcs) => {
                self.stored_dtcs = dtcs;
            }
            Message::ConnectionStatus(state) => {
                self.connection = state;
            }
            Message::AdapterDetected(info) => {
                tracing::info!("Adapter detected: {} ({})", info.chipset, info.firmware_version);
                self.adapter_info = Some(info);
            }
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
                        self.last_error = Some("No devices found".into());
                    } else {
                        self.scan_mode = ScanMode::Picking;
                    }
                }
            }
            Message::StartConnect(kind) => {
                self.pending_connect = Some(kind);
                self.connection = ConnectionState::Connecting;
                self.scan_mode = ScanMode::Idle;
            }
            Message::Error(e) => {
                self.last_error = Some(e);
            }
            Message::Tick => {
                // Tick is used to trigger redraws — no state change needed
            }
            Message::Quit => {
                self.running = false;
            }
        }
    }

    /// Check a PID reading against its resolved threshold and update active_alerts.
    fn check_threshold(&mut self, pid: Pid, value: f64) {
        let code = pid.code();

        // Remove any existing alert for this PID
        self.active_alerts.retain(|a| a.pid_code != code);

        // Check against threshold if we have one
        if let Some(threshold) = self.thresholds_cache.get(&code) {
            if let Some(alert) = threshold.check(value, pid.name()) {
                self.active_alerts.push(alert);
            }
        }
    }

    /// Get the highest severity alert level, if any alerts are active.
    pub fn worst_alert_level(&self) -> Option<AlertLevel> {
        let has_critical = self
            .active_alerts
            .iter()
            .any(|a| a.level == AlertLevel::Critical);
        if has_critical {
            Some(AlertLevel::Critical)
        } else if !self.active_alerts.is_empty() {
            Some(AlertLevel::Warning)
        } else {
            None
        }
    }

    /// Get display-ready temperature value.
    pub fn display_temp(&self) -> Option<(f64, &'static str)> {
        self.vehicle
            .coolant_temp
            .as_ref()
            .map(|r| match self.temp_unit {
                TemperatureUnit::Celsius => (r.value, "°C"),
                TemperatureUnit::Fahrenheit => (r.value * 9.0 / 5.0 + 32.0, "°F"),
            })
    }

    /// Get display-ready temperature value for any temperature PidReading.
    pub fn display_temp_value(&self, reading: &PidReading) -> (f64, &'static str) {
        match self.temp_unit {
            TemperatureUnit::Celsius => (reading.value, "°C"),
            TemperatureUnit::Fahrenheit => (reading.value * 9.0 / 5.0 + 32.0, "°F"),
        }
    }

    /// Get display-ready speed value.
    pub fn display_speed(&self) -> Option<(f64, &'static str)> {
        self.vehicle
            .speed
            .as_ref()
            .map(|r| match self.speed_unit {
                SpeedUnit::Kmh => (r.value, "km/h"),
                SpeedUnit::Mph => (r.value * 0.621371, "mph"),
            })
    }
}
