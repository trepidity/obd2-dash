use std::collections::VecDeque;
use std::fmt;
use std::time::Instant;
use thiserror::Error;

#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum Obd2Error {
    #[error("serial port error: {0}")]
    Serial(#[from] serialport::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ELM327 returned no data")]
    NoData,

    #[error("ELM327 returned error: {0}")]
    DeviceError(String),

    #[error("failed to parse response: {0}")]
    ParseError(String),

    #[error("connection timeout")]
    Timeout,

    #[error("unsupported PID: {0:#04x}")]
    UnsupportedPid(u8),

    #[error("BLE error: {0}")]
    Ble(String),
}

/// Detected adapter chipset family.
#[derive(Debug, Clone, PartialEq)]
pub enum Chipset {
    /// Cheap ELM327 clones (v1.3/v1.4/v1.5) — limited protocol support.
    Elm327Clone,
    /// Genuine ELM327 (v2.0+).
    Elm327Genuine,
    /// STN1110/STN2120 (OBDLink) — responds to STI/STDI.
    Stn,
    /// Could not determine chipset.
    Unknown,
}

impl fmt::Display for Chipset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Chipset::Elm327Clone => write!(f, "ELM327 Clone"),
            Chipset::Elm327Genuine => write!(f, "ELM327"),
            Chipset::Stn => write!(f, "STN"),
            Chipset::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Capability flags derived from the detected chipset.
#[derive(Debug, Clone)]
pub struct AdapterCaps {
    /// Safe to offer "clear codes" (mode 04).
    pub can_clear_dtcs: bool,
    /// HS-CAN + MS-CAN switching (STP protocols).
    pub dual_can: bool,
    /// Manufacturer-specific mode $22 extended PIDs.
    pub enhanced_diag: bool,
    /// ATRV battery voltage command.
    pub battery_voltage: bool,
    /// ATAT adaptive timing support.
    pub adaptive_timing: bool,
}

/// Information about the connected OBD2 adapter.
#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub chipset: Chipset,
    /// Raw firmware version string, e.g. "ELM327 v1.4b", "STN2120 v4.2.0".
    pub firmware_version: String,
    pub caps: AdapterCaps,
}

impl AdapterInfo {
    /// Classify chipset from the ATZ firmware string and optional STI probe result.
    pub fn detect(atz_response: &str, sti_response: Option<&str>) -> Self {
        // If STI got a valid response (not "?" or empty), it's an STN chip
        let is_stn = sti_response
            .map(|s| !s.is_empty() && !s.starts_with('?'))
            .unwrap_or(false);

        let firmware_version = atz_response.trim().to_string();

        let chipset = if is_stn {
            Chipset::Stn
        } else if let Some(version) = extract_elm_version(&firmware_version) {
            if version >= 2.0 {
                Chipset::Elm327Genuine
            } else {
                Chipset::Elm327Clone
            }
        } else {
            Chipset::Unknown
        };

        let caps = AdapterCaps::from_chipset(&chipset);

        Self {
            chipset,
            firmware_version,
            caps,
        }
    }
}

impl AdapterCaps {
    fn from_chipset(chipset: &Chipset) -> Self {
        match chipset {
            Chipset::Stn => Self {
                can_clear_dtcs: true,
                dual_can: true,
                enhanced_diag: true,
                battery_voltage: true,
                adaptive_timing: true,
            },
            Chipset::Elm327Genuine => Self {
                can_clear_dtcs: true,
                dual_can: false,
                enhanced_diag: false,
                battery_voltage: true,
                adaptive_timing: true,
            },
            Chipset::Elm327Clone => Self {
                can_clear_dtcs: false,
                dual_can: false,
                enhanced_diag: false,
                battery_voltage: true,
                adaptive_timing: false,
            },
            Chipset::Unknown => Self {
                can_clear_dtcs: false,
                dual_can: false,
                enhanced_diag: false,
                battery_voltage: true,
                adaptive_timing: false,
            },
        }
    }
}

