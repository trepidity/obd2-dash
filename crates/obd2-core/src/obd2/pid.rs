use super::types::{Obd2Error, PidReading};

/// OBD-II Mode 01 PIDs we support.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pid {
    EngineLoad,           // 0x04
    CoolantTemp,          // 0x05
    ShortFuelTrimBank1,   // 0x06
    LongFuelTrimBank1,    // 0x07
    ShortFuelTrimBank2,   // 0x08
    LongFuelTrimBank2,    // 0x09
    FuelPressure,         // 0x0A
    IntakeMap,            // 0x0B
    EngineRpm,            // 0x0C
    VehicleSpeed,         // 0x0D
    IntakeAirTemp,        // 0x0F
    Maf,                  // 0x10
    ThrottlePosition,     // 0x11
    FuelTankLevel,        // 0x2F
    BarometricPressure,   // 0x33
    CatalystTempB1S1,     // 0x3C
    CatalystTempB2S1,     // 0x3D
    CatalystTempB1S2,     // 0x3E
    CatalystTempB2S2,     // 0x3F
    ControlModuleVoltage, // 0x42
    AmbientAirTemp,       // 0x46
    EngineOilTemp,        // 0x5C
    EngineFuelRate,       // 0x5E
    // Extended / manufacturer-specific (not standard Mode 01)
    TransmissionTemp,     // 0xFE (custom)
    OilPressure,          // 0xFD (custom)
}

impl Pid {
    /// The PID byte code (Mode 01).
    pub fn code(&self) -> u8 {
        match self {
            Pid::EngineLoad => 0x04,
            Pid::CoolantTemp => 0x05,
            Pid::ShortFuelTrimBank1 => 0x06,
            Pid::LongFuelTrimBank1 => 0x07,
            Pid::ShortFuelTrimBank2 => 0x08,
            Pid::LongFuelTrimBank2 => 0x09,
            Pid::FuelPressure => 0x0A,
            Pid::IntakeMap => 0x0B,
            Pid::EngineRpm => 0x0C,
            Pid::VehicleSpeed => 0x0D,
            Pid::IntakeAirTemp => 0x0F,
            Pid::Maf => 0x10,
            Pid::ThrottlePosition => 0x11,
            Pid::FuelTankLevel => 0x2F,
            Pid::BarometricPressure => 0x33,
            Pid::CatalystTempB1S1 => 0x3C,
            Pid::CatalystTempB2S1 => 0x3D,
            Pid::CatalystTempB1S2 => 0x3E,
            Pid::CatalystTempB2S2 => 0x3F,
            Pid::ControlModuleVoltage => 0x42,
            Pid::AmbientAirTemp => 0x46,
            Pid::EngineOilTemp => 0x5C,
            Pid::EngineFuelRate => 0x5E,
            Pid::TransmissionTemp => 0xFE,
            Pid::OilPressure => 0xFD,
        }
    }

