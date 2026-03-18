//! Diagnostic Trouble Code (DTC) support — decoding, descriptions, and mock scenarios.

/// Top-level DTC category derived from the first two bits of byte A.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DtcCategory {
    Powertrain, // P
    Chassis,    // C
    Body,       // B
    Network,    // U
}

impl DtcCategory {
    pub fn prefix(self) -> char {
        match self {
            Self::Powertrain => 'P',
            Self::Chassis => 'C',
            Self::Body => 'B',
            Self::Network => 'U',
        }
    }
}

/// A single Diagnostic Trouble Code.
#[derive(Debug, Clone)]
pub struct Dtc {
    pub code: String,
    pub description: &'static str,
    pub category: DtcCategory,
}

impl Dtc {
    /// Decode a DTC from two raw OBD-II bytes (Mode 03 response pair).
    ///
    /// Byte layout:
    ///   Bits 15-14 of A → category (P/C/B/U)
    ///   Bits 13-12 of A → second character (0-3)
    ///   Bits 11-8  of A → third character (hex digit)
    ///   Bits  7-4  of B → fourth character (hex digit)
    ///   Bits  3-0  of B → fifth character (hex digit)
    pub fn from_bytes(a: u8, b: u8) -> Self {
        let category = match (a >> 6) & 0x03 {
            0 => DtcCategory::Powertrain,
            1 => DtcCategory::Chassis,
            2 => DtcCategory::Body,
            _ => DtcCategory::Network,
        };

        let second = (a >> 4) & 0x03;
        let third = a & 0x0F;
        let fourth = (b >> 4) & 0x0F;
        let fifth = b & 0x0F;

        let code = format!(
            "{}{}{:X}{:X}{:X}",
            category.prefix(),
            second,
            third,
            fourth,
            fifth
        );

        let description = dtc_description(&code);
        Dtc {
            code,
            description,
            category,
        }
    }

    /// Create a DTC directly from a code string (e.g. "P0420").
    pub fn from_code(code: &str) -> Self {
        let category = match code.chars().next() {
            Some('P') => DtcCategory::Powertrain,
            Some('C') => DtcCategory::Chassis,
            Some('B') => DtcCategory::Body,
            Some('U') => DtcCategory::Network,
            _ => DtcCategory::Powertrain,
        };
        let description = dtc_description(code);
        Dtc {
            code: code.to_string(),
            description,
            category,
        }
    }
}

