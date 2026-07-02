use std::borrow::Cow;
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use obd2_core::adapter::{AdapterInfo, ProtocolSelectionSource};
use obd2_core::protocol::dtc::Dtc;
use obd2_core::protocol::enhanced::Reading;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::ReadinessStatus;
use obd2_core::session::discovery::{DiscoveryProfile, VisibleEcu};
use obd2_core::vehicle::{ModuleId, Protocol};
use obd2_dash::profiles::ProfileEvidenceRecord;

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
    pub value: Option<f64>,
    pub unit: String,
    pub last_error: Option<String>,
    pub confidence: Option<String>,
    pub evidence: Option<String>,
}

/// A snapshot of an O2 sensor monitoring test result.
#[derive(Debug, Clone)]
pub struct O2Reading {
    pub test_name: String,
    pub sensor: String,
    pub value: f64,
    pub unit: String,
}

/// Freeze-frame sensor snapshot associated with a specific DTC.
#[derive(Debug, Clone)]
pub struct FreezeFrameSnapshot {
    pub dtc_code: String,
    pub readings: Vec<(Pid, f64, &'static str)>, // (pid, value, unit)
}

/// One standard DTC-service probe result from the latest scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticScanEntry {
    pub scope: DiagnosticScanScope,
    pub service: DtcService,
    pub result: DiagnosticScanResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticScanScope {
    Broadcast,
    Module(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DtcService {
    Stored,
    Pending,
    Permanent,
    GmClass2All,
    GmClass2Active,
}

impl DtcService {
    pub fn service_id(self) -> u8 {
        match self {
            Self::Stored => 0x03,
            Self::Pending => 0x07,
            Self::Permanent => 0x0A,
            Self::GmClass2All | Self::GmClass2Active => 0x19,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Stored => "03",
            Self::Pending => "07",
            Self::Permanent => "0A",
            Self::GmClass2All => "19ff",
            Self::GmClass2Active => "1992",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticScanResult {
    Codes(usize),
    Empty,
    NoData,
    Unsupported(String),
    Error(String),
}

/// Discovery information surfaced from obd2-core Session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryState {
    pub selected_protocol: Protocol,
    pub protocol_choice_source: ProtocolSelectionSource,
    pub active_bus_id: Option<String>,
    pub active_bus_description: Option<String>,
    pub active_bus_speed_bps: Option<u32>,
    pub resolved_modules: Vec<ModuleId>,
    pub visible_ecus: Vec<VisibleEcu>,
}

impl From<&DiscoveryProfile> for DiscoveryState {
    fn from(profile: &DiscoveryProfile) -> Self {
        let active_bus_id = profile.active_bus.as_ref().map(|bus| bus.id.0.clone());
        let active_bus_description = profile
            .active_bus
            .as_ref()
            .and_then(|bus| bus.description.clone());
        let active_bus_speed_bps = profile.active_bus.as_ref().map(|bus| bus.speed_bps);

        let mut resolved_modules: Vec<ModuleId> = profile.modules.keys().cloned().collect();
        resolved_modules.sort_by(|a, b| a.0.cmp(&b.0));

        Self {
            selected_protocol: profile.selected_protocol,
            protocol_choice_source: profile.protocol_choice_source,
            active_bus_id,
            active_bus_description,
            active_bus_speed_bps,
            resolved_modules,
            visible_ecus: profile.visible_ecus.clone(),
        }
    }
}

/// Domain-only events (no scanner/UI messages).
#[derive(Debug)]
pub enum DomainMessage {
    PidUpdate(Pid, Reading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    DiagnosticScanUpdate(Vec<DiagnosticScanEntry>),
    ConnectionStatus(ConnectionState),
    DiscoveryUpdated(DiscoveryState),
    AdapterDetected(AdapterInfo),
    Error(String),
    EnhancedPidUpdate {
        did: u16,
        module: String,
        name: String,
        value: f64,
        unit: String,
        confidence: Option<String>,
        evidence: Option<String>,
    },
    EnhancedPidTarget {
        did: u16,
        module: String,
        name: String,
        unit: String,
        confidence: Option<String>,
        evidence: Option<String>,
    },
    EnhancedPidError {
        did: u16,
        module: String,
        name: String,
        unit: String,
        error: String,
        confidence: Option<String>,
        evidence: Option<String>,
    },
    O2MonitoringUpdate(Vec<O2Reading>),
    ReadinessUpdate(ReadinessStatus),
    ProfileEvidence(Box<ProfileEvidenceRecord>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    AdapterPresent,
    AdapterInitialized,
    ProtocolNegotiating,
    Connected,
    IgnitionOff,
    UnsupportedProtocol,
    Error(String),
}

impl ConnectionState {
    pub fn from_session(state: &obd2_core::session::ConnectionState) -> Self {
        match state {
            obd2_core::session::ConnectionState::AdapterPresent => Self::AdapterPresent,
            obd2_core::session::ConnectionState::AdapterInitialized => Self::AdapterInitialized,
            obd2_core::session::ConnectionState::ProtocolNegotiating => Self::ProtocolNegotiating,
            obd2_core::session::ConnectionState::Connected => Self::Connected,
            obd2_core::session::ConnectionState::IgnitionOff => Self::IgnitionOff,
            obd2_core::session::ConnectionState::UnsupportedProtocol => Self::UnsupportedProtocol,
            obd2_core::session::ConnectionState::Disconnected => Self::Disconnected,
            obd2_core::session::ConnectionState::Error(e) => Self::Error(e.clone()),
        }
    }

    pub fn allows_scan_hint(&self) -> bool {
        matches!(
            self,
            Self::Disconnected | Self::Error(_) | Self::UnsupportedProtocol
        )
    }
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
    pub discovery: Option<DiscoveryState>,
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
    pub diagnostic_scan: Vec<DiagnosticScanEntry>,
    pub diagnostic_scan_updated: Option<Instant>,
    pub enhanced_readings: Vec<EnhancedReading>,
    pub o2_readings: Vec<O2Reading>,
    pub fuel_economy: FuelEconomyState,
    pub driving: DrivingBehavior,
    pub readiness: Option<ReadinessStatus>,
    pub freeze_frame_pending: bool,
    pub freeze_frame_data: Option<FreezeFrameSnapshot>,
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
            discovery: None,
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
            diagnostic_scan: Vec::new(),
            diagnostic_scan_updated: None,
            enhanced_readings: Vec::new(),
            o2_readings: Vec::new(),
            fuel_economy: FuelEconomyState::new(),
            driving: DrivingBehavior::new(),
            readiness: None,
            freeze_frame_pending: false,
            freeze_frame_data: None,
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
                DomainMessage::DiagnosticScanUpdate(_) => {}
                DomainMessage::EnhancedPidUpdate {
                    did,
                    ref module,
                    ref name,
                    value,
                    ref unit,
                    ..
                } => {
                    let _ = writer.write_enhanced(offset_ms, *did, module, name, unit, *value);
                }
                DomainMessage::EnhancedPidTarget { .. }
                | DomainMessage::EnhancedPidError { .. } => {}
                DomainMessage::O2MonitoringUpdate(ref readings) => {
                    for r in readings {
                        let _ =
                            writer.write_o2(offset_ms, &r.test_name, &r.sensor, &r.unit, r.value);
                    }
                }
                DomainMessage::ProfileEvidence(record) => {
                    let _ = writer.write_profile_evidence(offset_ms, record);
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
                if dtcs != self.stored_dtcs {
                    if dtcs.is_empty() {
                        if !self.stored_dtcs.is_empty() {
                            self.push_alert_history("DTCs cleared".to_string());
                        }
                    } else {
                        for dtc in &dtcs {
                            self.push_alert_history(format_dtc_alert(dtc));
                        }
                    }
                }
                self.stored_dtcs = dtcs;
            }
            DomainMessage::DiagnosticScanUpdate(entries) => {
                self.diagnostic_scan = entries;
                self.diagnostic_scan_updated = Some(Instant::now());
            }
            DomainMessage::ConnectionStatus(state) => {
                if matches!(
                    state,
                    ConnectionState::Disconnected
                        | ConnectionState::Connecting
                        | ConnectionState::Error(_)
                ) {
                    self.discovery = None;
                    self.readiness = None;
                    self.freeze_frame_data = None;
                    self.freeze_frame_pending = false;
                    self.diagnostic_scan.clear();
                    self.diagnostic_scan_updated = None;
                    self.enhanced_readings.clear();
                    self.o2_readings.clear();
                }
                self.connection = state;
            }
            DomainMessage::DiscoveryUpdated(discovery) => {
                self.discovery = Some(discovery);
            }
            DomainMessage::AdapterDetected(info) => {
                tracing::info!("Adapter detected: {:?} ({})", info.chipset, info.firmware);
                self.adapter_info = Some(info);
            }
            DomainMessage::Error(e) => {
                tracing::error!(target: "alerts", "Error: {}", e);
                self.push_alert_history(format!("Error: {}", e));
                self.last_error = Some(e);
            }
            DomainMessage::EnhancedPidUpdate {
                did,
                module,
                name,
                value,
                unit,
                confidence,
                evidence,
            } => {
                // Upsert: find existing by (did, module) or insert
                if let Some(existing) = self
                    .enhanced_readings
                    .iter_mut()
                    .find(|r| r.did == did && r.module == module)
                {
                    existing.value = Some(value);
                    existing.name = name;
                    existing.unit = unit;
                    existing.last_error = None;
                    existing.confidence = confidence;
                    existing.evidence = evidence;
                } else {
                    self.enhanced_readings.push(EnhancedReading {
                        did,
                        module,
                        name,
                        value: Some(value),
                        unit,
                        last_error: None,
                        confidence,
                        evidence,
                    });
                }
            }
            DomainMessage::EnhancedPidTarget {
                did,
                module,
                name,
                unit,
                confidence,
                evidence,
            } => {
                if let Some(existing) = self
                    .enhanced_readings
                    .iter_mut()
                    .find(|r| r.did == did && r.module == module)
                {
                    existing.name = name;
                    existing.unit = unit;
                    existing.confidence = confidence;
                    existing.evidence = evidence;
                } else {
                    self.enhanced_readings.push(EnhancedReading {
                        did,
                        module,
                        name,
                        value: None,
                        unit,
                        last_error: None,
                        confidence,
                        evidence,
                    });
                }
            }
            DomainMessage::EnhancedPidError {
                did,
                module,
                name,
                unit,
                error,
                confidence,
                evidence,
            } => {
                if let Some(existing) = self
                    .enhanced_readings
                    .iter_mut()
                    .find(|r| r.did == did && r.module == module)
                {
                    existing.value = None;
                    existing.name = name;
                    existing.unit = unit;
                    existing.last_error = Some(error);
                    existing.confidence = confidence;
                    existing.evidence = evidence;
                } else {
                    self.enhanced_readings.push(EnhancedReading {
                        did,
                        module,
                        name,
                        value: None,
                        unit,
                        last_error: Some(error),
                        confidence,
                        evidence,
                    });
                }
            }
            DomainMessage::O2MonitoringUpdate(readings) => {
                self.o2_readings = readings;
            }
            DomainMessage::ReadinessUpdate(status) => {
                self.readiness = Some(status);
            }
            DomainMessage::ProfileEvidence(_) => {}
        }
    }

    pub fn readiness_ignition_label(&self, status: &ReadinessStatus) -> &'static str {
        if self
            .vehicle_info
            .as_ref()
            .and_then(|info| info.fuel_type.as_deref())
            .is_some_and(fuel_indicates_diesel)
            || status.compression_ignition
        {
            "Diesel"
        } else {
            "Spark"
        }
    }

    /// Check a PID reading against its resolved threshold and update active_alerts.
    fn check_threshold(&mut self, pid: Pid, value: f64) {
        let code = pid.0;
        let previous_alert = self
            .active_alerts
            .iter()
            .find(|a| a.pid_code == code)
            .map(|a| a.to_string());

        // Remove any existing alert for this PID
        self.active_alerts.retain(|a| a.pid_code != code);

        if self.should_suppress_threshold_alert(pid, value) {
            return;
        }

        // Check against threshold if we have one
        if let Some(threshold) = self.thresholds_cache.get(&code) {
            if let Some(alert) = threshold.check(value, pid.name()) {
                let msg = format!("{}", alert);
                if previous_alert.as_deref() != Some(msg.as_str()) {
                    match alert.level {
                        AlertLevel::Critical => tracing::error!(target: "alerts", "{}", msg),
                        AlertLevel::Warning => tracing::warn!(target: "alerts", "{}", msg),
                    }
                    self.push_alert_history(msg);
                }
                self.active_alerts.push(alert);
            }
        }
    }

    fn should_suppress_threshold_alert(&self, pid: Pid, value: f64) -> bool {
        if pid != Pid::ENGINE_RPM || value >= 300.0 {
            return false;
        }

        // 0 rpm key-on and sub-idle cranking are not diagnostic faults. Keep high-RPM
        // thresholding intact, but do not page the alerts panel while stationary.
        self.vehicle.speed.unwrap_or(0.0).abs() < 0.5
    }

    fn push_alert_history(&mut self, msg: String) {
        self.alert_history.push_back(msg);
        const MAX_ALERT_HISTORY: usize = 100;
        while self.alert_history.len() > MAX_ALERT_HISTORY {
            self.alert_history.pop_front();
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

    pub fn uses_us_customary_units(&self) -> bool {
        self.speed_unit == SpeedUnit::Mph
    }

    pub fn display_pressure_value(&self, kpa: f64) -> (f64, &'static str) {
        if self.uses_us_customary_units() {
            (kpa * 0.145_037_737_7, "psi")
        } else {
            (kpa, "kPa")
        }
    }

    pub fn display_psi_value(&self, psi: f64) -> (f64, &'static str) {
        if self.uses_us_customary_units() {
            (psi, "psi")
        } else {
            (psi / 0.145_037_737_7, "kPa")
        }
    }

    pub fn display_fuel_rate_value(&self, liters_per_hour: f64) -> (f64, &'static str) {
        if self.uses_us_customary_units() {
            (liters_per_hour * 0.264_172_052_4, "gal/h")
        } else {
            (liters_per_hour, "L/h")
        }
    }

    pub fn display_mass_air_flow_value(&self, grams_per_sec: f64) -> (f64, &'static str) {
        (grams_per_sec, "g/s")
    }

    pub fn display_distance_value(&self, km: f64) -> (f64, &'static str) {
        if self.uses_us_customary_units() {
            (km * 0.621_371, "mi")
        } else {
            (km, "km")
        }
    }

    pub fn display_torque_value(&self, nm: f64) -> (f64, &'static str) {
        if self.uses_us_customary_units() {
            (nm * 0.737_562_149, "lb-ft")
        } else {
            (nm, "Nm")
        }
    }

    pub fn display_pid_value(&self, pid: Pid, value: f64) -> (f64, &'static str) {
        match pid.0 {
            0x05 | 0x0F | 0x3C..=0x3F | 0x46 | 0x5C | 0xFE => self.display_temp_value(value),
            0x0D => match self.speed_unit {
                SpeedUnit::Kmh => (value, "km/h"),
                SpeedUnit::Mph => (value * 0.621371, "mph"),
            },
            0x0A | 0x0B | 0x23 | 0x33 | 0x59 | 0xFD => self.display_pressure_value(value),
            0x10 => self.display_mass_air_flow_value(value),
            0x5E => self.display_fuel_rate_value(value),
            0x21 | 0x31 => self.display_distance_value(value),
            0x63 => self.display_torque_value(value),
            _ => (value, pid.unit()),
        }
    }

    pub fn display_unit_value<'a>(&self, value: f64, unit: &'a str) -> (f64, Cow<'a, str>) {
        match unit {
            "\u{00B0}C" | "C" => {
                let (value, unit) = self.display_temp_value(value);
                (value, Cow::Borrowed(unit))
            }
            "km/h" => match self.speed_unit {
                SpeedUnit::Kmh => (value, Cow::Borrowed("km/h")),
                SpeedUnit::Mph => (value * 0.621371, Cow::Borrowed("mph")),
            },
            "kPa" => {
                let (value, unit) = self.display_pressure_value(value);
                (value, Cow::Borrowed(unit))
            }
            "kPa abs" => {
                if self.uses_us_customary_units() {
                    (value * 0.145_037_737_7, Cow::Borrowed("psi abs"))
                } else {
                    (value, Cow::Borrowed("kPa abs"))
                }
            }
            "psi" => {
                let (value, unit) = self.display_psi_value(value);
                (value, Cow::Borrowed(unit))
            }
            "L/h" => {
                let (value, unit) = self.display_fuel_rate_value(value);
                (value, Cow::Borrowed(unit))
            }
            "g/s" => {
                let (value, unit) = self.display_mass_air_flow_value(value);
                (value, Cow::Borrowed(unit))
            }
            "km" => {
                let (value, unit) = self.display_distance_value(value);
                (value, Cow::Borrowed(unit))
            }
            "Nm" => {
                let (value, unit) = self.display_torque_value(value);
                (value, Cow::Borrowed(unit))
            }
            _ => (value, Cow::Borrowed(unit)),
        }
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

fn fuel_indicates_diesel(fuel: &str) -> bool {
    fuel.as_bytes()
        .windows(6)
        .any(|window| window.eq_ignore_ascii_case(b"diesel"))
}

fn format_dtc_alert(dtc: &Dtc) -> String {
    let status = match dtc.status {
        obd2_core::protocol::dtc::DtcStatus::Stored => "stored",
        obd2_core::protocol::dtc::DtcStatus::Pending => "pending",
        obd2_core::protocol::dtc::DtcStatus::Permanent => "permanent",
    };
    let description = dtc
        .description
        .as_deref()
        .unwrap_or("Diagnostic trouble code");
    if let Some(module) = &dtc.source_module {
        format!("DTC [{status}] {} ({module}): {description}", dtc.code)
    } else {
        format!("DTC [{status}] {}: {description}", dtc.code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_core::protocol::enhanced::{ReadingSource, Value};
    use obd2_core::vehicle::BusId;

    fn sample_discovery() -> DiscoveryState {
        DiscoveryState {
            selected_protocol: Protocol::Can11Bit500,
            protocol_choice_source: ProtocolSelectionSource::AutoDetect,
            active_bus_id: Some("can".into()),
            active_bus_description: Some("CAN".into()),
            active_bus_speed_bps: Some(500_000),
            resolved_modules: vec![ModuleId::new("ecm"), ModuleId::new("tcm")],
            visible_ecus: vec![VisibleEcu {
                id: "7e8".into(),
                bus: Some(BusId("can".into())),
                module: Some(ModuleId::new("ecm")),
                address: None,
                observation_count: 1,
            }],
        }
    }

    fn scalar_reading(value: f64, unit: &'static str) -> Reading {
        Reading {
            value: Value::Scalar(value),
            unit,
            timestamp: Instant::now(),
            raw_bytes: Vec::new(),
            source: ReadingSource::Live,
        }
    }

    fn threshold(
        pid: Pid,
        low_critical: Option<f64>,
        high_critical: Option<f64>,
    ) -> ResolvedThreshold {
        ResolvedThreshold {
            pid_code: pid.0,
            min_value: 0.0,
            max_value: 1000.0,
            low_warning: None,
            high_warning: None,
            low_critical,
            high_critical,
            unit: pid.unit().to_string(),
        }
    }

    #[test]
    fn test_connection_state_from_session_preserves_ignition_off() {
        let mapped =
            ConnectionState::from_session(&obd2_core::session::ConnectionState::IgnitionOff);
        assert_eq!(mapped, ConnectionState::IgnitionOff);
    }

    #[test]
    fn test_freeze_frame_cleared_on_disconnect() {
        let mut domain = DomainState::new(250);
        domain.freeze_frame_data = Some(FreezeFrameSnapshot {
            dtc_code: "P0420".into(),
            readings: vec![(Pid::ENGINE_RPM, 2500.0, "rpm")],
        });
        domain.freeze_frame_pending = true;

        domain.update(DomainMessage::ConnectionStatus(
            ConnectionState::Disconnected,
        ));

        assert!(domain.freeze_frame_data.is_none());
        assert!(!domain.freeze_frame_pending);
    }

    #[test]
    fn test_display_unit_value_converts_pressure_for_us_units() {
        let mut domain = DomainState::new(250);
        domain.speed_unit = SpeedUnit::Mph;

        let (value, unit) = domain.display_unit_value(100.0, "kPa");

        assert!((value - 14.503_773_77).abs() < 0.000_001);
        assert_eq!(unit, "psi");
    }

    #[test]
    fn test_display_unit_value_converts_abs_pressure_for_us_units() {
        let mut domain = DomainState::new(250);
        domain.speed_unit = SpeedUnit::Mph;

        let (value, unit) = domain.display_unit_value(100.0, "kPa abs");

        assert!((value - 14.503_773_77).abs() < 0.000_001);
        assert_eq!(unit, "psi abs");
    }

    #[test]
    fn test_display_unit_value_keeps_psi_for_us_units() {
        let mut domain = DomainState::new(250);
        domain.speed_unit = SpeedUnit::Mph;

        let (value, unit) = domain.display_unit_value(551.0, "psi");

        assert!((value - 551.0).abs() < 0.000_001);
        assert_eq!(unit, "psi");
    }

    #[test]
    fn test_display_pid_value_keeps_maf_metric_for_us_units() {
        let mut domain = DomainState::new(250);
        domain.speed_unit = SpeedUnit::Mph;

        let (value, unit) = domain.display_pid_value(Pid::MAF, 39.0);

        assert!((value - 39.0).abs() < 0.000_001);
        assert_eq!(unit, "g/s");
    }

    #[test]
    fn test_display_unit_value_keeps_maf_metric_for_us_units() {
        let mut domain = DomainState::new(250);
        domain.speed_unit = SpeedUnit::Mph;

        let (value, unit) = domain.display_unit_value(39.0, "g/s");

        assert!((value - 39.0).abs() < 0.000_001);
        assert_eq!(unit, "g/s");
    }

    #[test]
    fn test_display_mass_air_flow_keeps_metric_units() {
        let mut domain = DomainState::new(250);
        domain.speed_unit = SpeedUnit::Kmh;

        let (value, unit) = domain.display_mass_air_flow_value(39.0);

        assert_eq!(value, 39.0);
        assert_eq!(unit, "g/s");
    }

    #[test]
    fn test_stationary_off_or_cranking_rpm_does_not_alert() {
        let mut domain = DomainState::new(250);
        domain
            .thresholds_cache
            .insert(0x0C, threshold(Pid::ENGINE_RPM, Some(300.0), None));

        domain.update(DomainMessage::PidUpdate(
            Pid::ENGINE_RPM,
            scalar_reading(0.0, "rpm"),
        ));
        domain.update(DomainMessage::PidUpdate(
            Pid::ENGINE_RPM,
            scalar_reading(192.2, "rpm"),
        ));

        assert!(domain.active_alerts.is_empty());
        assert!(domain.alert_history.is_empty());
    }

    #[test]
    fn test_identical_threshold_alert_is_not_repeated_in_history() {
        let mut domain = DomainState::new(250);
        domain
            .thresholds_cache
            .insert(0x05, threshold(Pid::COOLANT_TEMP, None, Some(100.0)));

        domain.update(DomainMessage::PidUpdate(
            Pid::COOLANT_TEMP,
            scalar_reading(110.0, "\u{00B0}C"),
        ));
        domain.update(DomainMessage::PidUpdate(
            Pid::COOLANT_TEMP,
            scalar_reading(110.0, "\u{00B0}C"),
        ));

        assert_eq!(domain.active_alerts.len(), 1);
        assert_eq!(domain.alert_history.len(), 1);
    }

    #[test]
    fn test_enhanced_pid_target_registers_without_value() {
        let mut domain = DomainState::new(250);

        domain.update(DomainMessage::EnhancedPidTarget {
            did: 0x1543,
            module: "ecm".into(),
            name: "VGT Vane Position Actual".into(),
            unit: "%".into(),
            confidence: None,
            evidence: None,
        });

        assert_eq!(domain.enhanced_readings.len(), 1);
        let reading = &domain.enhanced_readings[0];
        assert_eq!(reading.did, 0x1543);
        assert_eq!(reading.module, "ecm");
        assert_eq!(reading.value, None);
        assert_eq!(reading.last_error, None);
    }

    #[test]
    fn test_enhanced_pid_update_fills_registered_target_value() {
        let mut domain = DomainState::new(250);

        domain.update(DomainMessage::EnhancedPidTarget {
            did: 0x1543,
            module: "ecm".into(),
            name: "VGT Vane Position Actual".into(),
            unit: "%".into(),
            confidence: None,
            evidence: None,
        });
        domain.update(DomainMessage::EnhancedPidUpdate {
            did: 0x1543,
            module: "ecm".into(),
            name: "VGT Vane Position Actual".into(),
            value: 72.5,
            unit: "%".into(),
            confidence: None,
            evidence: None,
        });

        assert_eq!(domain.enhanced_readings.len(), 1);
        assert_eq!(domain.enhanced_readings[0].value, Some(72.5));
        assert_eq!(domain.enhanced_readings[0].last_error, None);
    }

    #[test]
    fn test_enhanced_pid_error_marks_registered_target_without_alert_spam() {
        let mut domain = DomainState::new(250);

        domain.update(DomainMessage::EnhancedPidTarget {
            did: 0x1543,
            module: "ecm".into(),
            name: "VGT Vane Position Actual".into(),
            unit: "%".into(),
            confidence: None,
            evidence: None,
        });
        domain.update(DomainMessage::EnhancedPidError {
            did: 0x1543,
            module: "ecm".into(),
            name: "VGT Vane Position Actual".into(),
            unit: "%".into(),
            error: "parse error: no valid enhanced response payload".into(),
            confidence: None,
            evidence: None,
        });

        assert_eq!(domain.enhanced_readings.len(), 1);
        assert_eq!(domain.enhanced_readings[0].value, None);
        assert_eq!(
            domain.enhanced_readings[0].last_error.as_deref(),
            Some("parse error: no valid enhanced response payload")
        );
        assert!(domain.alert_history.is_empty());
    }

    #[test]
    fn test_dtc_update_appends_alert_history() {
        let mut domain = DomainState::new(250);
        let mut dtc = Dtc::from_code("P2563");
        dtc.status = obd2_core::protocol::dtc::DtcStatus::Stored;
        dtc.description = Some("Turbocharger Boost Control Position Sensor Performance".into());
        dtc.source_module = Some("ecm".into());

        domain.update(DomainMessage::DtcUpdate(vec![dtc]));

        assert_eq!(domain.stored_dtcs.len(), 1);
        assert!(domain
            .alert_history
            .iter()
            .any(|message| message.contains("DTC [stored] P2563 (ecm)")));
    }

    #[test]
    fn test_diagnostic_scan_update_clears_on_disconnect() {
        let mut domain = DomainState::new(250);

        domain.update(DomainMessage::DiagnosticScanUpdate(vec![
            DiagnosticScanEntry {
                scope: DiagnosticScanScope::Broadcast,
                service: DtcService::Pending,
                result: DiagnosticScanResult::Empty,
            },
        ]));

        assert_eq!(domain.diagnostic_scan.len(), 1);
        assert!(domain.diagnostic_scan_updated.is_some());

        domain.update(DomainMessage::ConnectionStatus(
            ConnectionState::Disconnected,
        ));

        assert!(domain.diagnostic_scan.is_empty());
        assert!(domain.diagnostic_scan_updated.is_none());
    }

    #[test]
    fn test_readiness_stored_and_cleared_on_disconnect() {
        use obd2_core::protocol::service::MonitorStatus;

        let mut domain = DomainState::new(250);
        let status = ReadinessStatus {
            mil_on: false,
            dtc_count: 0,
            compression_ignition: false,
            monitors: vec![MonitorStatus {
                name: "Catalyst".into(),
                supported: true,
                complete: true,
            }],
        };

        domain.update(DomainMessage::ReadinessUpdate(status));
        assert!(domain.readiness.is_some());
        assert_eq!(domain.readiness.as_ref().unwrap().monitors.len(), 1);

        domain.update(DomainMessage::ConnectionStatus(
            ConnectionState::Disconnected,
        ));
        assert!(domain.readiness.is_none());
    }

    #[test]
    fn test_readiness_ignition_label_prefers_identified_diesel_vehicle() {
        let mut domain = DomainState::new(250);
        domain.vehicle_info = Some(VehicleInfo {
            vin: "1GTHK29294E391526".into(),
            year: Some(2004),
            make: Some("GMC".into()),
            model: Some("Sierra".into()),
            trim: None,
            engine_family_id: None,
            engine_family_code: Some("LLY".into()),
            transmission_type: None,
            drive_type: None,
            fuel_type: Some("Diesel".into()),
            displacement_l: Some(6.6),
            cylinders: Some(8),
        });
        let status = ReadinessStatus {
            mil_on: false,
            dtc_count: 0,
            compression_ignition: false,
            monitors: Vec::new(),
        };

        assert_eq!(domain.readiness_ignition_label(&status), "Diesel");
    }

    #[test]
    fn test_readiness_ignition_label_uses_pid_bits_without_vehicle_override() {
        let domain = DomainState::new(250);
        let status = ReadinessStatus {
            mil_on: false,
            dtc_count: 0,
            compression_ignition: false,
            monitors: Vec::new(),
        };

        assert_eq!(domain.readiness_ignition_label(&status), "Spark");
    }

    #[test]
    fn test_disconnect_clears_discovery_and_enhanced_state() {
        let mut domain = DomainState::new(250);
        domain.update(DomainMessage::DiscoveryUpdated(sample_discovery()));
        domain.enhanced_readings.push(EnhancedReading {
            did: 0x1234,
            module: "ecm".into(),
            name: "Test".into(),
            value: Some(1.0),
            unit: "u".into(),
            last_error: None,
            confidence: None,
            evidence: None,
        });
        domain.o2_readings.push(O2Reading {
            test_name: "TID".into(),
            sensor: "O2".into(),
            value: 1.0,
            unit: "V".into(),
        });

        domain.update(DomainMessage::ConnectionStatus(
            ConnectionState::Disconnected,
        ));

        assert!(domain.discovery.is_none());
        assert!(domain.enhanced_readings.is_empty());
        assert!(domain.o2_readings.is_empty());
    }
}
