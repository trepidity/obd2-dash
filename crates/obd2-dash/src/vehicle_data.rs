use std::collections::VecDeque;

use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::enhanced::Reading;

/// Ring-buffer history for sparkline display.
const HISTORY_CAPACITY: usize = 120; // 30s at 4 Hz

#[derive(Debug, Clone, Default)]
pub struct PidHistory {
    pub readings: VecDeque<u64>,
}

impl PidHistory {
    pub fn new() -> Self {
        Self {
            readings: VecDeque::with_capacity(HISTORY_CAPACITY),
        }
    }

    pub fn push(&mut self, value: u64) {
        if self.readings.len() >= HISTORY_CAPACITY {
            self.readings.pop_front();
        }
        self.readings.push_back(value);
    }
}

/// Aggregated vehicle data from OBD-II readings.
/// Stores the latest f64 value for each PID (extracted via `Reading::value.as_f64()`).
#[derive(Debug, Clone, Default)]
pub struct VehicleData {
    // Core PIDs
    pub rpm: Option<f64>,
    pub speed: Option<f64>,
    pub coolant_temp: Option<f64>,
    pub engine_load: Option<f64>,
    pub battery_voltage: Option<f64>,

    // Fuel trims
    pub short_fuel_trim_b1: Option<f64>,
    pub long_fuel_trim_b1: Option<f64>,
    pub short_fuel_trim_b2: Option<f64>,
    pub long_fuel_trim_b2: Option<f64>,

    // Engine / intake
    pub fuel_pressure: Option<f64>,
    pub intake_map: Option<f64>,
    pub intake_air_temp: Option<f64>,
    pub maf: Option<f64>,
    pub throttle_position: Option<f64>,

    // Fuel system
    pub fuel_tank_level: Option<f64>,
    pub engine_fuel_rate: Option<f64>,

    // Environment / system
    pub barometric_pressure: Option<f64>,
    pub control_module_voltage: Option<f64>,
    pub ambient_air_temp: Option<f64>,

    // Temperatures
    pub engine_oil_temp: Option<f64>,
    pub transmission_temp: Option<f64>,
    pub catalyst_temp_b1s1: Option<f64>,
    pub catalyst_temp_b2s1: Option<f64>,
    pub catalyst_temp_b1s2: Option<f64>,
    pub catalyst_temp_b2s2: Option<f64>,

    // Pressures (extended)
    pub oil_pressure: Option<f64>,
    pub fuel_rail_gauge_pressure: Option<f64>,
    pub fuel_rail_abs_pressure: Option<f64>,

    // Timing / torque
    pub timing_advance: Option<f64>,
    pub demanded_torque: Option<f64>,
    pub actual_torque: Option<f64>,
    pub reference_torque: Option<f64>,

    // Throttle / pedal
    pub relative_throttle_pos: Option<f64>,
    pub abs_throttle_pos_b: Option<f64>,
    pub accel_pedal_pos_d: Option<f64>,
    pub accel_pedal_pos_e: Option<f64>,
    pub commanded_throttle_actuator: Option<f64>,

    // EGR / EVAP / load / equiv ratio
    pub commanded_egr: Option<f64>,
    pub commanded_evap_purge: Option<f64>,
    pub absolute_load: Option<f64>,
    pub commanded_equiv_ratio: Option<f64>,

    // Distance / run time
    pub run_time: Option<f64>,
    pub distance_with_mil: Option<f64>,
    pub distance_since_dtc_clear: Option<f64>,

    // Derived values (computed, not polled)
    pub boost_pressure: Option<f64>,

    // Histories
    pub rpm_history: PidHistory,
    pub speed_history: PidHistory,
    pub throttle_history: PidHistory,
    pub load_history: PidHistory,
}

