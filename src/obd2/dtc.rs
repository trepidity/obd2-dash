/// Diagnostic Trouble Code (DTC) support — decoding, descriptions, and mock scenarios.

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
pub fn dtc_description(code: &str) -> &'static str {
    match code {
        // Fuel / air metering
        "P0100" => "Mass air flow circuit malfunction",
        "P0101" => "Mass air flow circuit range/performance",
        "P0102" => "Mass air flow circuit low input",
        "P0110" => "Intake air temperature circuit malfunction",
        "P0120" => "Throttle position sensor circuit malfunction",
        "P0130" => "O2 sensor circuit malfunction (Bank 1 Sensor 1)",
        "P0171" => "System too lean (Bank 1)",
        "P0172" => "System too rich (Bank 1)",
        "P0174" => "System too lean (Bank 2)",
        "P0175" => "System too rich (Bank 2)",

        // Ignition / misfire
        "P0300" => "Random/multiple cylinder misfire detected",
        "P0301" => "Cylinder 1 misfire detected",
        "P0302" => "Cylinder 2 misfire detected",
        "P0303" => "Cylinder 3 misfire detected",
        "P0304" => "Cylinder 4 misfire detected",

        // Emission controls
        "P0401" => "EGR flow insufficient detected",
        "P0420" => "Catalyst system efficiency below threshold (Bank 1)",
        "P0430" => "Catalyst system efficiency below threshold (Bank 2)",
        "P0440" => "Evaporative emission system malfunction",
        "P0442" => "Evaporative emission system leak detected (small)",
        "P0455" => "Evaporative emission system leak detected (large)",

        // Idle / speed control
        "P0500" => "Vehicle speed sensor malfunction",
        "P0505" => "Idle air control system malfunction",
        "P0507" => "Idle control system RPM higher than expected",

        // Transmission
        "P0700" => "Transmission control system malfunction",
        "P0715" => "Input/turbine speed sensor circuit malfunction",
        "P0720" => "Output speed sensor circuit malfunction",

        _ => "Unknown DTC",
    }
}

/// Return the DTC list for a given mock scenario index.
pub fn scenario_dtcs(scenario: u8) -> Vec<Dtc> {
    match scenario {
        0 => vec![],
        1 => vec![
            Dtc::from_code("P0420"),
            Dtc::from_code("P0171"),
        ],
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
