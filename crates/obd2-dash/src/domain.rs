use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use obd2_core::adapter::AdapterInfo;
use obd2_core::protocol::dtc::Dtc;
use obd2_core::protocol::enhanced::Reading;
use obd2_core::protocol::pid::Pid;

use crate::analysis::driving::DrivingBehavior;
use crate::analysis::fuel_economy::{FuelEconomyState, SensorSnapshot};
use crate::recording::storage::StorageManager;
use crate::recording::RecordingState;
use crate::vehicle_data::VehicleData;
use obd2_db::models::{Alert, AlertLevel, ResolvedThreshold, VehicleInfo};

/// A snapshot of an enhanced (manufacturer-specific) PID reading.
#[derive(Debug, Clone)]
pub struct EnhancedReading {
    pub did: u16,
    pub module: String,
    pub name: String,
    pub value: f64,
    pub unit: String,
}

/// A snapshot of an O2 sensor monitoring test result.
#[derive(Debug, Clone)]
pub struct O2Reading {
    pub test_name: String,
    pub sensor: String,
    pub value: f64,
    pub unit: String,
}

/// Domain-only events (no scanner/UI messages).
#[derive(Debug)]
pub enum DomainMessage {
    PidUpdate(Pid, Reading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    ConnectionStatus(ConnectionState),
    AdapterDetected(AdapterInfo),
    Error(String),
    EnhancedPidUpdate {
        did: u16,
        module: String,
        name: String,
        value: f64,
        unit: String,
    },
    O2MonitoringUpdate(Vec<O2Reading>),
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

/// All vehicle/analysis/recording state — the domain layer.
pub struct DomainState {
    pub vehicle: VehicleData,
    pub connection: ConnectionState,
    pub adapter_info: Option<AdapterInfo>,
    pub last_error: Option<String>,
    pub poll_interval_ms: u64,
    pub temp_unit: TemperatureUnit,
    pub speed_unit: SpeedUnit,
    pub vehicle_info: Option<VehicleInfo>,
    pub active_alerts: Vec<Alert>,
    pub alert_history: VecDeque<String>,
    pub thresholds_cache: HashMap<u8, ResolvedThreshold>,
    pub stored_dtcs: Vec<Dtc>,
    pub enhanced_readings: Vec<EnhancedReading>,
    pub o2_readings: Vec<O2Reading>,
    pub fuel_economy: FuelEconomyState,
    pub driving: DrivingBehavior,
    pub recording: RecordingState,
    pub storage_manager: Option<StorageManager>,
    last_pid_update: Instant,
}

impl DomainState {
    pub fn new(poll_interval_ms: u64) -> Self {
        let (temp_unit, speed_unit) = detect_system_units();
        Self {
            vehicle: VehicleData::default(),
            connection: ConnectionState::Disconnected,
            adapter_info: None,
            last_error: None,
            poll_interval_ms,
            temp_unit,
            speed_unit,
            vehicle_info: None,
            active_alerts: Vec::new(),
            alert_history: VecDeque::new(),
            thresholds_cache: HashMap::new(),
            stored_dtcs: Vec::new(),
            enhanced_readings: Vec::new(),
            o2_readings: Vec::new(),
            fuel_economy: FuelEconomyState::new(),
            driving: DrivingBehavior::new(),
            recording: RecordingState::Idle,
            storage_manager: None,
            last_pid_update: Instant::now(),
        }
    }

    /// Process a domain message and update state.
    pub fn update(&mut self, msg: DomainMessage) {
        // Recording interception: capture data before processing
        if let RecordingState::Recording {
            ref mut writer,
            ref start_instant,
            ..
        } = self.recording
        {
            let offset_ms = start_instant.elapsed().as_millis() as u32;
            match &msg {
                DomainMessage::PidUpdate(pid, reading) => {
                    let value = reading.value.as_f64().unwrap_or(0.0);
                    let raw = &reading.raw_bytes;
                    let _ = writer.write_pid(offset_ms, pid.0, value, raw);
                }
                DomainMessage::VoltageUpdate(v) => {
                    let _ = writer.write_voltage(offset_ms, *v);
                }
                DomainMessage::DtcUpdate(dtcs) => {
                    for dtc in dtcs {
                        let _ = writer.write_dtc(offset_ms, &dtc.code);
                    }
                }
                DomainMessage::EnhancedPidUpdate { .. }
                | DomainMessage::O2MonitoringUpdate(_) => {
                    // Enhanced/O2 data not yet recorded
                }
                _ => {}
            }
        }

        match msg {
            DomainMessage::PidUpdate(pid, reading) => {
                let value = reading.value.as_f64().unwrap_or(0.0);

                // Update driving behavior on speed updates
                if pid == Pid::VEHICLE_SPEED {
                    let throttle = self.vehicle.throttle_position.unwrap_or(0.0);
                    self.driving.update(value, throttle);
                }

                self.vehicle.apply_reading(pid, &reading);
                self.check_threshold(pid, value);

                // Recalculate fuel economy
                let now = Instant::now();
                let dt = now.duration_since(self.last_pid_update).as_secs_f64();
                self.last_pid_update = now;
                let snap = SensorSnapshot::from_vehicle(&self.vehicle);
                self.fuel_economy.recalculate(&snap, dt);

                self.last_error = None;
            }
            DomainMessage::VoltageUpdate(v) => {
                self.vehicle.battery_voltage = Some(v);
            }
            DomainMessage::DtcUpdate(dtcs) => {
                self.stored_dtcs = dtcs;
            }
            DomainMessage::ConnectionStatus(state) => {
                self.connection = state;
            }
            DomainMessage::AdapterDetected(info) => {
                tracing::info!(
                    "Adapter detected: {:?} ({})",
                    info.chipset,
                    info.firmware
                );
                self.adapter_info = Some(info);
            }
            DomainMessage::Error(e) => {
                tracing::error!(target: "alerts", "Error: {}", e);
                self.alert_history.push_back(format!("Error: {}", e));
                const MAX_ALERT_HISTORY: usize = 100;
                while self.alert_history.len() > MAX_ALERT_HISTORY {
                    self.alert_history.pop_front();
                }
                self.last_error = Some(e);
            }
            DomainMessage::EnhancedPidUpdate {
                did,
                module,
                name,
                value,
                unit,
            } => {
                // Upsert: find existing by (did, module) or insert
                if let Some(existing) = self
                    .enhanced_readings
                    .iter_mut()
                    .find(|r| r.did == did && r.module == module)
                {
                    existing.value = value;
                    existing.name = name;
                    existing.unit = unit;
                } else {
                    self.enhanced_readings.push(EnhancedReading {
                        did,
                        module,
                        name,
                        value,
                        unit,
                    });
                }
            }
            DomainMessage::O2MonitoringUpdate(readings) => {
                self.o2_readings = readings;
            }
        }
    }

    /// Check a PID reading against its resolved threshold and update active_alerts.
    fn check_threshold(&mut self, pid: Pid, value: f64) {
        let code = pid.0;

        // Remove any existing alert for this PID
        self.active_alerts.retain(|a| a.pid_code != code);

        // Check against threshold if we have one
        if let Some(threshold) = self.thresholds_cache.get(&code) {
            if let Some(alert) = threshold.check(value, pid.name()) {
                let msg = format!("{}", alert);
                match alert.level {
                    AlertLevel::Critical => tracing::error!(target: "alerts", "{}", msg),
                    AlertLevel::Warning => tracing::warn!(target: "alerts", "{}", msg),
                }
                self.alert_history.push_back(msg);
                const MAX_ALERT_HISTORY: usize = 100;
                while self.alert_history.len() > MAX_ALERT_HISTORY {
                    self.alert_history.pop_front();
                }
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
        self.vehicle.coolant_temp.map(|v| match self.temp_unit {
            TemperatureUnit::Celsius => (v, "\u{00B0}C"),
            TemperatureUnit::Fahrenheit => (v * 9.0 / 5.0 + 32.0, "\u{00B0}F"),
        })
    }

    /// Get display-ready temperature value for any temperature reading.
    pub fn display_temp_value(&self, value: f64) -> (f64, &'static str) {
        match self.temp_unit {
            TemperatureUnit::Celsius => (value, "\u{00B0}C"),
            TemperatureUnit::Fahrenheit => (value * 9.0 / 5.0 + 32.0, "\u{00B0}F"),
        }
    }

    /// Get display-ready speed value.
    pub fn display_speed(&self) -> Option<(f64, &'static str)> {
        self.vehicle.speed.map(|v| match self.speed_unit {
            SpeedUnit::Kmh => (v, "km/h"),
            SpeedUnit::Mph => (v * 0.621371, "mph"),
        })
    }
}

/// Detect system locale and return appropriate temperature/speed units.
fn detect_system_units() -> (TemperatureUnit, SpeedUnit) {
    let locale = sys_locale::get_locale().unwrap_or_default().to_lowercase();
    let locale = locale.replace('-', "_");

    if locale.starts_with("en_us") || locale.starts_with("en_lr") || locale.starts_with("my_mm") {
        tracing::debug!("Locale '{}' -> Fahrenheit + mph", locale);
        (TemperatureUnit::Fahrenheit, SpeedUnit::Mph)
    } else if locale.starts_with("en_gb") {
        tracing::debug!("Locale '{}' -> Celsius + mph", locale);
        (TemperatureUnit::Celsius, SpeedUnit::Mph)
    } else {
        tracing::debug!("Locale '{}' -> Celsius + km/h", locale);
        (TemperatureUnit::Celsius, SpeedUnit::Kmh)
    }
}