impl VehicleData {
    /// Apply a reading from a PID to the appropriate field.
    /// Extracts the f64 value from the Reading.
    pub fn apply_reading(&mut self, pid: Pid, reading: &Reading) {
        let value = match reading.value.as_f64() {
            Ok(v) => v,
            Err(_) => return, // Skip non-scalar values (bitfields, etc.)
        };

        match pid {
            p if p == Pid::ENGINE_RPM => {
                self.rpm_history.push(value as u64);
                self.rpm = Some(value);
            }
            p if p == Pid::VEHICLE_SPEED => {
                self.speed_history.push(value as u64);
                self.speed = Some(value);
            }
            p if p == Pid::COOLANT_TEMP => {
                self.coolant_temp = Some(value);
            }
            p if p == Pid::ENGINE_LOAD => {
                self.load_history.push(value as u64);
                self.engine_load = Some(value);
            }
            p if p == Pid::SHORT_FUEL_TRIM_B1 => {
                self.short_fuel_trim_b1 = Some(value);
            }
            p if p == Pid::LONG_FUEL_TRIM_B1 => {
                self.long_fuel_trim_b1 = Some(value);
            }
            p if p == Pid::SHORT_FUEL_TRIM_B2 => {
                self.short_fuel_trim_b2 = Some(value);
            }
            p if p == Pid::LONG_FUEL_TRIM_B2 => {
                self.long_fuel_trim_b2 = Some(value);
            }
            p if p == Pid::FUEL_PRESSURE => {
                self.fuel_pressure = Some(value);
            }
            p if p == Pid::INTAKE_MAP => {
                self.intake_map = Some(value);
            }
            p if p == Pid::INTAKE_AIR_TEMP => {
                self.intake_air_temp = Some(value);
            }
            p if p == Pid::MAF => {
                self.maf = Some(value);
            }
            p if p == Pid::THROTTLE_POSITION => {
                self.throttle_history.push(value as u64);
                self.throttle_position = Some(value);
            }
            p if p == Pid::FUEL_TANK_LEVEL => {
                self.fuel_tank_level = Some(value);
            }
            p if p == Pid::BAROMETRIC_PRESSURE => {
                self.barometric_pressure = Some(value);
            }
            p if p == Pid::CATALYST_TEMP_B1S1 => {
                self.catalyst_temp_b1s1 = Some(value);
            }
            p if p == Pid::CATALYST_TEMP_B2S1 => {
                self.catalyst_temp_b2s1 = Some(value);
            }
            p if p == Pid::CATALYST_TEMP_B1S2 => {
                self.catalyst_temp_b1s2 = Some(value);
            }
            p if p == Pid::CATALYST_TEMP_B2S2 => {
                self.catalyst_temp_b2s2 = Some(value);
            }
            p if p == Pid::CONTROL_MODULE_VOLTAGE => {
                self.control_module_voltage = Some(value);
            }
            p if p == Pid::AMBIENT_AIR_TEMP => {
                self.ambient_air_temp = Some(value);
            }
            p if p == Pid::ENGINE_OIL_TEMP => {
                self.engine_oil_temp = Some(value);
            }
            p if p == Pid::ENGINE_FUEL_RATE => {
                self.engine_fuel_rate = Some(value);
            }
            p if p == Pid::TIMING_ADVANCE => {
                self.timing_advance = Some(value);
            }
            p if p == Pid::RUN_TIME => {
                self.run_time = Some(value);
            }
            p if p == Pid::DISTANCE_WITH_MIL => {
                self.distance_with_mil = Some(value);
            }
            p if p == Pid::FUEL_RAIL_GAUGE_PRESSURE => {
                self.fuel_rail_gauge_pressure = Some(value);
            }
            p if p == Pid::COMMANDED_EGR => {
                self.commanded_egr = Some(value);
            }
            p if p == Pid::COMMANDED_EVAP_PURGE => {
                self.commanded_evap_purge = Some(value);
            }
            p if p == Pid::DISTANCE_SINCE_CLEAR => {
                self.distance_since_dtc_clear = Some(value);
            }
            p if p == Pid::ABSOLUTE_LOAD => {
                self.absolute_load = Some(value);
            }
            p if p == Pid::COMMANDED_EQUIV_RATIO => {
                self.commanded_equiv_ratio = Some(value);
            }
            p if p == Pid::RELATIVE_THROTTLE_POS => {
                self.relative_throttle_pos = Some(value);
            }
            p if p == Pid::ABS_THROTTLE_POS_B => {
                self.abs_throttle_pos_b = Some(value);
            }
            p if p == Pid::ACCEL_PEDAL_POS_D => {
                self.accel_pedal_pos_d = Some(value);
            }
            p if p == Pid::ACCEL_PEDAL_POS_E => {
                self.accel_pedal_pos_e = Some(value);
            }
            p if p == Pid::COMMANDED_THROTTLE_ACTUATOR => {
                self.commanded_throttle_actuator = Some(value);
            }
            p if p == Pid::FUEL_RAIL_ABS_PRESSURE => {
                self.fuel_rail_abs_pressure = Some(value);
            }
            p if p == Pid::DEMANDED_TORQUE => {
                self.demanded_torque = Some(value);
            }
            p if p == Pid::ACTUAL_TORQUE => {
                self.actual_torque = Some(value);
            }
            p if p == Pid::REFERENCE_TORQUE => {
                self.reference_torque = Some(value);
            }
            _ => {
                // Unknown PID — ignore
            }
        }

        // Recompute derived boost pressure (MAP - Barometric)
        self.boost_pressure = match (self.intake_map, self.barometric_pressure) {
            (Some(map), Some(baro)) => Some(map - baro),
            _ => None,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use obd2_core::protocol::enhanced::{ReadingSource, Value};

    fn make_reading(value: f64) -> Reading {
        Reading {
            value: Value::Scalar(value),
            unit: "",
            timestamp: Instant::now(),
            raw_bytes: vec![],
            source: ReadingSource::Live,
        }
    }

    #[test]
    fn test_apply_reading_rpm() {
        let mut data = VehicleData::default();
        assert!(data.rpm.is_none());

        data.apply_reading(Pid::ENGINE_RPM, &make_reading(2500.0));

        assert_eq!(data.rpm, Some(2500.0));
        assert_eq!(data.rpm_history.readings.len(), 1);
        assert_eq!(data.rpm_history.readings[0], 2500);
    }

    #[test]
    fn test_apply_reading_speed_and_history() {
        let mut data = VehicleData::default();

        for i in 0..5 {
            data.apply_reading(Pid::VEHICLE_SPEED, &make_reading(i as f64 * 20.0));
        }

        assert_eq!(data.speed, Some(80.0));
        assert_eq!(data.speed_history.readings.len(), 5);
    }

    #[test]
    fn test_boost_pressure_derived() {
        let mut data = VehicleData::default();

        // Only MAP set — no boost yet
        data.apply_reading(Pid::INTAKE_MAP, &make_reading(120.0));
        assert!(data.boost_pressure.is_none());

        // Now set barometric — boost = MAP - baro
        data.apply_reading(Pid::BAROMETRIC_PRESSURE, &make_reading(101.0));
        assert!((data.boost_pressure.unwrap() - 19.0).abs() < 0.001);
    }

    #[test]
    fn test_boost_pressure_updates_on_map_change() {
        let mut data = VehicleData::default();
        data.apply_reading(Pid::BAROMETRIC_PRESSURE, &make_reading(101.0));
        data.apply_reading(Pid::INTAKE_MAP, &make_reading(150.0));
        assert!((data.boost_pressure.unwrap() - 49.0).abs() < 0.001);

        // MAP drops below baro — negative boost (vacuum)
        data.apply_reading(Pid::INTAKE_MAP, &make_reading(80.0));
        assert!((data.boost_pressure.unwrap() - (-21.0)).abs() < 0.001);
    }

    #[test]
    fn test_apply_reading_temperatures() {
        let mut data = VehicleData::default();
        data.apply_reading(Pid::COOLANT_TEMP, &make_reading(92.0));
        data.apply_reading(Pid::ENGINE_OIL_TEMP, &make_reading(87.0));
        data.apply_reading(Pid::CATALYST_TEMP_B1S1, &make_reading(412.0));

        assert_eq!(data.coolant_temp, Some(92.0));
        assert_eq!(data.engine_oil_temp, Some(87.0));
        assert_eq!(data.catalyst_temp_b1s1, Some(412.0));
    }

    #[test]
    fn test_apply_reading_fuel_trims() {
        let mut data = VehicleData::default();
        data.apply_reading(Pid::SHORT_FUEL_TRIM_B1, &make_reading(2.3));
        data.apply_reading(Pid::LONG_FUEL_TRIM_B1, &make_reading(-1.1));
        data.apply_reading(Pid::SHORT_FUEL_TRIM_B2, &make_reading(3.5));
        data.apply_reading(Pid::LONG_FUEL_TRIM_B2, &make_reading(-0.5));

        assert_eq!(data.short_fuel_trim_b1, Some(2.3));
        assert_eq!(data.long_fuel_trim_b1, Some(-1.1));
        assert_eq!(data.short_fuel_trim_b2, Some(3.5));
        assert_eq!(data.long_fuel_trim_b2, Some(-0.5));
    }

    #[test]
    fn test_pid_history_capacity() {
        let mut history = PidHistory::new();
        for i in 0..200 {
            history.push(i);
        }
        assert_eq!(history.readings.len(), HISTORY_CAPACITY);
        // Oldest values should have been dropped
        assert_eq!(history.readings[0], 200 - HISTORY_CAPACITY as u64);
    }

    #[test]
    fn test_unknown_pid_ignored() {
        let mut data = VehicleData::default();
        // PID 0xFF is not mapped — should be silently ignored
        data.apply_reading(Pid(0xFF), &make_reading(42.0));
        // No panic, no state change to any known field
        assert!(data.rpm.is_none());
        assert!(data.speed.is_none());
    }
}
