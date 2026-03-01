use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use crate::driving::DrivingBehavior;
use crate::fuel_economy::{FuelEconomyState, SensorSnapshot};
use crate::obd2::{AdapterInfo, Dtc, Pid, PidReading, VehicleData};
use crate::recording::storage::StorageManager;
use crate::recording::RecordingState;
use obd2_db::models::{Alert, AlertLevel, ResolvedThreshold, VehicleInfo};

/// Domain-only events (no scanner/UI messages).
#[derive(Debug)]
pub enum DomainMessage {
    PidUpdate(Pid, PidReading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    ConnectionStatus(ConnectionState),
    AdapterDetected(AdapterInfo),
    Error(String),
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
    pub fuel_economy: FuelEconomyState,
    pub driving: DrivingBehavior,
    pub recording: RecordingState,
    pub storage_manager: Option<StorageManager>,
    last_pid_update: Instant,
}

impl DomainState {
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            vehicle: VehicleData::default(),
            connection: ConnectionState::Disconnected,
            adapter_info: None,
            last_error: None,
            poll_interval_ms,
            temp_unit: TemperatureUnit::Celsius,
            speed_unit: SpeedUnit::Kmh,
            vehicle_info: None,
            active_alerts: Vec::new(),
            alert_history: VecDeque::new(),
            thresholds_cache: HashMap::new(),
            stored_dtcs: Vec::new(),
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
                    let raw = reading.raw_bytes.as_deref().unwrap_or(&[]);
                    let _ = writer.write_pid(offset_ms, pid.code(), reading.value, raw);
                }
                DomainMessage::VoltageUpdate(v) => {
                    let _ = writer.write_voltage(offset_ms, *v);
                }
                DomainMessage::DtcUpdate(dtcs) => {
                    for dtc in dtcs {
                        let _ = writer.write_dtc(offset_ms, &dtc.code);
                    }
                }
                _ => {}
            }
        }

        match msg {
            DomainMessage::PidUpdate(pid, reading) => {
                let value = reading.value;
                match pid {
                    Pid::EngineRpm => {
                        self.vehicle.rpm_history.push(reading.value as u64);
                        self.vehicle.rpm = Some(reading);
                    }
                    Pid::VehicleSpeed => {
                        self.vehicle.speed_history.push(reading.value as u64);
                        let throttle = self
                            .vehicle
                            .throttle_position
                            .as_ref()
                            .map(|r| r.value)
                            .unwrap_or(0.0);
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
                    Pid::TimingAdvance => {
                        self.vehicle.timing_advance = Some(reading);
                    }
                    Pid::RunTimeSinceStart => {
                        self.vehicle.run_time = Some(reading);
                    }
                    Pid::DistanceWithMil => {
                        self.vehicle.distance_with_mil = Some(reading);
                    }
                    Pid::FuelRailGaugePressure => {
                        self.vehicle.fuel_rail_gauge_pressure = Some(reading);
                    }
                    Pid::CommandedEgr => {
                        self.vehicle.commanded_egr = Some(reading);
                    }
                    Pid::CommandedEvapPurge => {
                        self.vehicle.commanded_evap_purge = Some(reading);
                    }
                    Pid::DistanceSinceDtcClear => {
                        self.vehicle.distance_since_dtc_clear = Some(reading);
                    }
                    Pid::AbsoluteLoad => {
                        self.vehicle.absolute_load = Some(reading);
                    }
                    Pid::CommandedEquivRatio => {
                        self.vehicle.commanded_equiv_ratio = Some(reading);
                    }
                    Pid::RelativeThrottlePos => {
                        self.vehicle.relative_throttle_pos = Some(reading);
                    }
                    Pid::AbsThrottlePosB => {
                        self.vehicle.abs_throttle_pos_b = Some(reading);
                    }
                    Pid::AccelPedalPosD => {
                        self.vehicle.accel_pedal_pos_d = Some(reading);
                    }
                    Pid::AccelPedalPosE => {
                        self.vehicle.accel_pedal_pos_e = Some(reading);
                    }
                    Pid::CommandedThrottleActuator => {
                        self.vehicle.commanded_throttle_actuator = Some(reading);
                    }
                    Pid::FuelRailAbsPressure => {
                        self.vehicle.fuel_rail_abs_pressure = Some(reading);
                    }
                    Pid::DemandedTorque => {
                        self.vehicle.demanded_torque = Some(reading);
                    }
                    Pid::ActualTorque => {
                        self.vehicle.actual_torque = Some(reading);
                    }
                    Pid::ReferenceTorque => {
                        self.vehicle.reference_torque = Some(reading);
                    }
                }
                // Recompute derived boost pressure (MAP - Barometric)
                self.vehicle.boost_pressure =
                    match (&self.vehicle.intake_map, &self.vehicle.barometric_pressure) {
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
                    "Adapter detected: {} ({})",
                    info.chipset,
                    info.firmware_version
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
        self.vehicle.speed.as_ref().map(|r| match self.speed_unit {
            SpeedUnit::Kmh => (r.value, "km/h"),
            SpeedUnit::Mph => (r.value * 0.621371, "mph"),
        })
    }
}
