/// Parameters for vehicle-specific mock simulation.
///
/// Currently only `vin` is used by obd2-core's MockAdapter.
/// Other fields (RPM, temps, voltage) are reserved for a future
/// rich-simulation adapter that supports warmup cycles and dynamic values.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct MockVehicleProfile {
    pub name: String,
    pub vin: String,
    pub idle_rpm_warm: f64,
    pub idle_rpm_cold: f64,
    pub max_rpm: f64,
    pub rpm_responsiveness: f64,
    pub speed_per_rpm: f64,
    pub warmup_rate: f64,
    pub normal_coolant_temp: f64,
    pub voltage: f64,
}

impl MockVehicleProfile {
    pub fn generic() -> Self {
        Self {
            name: "Generic Vehicle".to_string(),
            vin: "00000000000000000".to_string(),
            idle_rpm_warm: 800.0,
            idle_rpm_cold: 1000.0,
            max_rpm: 6500.0,
            rpm_responsiveness: 0.10,
            speed_per_rpm: 1.0 / 40.0,
            warmup_rate: 0.3,
            normal_coolant_temp: 90.0,
            voltage: 12.6,
        }
    }

    pub fn mini_2006() -> Self {
        Self {
            name: "2006 MINI Cooper S".to_string(),
            vin: "WMWRE33546T000001".to_string(),
            idle_rpm_warm: 750.0,
            idle_rpm_cold: 1100.0,
            max_rpm: 6800.0,
            rpm_responsiveness: 0.15,
            speed_per_rpm: 1.0 / 35.0,
            warmup_rate: 0.35,
            normal_coolant_temp: 92.0,
            voltage: 12.8,
        }
    }

    pub fn honda_2001() -> Self {
        Self {
            name: "2001 Honda Accord Coupe".to_string(),
            vin: "1HGCG32501A000001".to_string(),
            idle_rpm_warm: 750.0,
            idle_rpm_cold: 1200.0,
            max_rpm: 6100.0,
            rpm_responsiveness: 0.12,
            speed_per_rpm: 1.0 / 38.0,
            warmup_rate: 0.30,
            normal_coolant_temp: 90.0,
            voltage: 12.6,
        }
    }

    pub fn chevy_2004() -> Self {
        Self {
            name: "2004 Chevy 2500HD Duramax".to_string(),
            vin: "1GCHK23164F000001".to_string(),
            idle_rpm_warm: 650.0,
            idle_rpm_cold: 900.0,
            max_rpm: 3200.0,
            rpm_responsiveness: 0.06,
            speed_per_rpm: 1.0 / 15.0,
            warmup_rate: 0.25,
            normal_coolant_temp: 88.0,
            voltage: 12.4,
        }
    }
}