/// Extract the numeric version from an ELM327 firmware string.
/// e.g. "ELM327 v1.4b" → Some(1.4), "ELM327 v2.1" → Some(2.1)
fn extract_elm_version(firmware: &str) -> Option<f64> {
    let upper = firmware.to_uppercase();
    if !upper.contains("ELM327") {
        return None;
    }
    // Find 'v' or 'V' followed by digits
    let v_pos = upper.find('V')?;
    let after_v = &firmware[v_pos + 1..];
    // Take digits and first dot
    let version_str: String = after_v
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    version_str.parse::<f64>().ok()
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[test]
    fn test_elm327_clone_v14() {
        let info = AdapterInfo::detect("ELM327 v1.4b", None);
        assert_eq!(info.chipset, Chipset::Elm327Clone);
        assert!(!info.caps.can_clear_dtcs);
        assert!(!info.caps.adaptive_timing);
    }

    #[test]
    fn test_elm327_clone_v15() {
        let info = AdapterInfo::detect("ELM327 v1.5", None);
        assert_eq!(info.chipset, Chipset::Elm327Clone);
    }

    #[test]
    fn test_elm327_genuine_v21() {
        let info = AdapterInfo::detect("ELM327 v2.1", None);
        assert_eq!(info.chipset, Chipset::Elm327Genuine);
        assert!(info.caps.can_clear_dtcs);
        assert!(info.caps.adaptive_timing);
        assert!(!info.caps.dual_can);
    }

    #[test]
    fn test_stn_detected_via_sti() {
        let info = AdapterInfo::detect("ELM327 v1.4b", Some("STN2120 v4.2.0"));
        assert_eq!(info.chipset, Chipset::Stn);
        assert!(info.caps.can_clear_dtcs);
        assert!(info.caps.dual_can);
        assert!(info.caps.enhanced_diag);
    }

    #[test]
    fn test_sti_question_mark_not_stn() {
        let info = AdapterInfo::detect("ELM327 v1.5", Some("?"));
        assert_eq!(info.chipset, Chipset::Elm327Clone);
    }

    #[test]
    fn test_unknown_firmware() {
        let info = AdapterInfo::detect("RANDOM ADAPTER", None);
        assert_eq!(info.chipset, Chipset::Unknown);
        assert!(!info.caps.can_clear_dtcs);
    }

    #[test]
    fn test_extract_elm_version() {
        assert_eq!(extract_elm_version("ELM327 v1.4b"), Some(1.4));
        assert_eq!(extract_elm_version("ELM327 v2.1"), Some(2.1));
        assert_eq!(extract_elm_version("ELM327 v1.5"), Some(1.5));
        assert_eq!(extract_elm_version("RANDOM CHIP"), None);
    }

    #[test]
    fn test_chipset_display() {
        assert_eq!(format!("{}", Chipset::Elm327Clone), "ELM327 Clone");
        assert_eq!(format!("{}", Chipset::Elm327Genuine), "ELM327");
        assert_eq!(format!("{}", Chipset::Stn), "STN");
        assert_eq!(format!("{}", Chipset::Unknown), "Unknown");
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct PidReading {
    pub value: f64,
    pub unit: &'static str,
    pub timestamp: Instant,
}

impl PidReading {
    pub fn new(value: f64, unit: &'static str) -> Self {
        Self {
            value,
            unit,
            timestamp: Instant::now(),
        }
    }
}

/// Ring-buffer history for sparkline display.
const HISTORY_CAPACITY: usize = 120; // 30s at 4 Hz

#[derive(Debug, Clone)]
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

impl Default for PidHistory {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct VehicleData {
    // Original 4 PIDs
    pub rpm: Option<PidReading>,
    pub speed: Option<PidReading>,
    pub coolant_temp: Option<PidReading>,
    pub engine_load: Option<PidReading>,
    pub battery_voltage: Option<f64>,

    // Fuel trims
    pub short_fuel_trim_b1: Option<PidReading>,
    pub long_fuel_trim_b1: Option<PidReading>,
    pub short_fuel_trim_b2: Option<PidReading>,
    pub long_fuel_trim_b2: Option<PidReading>,

    // Engine / intake
    pub fuel_pressure: Option<PidReading>,
    pub intake_map: Option<PidReading>,
    pub intake_air_temp: Option<PidReading>,
    pub maf: Option<PidReading>,
    pub throttle_position: Option<PidReading>,

    // Fuel system
    pub fuel_tank_level: Option<PidReading>,
    pub engine_fuel_rate: Option<PidReading>,

    // Environment / system
    pub barometric_pressure: Option<PidReading>,
    pub control_module_voltage: Option<PidReading>,
    pub ambient_air_temp: Option<PidReading>,

    // Temperatures
    pub engine_oil_temp: Option<PidReading>,
    pub transmission_temp: Option<PidReading>,
    pub catalyst_temp_b1s1: Option<PidReading>,
    pub catalyst_temp_b2s1: Option<PidReading>,
    pub catalyst_temp_b1s2: Option<PidReading>,
    pub catalyst_temp_b2s2: Option<PidReading>,

    // Pressures (extended)
    pub oil_pressure: Option<PidReading>,
    pub fuel_rail_gauge_pressure: Option<PidReading>,
    pub fuel_rail_abs_pressure: Option<PidReading>,

    // Timing / torque
    pub timing_advance: Option<PidReading>,
    pub demanded_torque: Option<PidReading>,
    pub actual_torque: Option<PidReading>,
    pub reference_torque: Option<PidReading>,

    // Throttle / pedal
    pub relative_throttle_pos: Option<PidReading>,
    pub abs_throttle_pos_b: Option<PidReading>,
    pub accel_pedal_pos_d: Option<PidReading>,
    pub accel_pedal_pos_e: Option<PidReading>,
    pub commanded_throttle_actuator: Option<PidReading>,

    // EGR / EVAP / load / equiv ratio
    pub commanded_egr: Option<PidReading>,
    pub commanded_evap_purge: Option<PidReading>,
    pub absolute_load: Option<PidReading>,
    pub commanded_equiv_ratio: Option<PidReading>,

    // Distance / run time
    pub run_time: Option<PidReading>,
    pub distance_with_mil: Option<PidReading>,
    pub distance_since_dtc_clear: Option<PidReading>,

    // Derived values (computed, not polled)
    pub boost_pressure: Option<f64>,

    // Histories
    pub rpm_history: PidHistory,
    pub speed_history: PidHistory,
    pub throttle_history: PidHistory,
    pub load_history: PidHistory,
}

impl Default for VehicleData {
    fn default() -> Self {
        Self {
            rpm: None,
            speed: None,
            coolant_temp: None,
            engine_load: None,
            battery_voltage: None,
            short_fuel_trim_b1: None,
            long_fuel_trim_b1: None,
            short_fuel_trim_b2: None,
            long_fuel_trim_b2: None,
            fuel_pressure: None,
            intake_map: None,
            intake_air_temp: None,
            maf: None,
            throttle_position: None,
            fuel_tank_level: None,
            engine_fuel_rate: None,
            barometric_pressure: None,
            control_module_voltage: None,
            ambient_air_temp: None,
            engine_oil_temp: None,
            transmission_temp: None,
            catalyst_temp_b1s1: None,
            catalyst_temp_b2s1: None,
            catalyst_temp_b1s2: None,
            catalyst_temp_b2s2: None,
            oil_pressure: None,
            fuel_rail_gauge_pressure: None,
            fuel_rail_abs_pressure: None,
            timing_advance: None,
            demanded_torque: None,
            actual_torque: None,
            reference_torque: None,
            relative_throttle_pos: None,
            abs_throttle_pos_b: None,
            accel_pedal_pos_d: None,
            accel_pedal_pos_e: None,
            commanded_throttle_actuator: None,
            commanded_egr: None,
            commanded_evap_purge: None,
            absolute_load: None,
            commanded_equiv_ratio: None,
            run_time: None,
            distance_with_mil: None,
            distance_since_dtc_clear: None,
            boost_pressure: None,
            rpm_history: PidHistory::new(),
            speed_history: PidHistory::new(),
            throttle_history: PidHistory::new(),
            load_history: PidHistory::new(),
        }
    }
}