/// Look up a human-readable description for a DTC code.
///
/// Returns a generic description if the specific code is not in the database.
/// Covers ~180 common OBD-II codes (powertrain, chassis, body, network).
pub fn dtc_description(code: &str) -> &'static str {
    match code {
        // VVT / camshaft
        "P0010" => "Intake camshaft position actuator circuit (Bank 1)",
        "P0011" => "Intake camshaft position timing over-advanced (Bank 1)",
        "P0014" => "Exhaust camshaft position timing over-advanced (Bank 1)",
        "P0016" => "Crankshaft position / camshaft position correlation (Bank 1 Sensor A)",

        // O2 sensor heaters
        "P0030" => "HO2S heater control circuit (Bank 1 Sensor 1)",
        "P0036" => "HO2S heater control circuit (Bank 1 Sensor 2)",

        // Fuel / air metering
        "P0100" => "Mass air flow circuit malfunction",
        "P0101" => "Mass air flow circuit range/performance",
        "P0102" => "Mass air flow circuit low input",
        "P0110" => "Intake air temperature circuit malfunction",
        "P0111" => "Intake air temperature circuit range/performance",
        "P0112" => "Intake air temperature circuit low input",
        "P0113" => "Intake air temperature circuit high input",
        "P0120" => "Throttle position sensor circuit malfunction",
        "P0130" => "O2 sensor circuit malfunction (Bank 1 Sensor 1)",
        "P0131" => "O2 sensor circuit low voltage (Bank 1 Sensor 1)",
        "P0132" => "O2 sensor circuit high voltage (Bank 1 Sensor 1)",
        "P0133" => "O2 sensor circuit slow response (Bank 1 Sensor 1)",
        "P0134" => "O2 sensor circuit no activity detected (Bank 1 Sensor 1)",
        "P0135" => "O2 sensor heater circuit malfunction (Bank 1 Sensor 1)",
        "P0136" => "O2 sensor circuit malfunction (Bank 1 Sensor 2)",
        "P0137" => "O2 sensor circuit low voltage (Bank 1 Sensor 2)",
        "P0138" => "O2 sensor circuit high voltage (Bank 1 Sensor 2)",
        "P0171" => "System too lean (Bank 1)",
        "P0172" => "System too rich (Bank 1)",
        "P0174" => "System too lean (Bank 2)",
        "P0175" => "System too rich (Bank 2)",

        // Turbo / boost
        "P0234" => "Turbocharger overboost condition",
        "P0236" => "Turbocharger boost sensor A circuit range/performance",
        "P0299" => "Turbocharger underboost condition",

        // Ignition / misfire
        "P0300" => "Random/multiple cylinder misfire detected",
        "P0301" => "Cylinder 1 misfire detected",
        "P0302" => "Cylinder 2 misfire detected",
        "P0303" => "Cylinder 3 misfire detected",
        "P0304" => "Cylinder 4 misfire detected",

        // Camshaft position sensors
        "P0365" => "Camshaft position sensor B circuit (Bank 1)",
        "P0366" => "Camshaft position sensor B circuit range/performance (Bank 1)",

        // Emission controls
        "P0401" => "EGR flow insufficient detected",
        "P0420" => "Catalyst system efficiency below threshold (Bank 1)",
        "P0430" => "Catalyst system efficiency below threshold (Bank 2)",
        "P0440" => "Evaporative emission system malfunction",
        "P0442" => "Evaporative emission system leak detected (small)",
        "P0449" => "Evaporative emission system vent valve/solenoid circuit",
        "P0451" => "Evaporative emission system pressure sensor range/performance",
        "P0455" => "Evaporative emission system leak detected (large)",
        "P0496" => "Evaporative emission system high purge flow",

        // Idle / speed control
        "P0500" => "Vehicle speed sensor malfunction",
        "P0505" => "Idle air control system malfunction",
        "P0506" => "Idle control system RPM lower than expected",
        "P0507" => "Idle control system RPM higher than expected",

        // Thermostat
        "P0597" => "Thermostat heater control circuit open",
        "P0598" => "Thermostat heater control circuit low",
        "P0599" => "Thermostat heater control circuit high",

        // Transmission
        "P0700" => "Transmission control system malfunction",
        "P0711" => "Transmission fluid temperature sensor circuit range/performance",
        "P0715" => "Input/turbine speed sensor circuit malfunction",
        "P0717" => "Input/turbine speed sensor circuit no signal",
        "P0720" => "Output speed sensor circuit malfunction",
        "P0741" => "Torque converter clutch circuit performance or stuck off",
        "P0747" => "Pressure control solenoid A stuck on",
        "P0751" => "Shift solenoid A performance or stuck off",
        "P0756" => "Shift solenoid B performance or stuck off",

        // GM-specific / manufacturer
        "P1101" => "Intake airflow system performance",
        "P2097" => "Post catalyst fuel trim system too rich (Bank 1)",
        "P2227" => "Barometric pressure circuit range/performance",
        "P2270" => "O2 sensor signal stuck lean (Bank 1 Sensor 2)",
        "P2271" => "O2 sensor signal stuck rich (Bank 1 Sensor 2)",
        "P2797" => "Auxiliary transmission fluid pump performance",

        // Body (B) codes
        "B0083" => "Left side/front impact sensor circuit",
        "B0092" => "Left side/rear impact sensor circuit",
        "B0096" => "Right side/rear impact sensor circuit",
        "B0408" => "Temperature control A circuit",
        "B1325" => "Control module general memory failure",
        "B1517" => "Steering wheel controls switch 1 circuit",

        // Chassis (C) codes
        "C0035" => "Left front wheel speed sensor circuit",
        "C0040" => "Right front wheel speed sensor circuit",
        "C0045" => "Left rear wheel speed sensor circuit",
        "C0050" => "Right rear wheel speed sensor circuit",
        "C0110" => "Pump motor circuit malfunction",
        "C0161" => "ABS/TCS brake switch circuit malfunction",
        "C0186" => "Lateral accelerometer circuit",
        "C0196" => "Yaw rate sensor circuit",
        "C0550" => "ECU malfunction (stability system)",
        "C0899" => "Device voltage low",
        "C0900" => "Device voltage high",

        // Network (U) codes
        "U0001" => "High speed CAN communication bus",
        "U0073" => "Control module communication bus A off",
        "U0100" => "Lost communication with ECM/PCM A",
        "U0101" => "Lost communication with TCM",
        "U0121" => "Lost communication with ABS control module",
        "U0140" => "Lost communication with body control module",
        "U0146" => "Lost communication with gateway A",
        "U0151" => "Lost communication with restraints control module",
        "U0155" => "Lost communication with instrument panel cluster",
        "U0168" => "Lost communication with HVAC control module",
        "U0184" => "Lost communication with radio",
        "U0401" => "Invalid data received from ECM/PCM A",

        _ => "Unknown DTC",
    }
}