    /// Human-readable name for this PID.
    pub fn name(&self) -> &'static str {
        match self {
            Pid::EngineLoad => "Engine Load",
            Pid::CoolantTemp => "Coolant Temp",
            Pid::ShortFuelTrimBank1 => "Short Fuel Trim B1",
            Pid::LongFuelTrimBank1 => "Long Fuel Trim B1",
            Pid::ShortFuelTrimBank2 => "Short Fuel Trim B2",
            Pid::LongFuelTrimBank2 => "Long Fuel Trim B2",
            Pid::FuelPressure => "Fuel Pressure",
            Pid::IntakeMap => "Intake MAP",
            Pid::EngineRpm => "Engine RPM",
            Pid::VehicleSpeed => "Vehicle Speed",
            Pid::IntakeAirTemp => "Intake Air Temp",
            Pid::Maf => "MAF",
            Pid::ThrottlePosition => "Throttle Position",
            Pid::FuelTankLevel => "Fuel Tank Level",
            Pid::BarometricPressure => "Barometric Pressure",
            Pid::CatalystTempB1S1 => "Catalyst Temp B1S1",
            Pid::CatalystTempB2S1 => "Catalyst Temp B2S1",
            Pid::CatalystTempB1S2 => "Catalyst Temp B1S2",
            Pid::CatalystTempB2S2 => "Catalyst Temp B2S2",
            Pid::ControlModuleVoltage => "Control Module Voltage",
            Pid::AmbientAirTemp => "Ambient Air Temp",
            Pid::EngineOilTemp => "Engine Oil Temp",
            Pid::EngineFuelRate => "Engine Fuel Rate",
            Pid::TransmissionTemp => "Transmission Temp",
            Pid::OilPressure => "Oil Pressure",
        }
    }

    /// Look up a Pid by its hex code.
    pub fn from_code(code: u8) -> Option<Pid> {
        match code {
            0x04 => Some(Pid::EngineLoad),
            0x05 => Some(Pid::CoolantTemp),
            0x06 => Some(Pid::ShortFuelTrimBank1),
            0x07 => Some(Pid::LongFuelTrimBank1),
            0x08 => Some(Pid::ShortFuelTrimBank2),
            0x09 => Some(Pid::LongFuelTrimBank2),
            0x0A => Some(Pid::FuelPressure),
            0x0B => Some(Pid::IntakeMap),
            0x0C => Some(Pid::EngineRpm),
            0x0D => Some(Pid::VehicleSpeed),
            0x0F => Some(Pid::IntakeAirTemp),
            0x10 => Some(Pid::Maf),
            0x11 => Some(Pid::ThrottlePosition),
            0x2F => Some(Pid::FuelTankLevel),
            0x33 => Some(Pid::BarometricPressure),
            0x3C => Some(Pid::CatalystTempB1S1),
            0x3D => Some(Pid::CatalystTempB2S1),
            0x3E => Some(Pid::CatalystTempB1S2),
            0x3F => Some(Pid::CatalystTempB2S2),
            0x42 => Some(Pid::ControlModuleVoltage),
            0x46 => Some(Pid::AmbientAirTemp),
            0x5C => Some(Pid::EngineOilTemp),
            0x5E => Some(Pid::EngineFuelRate),
            0xFD => Some(Pid::OilPressure),
            0xFE => Some(Pid::TransmissionTemp),
            _ => None,
        }
    }

    /// The AT command string to send (e.g. "010C" for RPM).
    pub fn command(&self) -> String {
        format!("01{:02X}", self.code())
    }

    /// The expected response service byte (Mode 01 + 0x40 = 0x41).
    pub fn response_id(&self) -> u8 {
        0x41
    }

    /// Human-readable unit string.
    pub fn unit(&self) -> &'static str {
        match self {
            Pid::EngineLoad => "%",
            Pid::CoolantTemp => "°C",
            Pid::ShortFuelTrimBank1
            | Pid::LongFuelTrimBank1
            | Pid::ShortFuelTrimBank2
            | Pid::LongFuelTrimBank2 => "%",
            Pid::FuelPressure => "kPa",
            Pid::IntakeMap => "kPa",
            Pid::EngineRpm => "rpm",
            Pid::VehicleSpeed => "km/h",
            Pid::IntakeAirTemp => "°C",
            Pid::Maf => "g/s",
            Pid::ThrottlePosition => "%",
            Pid::FuelTankLevel => "%",
            Pid::BarometricPressure => "kPa",
            Pid::CatalystTempB1S1
            | Pid::CatalystTempB2S1
            | Pid::CatalystTempB1S2
            | Pid::CatalystTempB2S2 => "°C",
            Pid::ControlModuleVoltage => "V",
            Pid::AmbientAirTemp => "°C",
            Pid::EngineOilTemp => "°C",
            Pid::EngineFuelRate => "L/h",
            Pid::TransmissionTemp => "°C",
            Pid::OilPressure => "kPa",
        }
    }

    /// Parse raw data bytes into a PidReading.
    /// `data` should contain only the data bytes (after service ID and PID byte).
    pub fn parse_value(&self, data: &[u8]) -> Result<PidReading, Obd2Error> {
        match self {
            Pid::EngineLoad | Pid::ThrottlePosition | Pid::FuelTankLevel => {
                // A * 100 / 255
                if data.is_empty() {
                    return Err(Obd2Error::ParseError(format!(
                        "{}: need 1 byte",
                        self.name()
                    )));
                }
                let value = data[0] as f64 * 100.0 / 255.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::CoolantTemp | Pid::IntakeAirTemp | Pid::AmbientAirTemp => {
                // A - 40
                if data.is_empty() {
                    return Err(Obd2Error::ParseError(format!(
                        "{}: need 1 byte",
                        self.name()
                    )));
                }
                let value = data[0] as f64 - 40.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::ShortFuelTrimBank1
            | Pid::LongFuelTrimBank1
            | Pid::ShortFuelTrimBank2
            | Pid::LongFuelTrimBank2 => {
                // (A - 128) * 100 / 128
                if data.is_empty() {
                    return Err(Obd2Error::ParseError(format!(
                        "{}: need 1 byte",
                        self.name()
                    )));
                }
                let value = (data[0] as f64 - 128.0) * 100.0 / 128.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::FuelPressure => {
                // A * 3
                if data.is_empty() {
                    return Err(Obd2Error::ParseError("Fuel Pressure: need 1 byte".into()));
                }
                let value = data[0] as f64 * 3.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::IntakeMap | Pid::BarometricPressure => {
                // A (direct kPa)
                if data.is_empty() {
                    return Err(Obd2Error::ParseError(format!(
                        "{}: need 1 byte",
                        self.name()
                    )));
                }
                let value = data[0] as f64;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::EngineRpm => {
                // (256A + B) / 4
                if data.len() < 2 {
                    return Err(Obd2Error::ParseError("RPM: need 2 bytes".into()));
                }
                let value = (256.0 * data[0] as f64 + data[1] as f64) / 4.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::VehicleSpeed => {
                // A (direct km/h)
                if data.is_empty() {
                    return Err(Obd2Error::ParseError("speed: need 1 byte".into()));
                }
                let value = data[0] as f64;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::Maf => {
                // (256A + B) / 100
                if data.len() < 2 {
                    return Err(Obd2Error::ParseError("MAF: need 2 bytes".into()));
                }
                let value = (256.0 * data[0] as f64 + data[1] as f64) / 100.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::CatalystTempB1S1
            | Pid::CatalystTempB2S1
            | Pid::CatalystTempB1S2
            | Pid::CatalystTempB2S2 => {
                // (256A + B) / 10 - 40
                if data.len() < 2 {
                    return Err(Obd2Error::ParseError(format!(
                        "{}: need 2 bytes",
                        self.name()
                    )));
                }
                let value = (256.0 * data[0] as f64 + data[1] as f64) / 10.0 - 40.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::ControlModuleVoltage => {
                // (256A + B) / 1000
                if data.len() < 2 {
                    return Err(Obd2Error::ParseError(
                        "Control Module Voltage: need 2 bytes".into(),
                    ));
                }
                let value = (256.0 * data[0] as f64 + data[1] as f64) / 1000.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::EngineOilTemp => {
                // A - 40
                if data.is_empty() {
                    return Err(Obd2Error::ParseError("Engine Oil Temp: need 1 byte".into()));
                }
                let value = data[0] as f64 - 40.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::EngineFuelRate => {
                // (256A + B) / 20
                if data.len() < 2 {
                    return Err(Obd2Error::ParseError(
                        "Engine Fuel Rate: need 2 bytes".into(),
                    ));
                }
                let value = (256.0 * data[0] as f64 + data[1] as f64) / 20.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::TransmissionTemp => {
                // A - 40 (same formula as coolant/oil temp)
                if data.is_empty() {
                    return Err(Obd2Error::ParseError(
                        "Transmission Temp: need 1 byte".into(),
                    ));
                }
                let value = data[0] as f64 - 40.0;
                Ok(PidReading::new(value, self.unit()))
            }
            Pid::OilPressure => {
                // (256A + B) / 4  (kPa)
                if data.len() < 2 {
                    return Err(Obd2Error::ParseError(
                        "Oil Pressure: need 2 bytes".into(),
                    ));
                }
                let value = (256.0 * data[0] as f64 + data[1] as f64) / 4.0;
                Ok(PidReading::new(value, self.unit()))
            }
        }
    }

    /// All actively polled PIDs.
    pub fn all() -> &'static [Pid] {
        Self::all_known()
    }

    /// All known PIDs (for threshold resolution, database seeding, etc.).
    pub fn all_known() -> &'static [Pid] {
        &[
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::ShortFuelTrimBank2,
            Pid::LongFuelTrimBank2,
            Pid::FuelPressure,
            Pid::IntakeMap,
            Pid::EngineRpm,
            Pid::VehicleSpeed,
            Pid::IntakeAirTemp,
            Pid::Maf,
            Pid::ThrottlePosition,
            Pid::FuelTankLevel,
            Pid::BarometricPressure,
            Pid::CatalystTempB1S1,
            Pid::CatalystTempB2S1,
            Pid::CatalystTempB1S2,
            Pid::CatalystTempB2S2,
            Pid::ControlModuleVoltage,
            Pid::AmbientAirTemp,
            Pid::EngineOilTemp,
            Pid::EngineFuelRate,
            Pid::TransmissionTemp,
            Pid::OilPressure,
        ]
    }

    /// All known PID codes as u8 values.
    pub fn all_known_codes() -> Vec<u8> {
        Self::all_known().iter().map(|p| p.code()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_codes() {
        assert_eq!(Pid::EngineLoad.code(), 0x04);
        assert_eq!(Pid::CoolantTemp.code(), 0x05);
        assert_eq!(Pid::EngineRpm.code(), 0x0C);
        assert_eq!(Pid::VehicleSpeed.code(), 0x0D);
    }

    #[test]
    fn test_pid_commands() {
        assert_eq!(Pid::EngineLoad.command(), "0104");
        assert_eq!(Pid::CoolantTemp.command(), "0105");
        assert_eq!(Pid::EngineRpm.command(), "010C");
        assert_eq!(Pid::VehicleSpeed.command(), "010D");
    }

    #[test]
    fn test_from_code() {
        assert_eq!(Pid::from_code(0x04), Some(Pid::EngineLoad));
        assert_eq!(Pid::from_code(0x0C), Some(Pid::EngineRpm));
        assert_eq!(Pid::from_code(0x5E), Some(Pid::EngineFuelRate));
        assert_eq!(Pid::from_code(0xFF), None);
    }

    #[test]
    fn test_name() {
        assert_eq!(Pid::EngineRpm.name(), "Engine RPM");
        assert_eq!(Pid::CoolantTemp.name(), "Coolant Temp");
        assert_eq!(Pid::ControlModuleVoltage.name(), "Control Module Voltage");
    }

    #[test]
    fn test_parse_engine_load() {
        let reading = Pid::EngineLoad.parse_value(&[0x00]).unwrap();
        assert!((reading.value - 0.0).abs() < 0.01);
        assert_eq!(reading.unit, "%");

        let reading = Pid::EngineLoad.parse_value(&[0xFF]).unwrap();
        assert!((reading.value - 100.0).abs() < 0.01);

        let reading = Pid::EngineLoad.parse_value(&[0x80]).unwrap();
        assert!((reading.value - 50.196).abs() < 0.01);
    }

    #[test]
    fn test_parse_coolant_temp() {
        let reading = Pid::CoolantTemp.parse_value(&[0x00]).unwrap();
        assert!((reading.value - (-40.0)).abs() < 0.01);
        assert_eq!(reading.unit, "°C");

        let reading = Pid::CoolantTemp.parse_value(&[40]).unwrap();
        assert!((reading.value - 0.0).abs() < 0.01);

        let reading = Pid::CoolantTemp.parse_value(&[130]).unwrap();
        assert!((reading.value - 90.0).abs() < 0.01);

        let reading = Pid::CoolantTemp.parse_value(&[0xFF]).unwrap();
        assert!((reading.value - 215.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_engine_rpm() {
        let reading = Pid::EngineRpm.parse_value(&[0x00, 0x00]).unwrap();
        assert!((reading.value - 0.0).abs() < 0.01);
        assert_eq!(reading.unit, "rpm");

        let reading = Pid::EngineRpm.parse_value(&[0x0C, 0x80]).unwrap();
        assert!((reading.value - 800.0).abs() < 0.01);

        let reading = Pid::EngineRpm.parse_value(&[0x32, 0x00]).unwrap();
        assert!((reading.value - 3200.0).abs() < 0.01);

        let reading = Pid::EngineRpm.parse_value(&[0xFF, 0xFF]).unwrap();
        assert!((reading.value - 16383.75).abs() < 0.01);
    }

    #[test]
    fn test_parse_vehicle_speed() {
        let reading = Pid::VehicleSpeed.parse_value(&[0x00]).unwrap();
        assert!((reading.value - 0.0).abs() < 0.01);
        assert_eq!(reading.unit, "km/h");

        let reading = Pid::VehicleSpeed.parse_value(&[72]).unwrap();
        assert!((reading.value - 72.0).abs() < 0.01);

        let reading = Pid::VehicleSpeed.parse_value(&[0xFF]).unwrap();
        assert!((reading.value - 255.0).abs() < 0.01);
    }

    #[test]
    fn test_parse_fuel_trim() {
        // 0% trim (centered)
        let reading = Pid::ShortFuelTrimBank1.parse_value(&[128]).unwrap();
        assert!((reading.value - 0.0).abs() < 0.01);

        // Max positive trim
        let reading = Pid::ShortFuelTrimBank1.parse_value(&[0xFF]).unwrap();
        assert!((reading.value - 99.2).abs() < 0.2);

        // Max negative trim
        let reading = Pid::ShortFuelTrimBank1.parse_value(&[0x00]).unwrap();
        assert!((reading.value - (-100.0)).abs() < 0.01);
    }

    #[test]
    fn test_parse_maf() {
        // 0 g/s
        let reading = Pid::Maf.parse_value(&[0x00, 0x00]).unwrap();
        assert!((reading.value - 0.0).abs() < 0.01);

        // 655.35 g/s max
        let reading = Pid::Maf.parse_value(&[0xFF, 0xFF]).unwrap();
        assert!((reading.value - 655.35).abs() < 0.01);
    }

    #[test]
    fn test_parse_control_module_voltage() {
        // ~12.5V
        let reading = Pid::ControlModuleVoltage.parse_value(&[0x30, 0xD4]).unwrap();
        // (256*0x30 + 0xD4) / 1000 = (12288 + 212) / 1000 = 12.5
        assert!((reading.value - 12.5).abs() < 0.01);
    }

    #[test]
    fn test_parse_errors() {
        assert!(Pid::EngineLoad.parse_value(&[]).is_err());
        assert!(Pid::CoolantTemp.parse_value(&[]).is_err());
        assert!(Pid::EngineRpm.parse_value(&[]).is_err());
        assert!(Pid::EngineRpm.parse_value(&[0x00]).is_err());
        assert!(Pid::VehicleSpeed.parse_value(&[]).is_err());
        assert!(Pid::Maf.parse_value(&[0x00]).is_err());
        assert!(Pid::ControlModuleVoltage.parse_value(&[]).is_err());
    }

    #[test]
    fn test_all_vs_all_known() {
        // all() now delegates to all_known()
        assert_eq!(Pid::all().len(), 25);
        assert_eq!(Pid::all_known().len(), 25);
        for pid in Pid::all() {
            assert!(Pid::all_known().contains(pid));
        }
    }
}