/// Return the DTC list for a given mock scenario index.
pub fn scenario_dtcs(scenario: u8) -> Vec<Dtc> {
    match scenario {
        0 => vec![],
        1 => vec![Dtc::from_code("P0420"), Dtc::from_code("P0171")],
        2 => vec![
            Dtc::from_code("P0300"),
            Dtc::from_code("P0171"),
            Dtc::from_code("P0420"),
            Dtc::from_code("P0505"),
            Dtc::from_code("P0700"),
        ],
        _ => vec![],
    }
}

/// Number of available DTC scenarios (for modular cycling).
pub const DTC_SCENARIO_COUNT: u8 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_bytes_p0420() {
        // P0420: P=00, 0=00, 4=0100, 2=0010, 0=0000
        // Byte A: 00_00_0100 = 0x04
        // Byte B: 0010_0000 = 0x20
        let dtc = Dtc::from_bytes(0x04, 0x20);
        assert_eq!(dtc.code, "P0420");
        assert_eq!(dtc.category, DtcCategory::Powertrain);
        assert!(dtc.description.contains("Catalyst"));
    }

    #[test]
    fn test_from_bytes_p0171() {
        // P0171: P=00, 0=00, 1=0001, 7=0111, 1=0001
        // Byte A: 00_00_0001 = 0x01
        // Byte B: 0111_0001 = 0x71
        let dtc = Dtc::from_bytes(0x01, 0x71);
        assert_eq!(dtc.code, "P0171");
        assert_eq!(dtc.category, DtcCategory::Powertrain);
        assert!(dtc.description.contains("lean"));
    }

    #[test]
    fn test_from_bytes_chassis_code() {
        // C0100: C=01, 0=00, 1=0001, 0=0000, 0=0000
        // Byte A: 01_00_0001 = 0x41
        // Byte B: 0000_0000 = 0x00
        let dtc = Dtc::from_bytes(0x41, 0x00);
        assert_eq!(dtc.code, "C0100");
        assert_eq!(dtc.category, DtcCategory::Chassis);
    }

    #[test]
    fn test_from_code() {
        let dtc = Dtc::from_code("P0300");
        assert_eq!(dtc.code, "P0300");
        assert_eq!(dtc.category, DtcCategory::Powertrain);
        assert!(dtc.description.contains("misfire"));
    }

    #[test]
    fn test_scenario_0_empty() {
        assert!(scenario_dtcs(0).is_empty());
    }

    #[test]
    fn test_scenario_1_minor() {
        let dtcs = scenario_dtcs(1);
        assert_eq!(dtcs.len(), 2);
        assert_eq!(dtcs[0].code, "P0420");
        assert_eq!(dtcs[1].code, "P0171");
    }

    #[test]
    fn test_scenario_2_multiple() {
        let dtcs = scenario_dtcs(2);
        assert_eq!(dtcs.len(), 5);
    }
}
