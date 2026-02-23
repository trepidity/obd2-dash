use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use rand::Rng;

use super::dtc::{self, Dtc};
use super::pid::Pid;
use super::types::{AdapterCaps, AdapterInfo, Chipset, Obd2Error, PidReading};
use super::Obd2Connection;
use crate::mock_profile::MockVehicleProfile;

/// Mock OBD2 connection that generates realistic-looking data for testing and demo.
pub struct MockObd2 {
    profile: MockVehicleProfile,
    dtc_scenario: Arc<AtomicU8>,

    // Core state (original 4)
    rpm: f64,
    speed: f64,
    coolant_temp: f64,
    engine_load: f64,
    voltage: f64,

    // Engine / intake
    throttle_position: f64,
    intake_map: f64,
    maf: f64,
    fuel_pressure: f64,

    // Fuel trims
    short_fuel_trim_b1: f64,
    long_fuel_trim_b1: f64,
    short_fuel_trim_b2: f64,
    long_fuel_trim_b2: f64,

    // Temperatures
    intake_air_temp: f64,
    ambient_air_temp: f64,
    engine_oil_temp: f64,
    catalyst_temp_b1s1: f64,
    catalyst_temp_b2s1: f64,
    catalyst_temp_b1s2: f64,
    catalyst_temp_b2s2: f64,

    // Extended
    transmission_temp: f64,
    oil_pressure: f64,

    // Fuel / system
    fuel_tank_level: f64,
    barometric_pressure: f64,
    control_module_voltage: f64,
    engine_fuel_rate: f64,

    // Timing
    cycle: u64,
    last_tick: Instant,
}

impl MockObd2 {
    /// Create a mock with the generic (backward-compatible) profile.
    pub fn new() -> Self {
        Self::with_profile(MockVehicleProfile::generic(), Arc::new(AtomicU8::new(0)))
    }

    /// Create a mock with a specific vehicle profile and shared DTC scenario selector.
    pub fn with_profile(profile: MockVehicleProfile, dtc_scenario: Arc<AtomicU8>) -> Self {
        let voltage = profile.voltage;
        Self {
            dtc_scenario,
            rpm: profile.idle_rpm_cold,
            speed: 0.0,
            coolant_temp: 20.0,
            engine_load: 15.0,
            voltage,

            throttle_position: 8.0,
            intake_map: 30.0,
            maf: 2.0,
            fuel_pressure: 300.0,

            short_fuel_trim_b1: 0.0,
            long_fuel_trim_b1: 0.0,
            short_fuel_trim_b2: 0.0,
            long_fuel_trim_b2: 0.0,

            intake_air_temp: 25.0,
            ambient_air_temp: 22.0,
            engine_oil_temp: 20.0,
            catalyst_temp_b1s1: 25.0,
            catalyst_temp_b2s1: 25.0,
            catalyst_temp_b1s2: 25.0,
            catalyst_temp_b2s2: 25.0,

            transmission_temp: 20.0,
            oil_pressure: 100.0,

            fuel_tank_level: 75.0,
            barometric_pressure: 101.3,
            control_module_voltage: voltage,
            engine_fuel_rate: 1.0,

            cycle: 0,
            last_tick: Instant::now(),
            profile,
        }
    }

    /// Get the VIN associated with this mock profile.
    pub fn vin(&self) -> &str {
        &self.profile.vin
    }

    /// Build a mock AdapterInfo for demo/testing purposes.
    pub fn mock_adapter_info() -> AdapterInfo {
        AdapterInfo {
            chipset: Chipset::Elm327Clone,
            firmware_version: "ELM327 v1.5 (Mock)".to_string(),
            caps: AdapterCaps {
                can_clear_dtcs: false,
                dual_can: false,
                enhanced_diag: false,
                battery_voltage: true,
                adaptive_timing: false,
            },
        }
    }

    /// Advance the mock simulation one tick — produces a gentle sine-wave drive pattern.
    /// Uses time-based gating so the sim advances at consistent real-time rate
    /// regardless of how many PIDs are queried per cycle.
    fn tick(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_tick);

        // Only advance simulation at ~20Hz (50ms intervals) regardless of query rate
        if elapsed.as_millis() < 50 {
            return;
        }
        self.last_tick = now;

        self.cycle += 1;
        let t = self.cycle as f64 * 0.05;
        let mut rng = rand::thread_rng();

        let p = &self.profile;

        // RPM: oscillates between idle and ~60% of max
        let rpm_swing = (p.max_rpm - p.idle_rpm_warm) * 0.6;
        let target_rpm = p.idle_rpm_warm + rpm_swing * 0.5 * (1.0 + (t * 0.3).sin());
        self.rpm +=
            (target_rpm - self.rpm) * p.rpm_responsiveness + rng.gen_range(-20.0..20.0);
        self.rpm = self.rpm.clamp(0.0, p.max_rpm * 1.05);

        // Speed: follows RPM via vehicle-specific ratio
        let target_speed = (self.rpm - p.idle_rpm_warm).max(0.0) * p.speed_per_rpm;
        self.speed += (target_speed - self.speed) * 0.08 + rng.gen_range(-1.0..1.0);
        self.speed = self.speed.clamp(0.0, 255.0);

        // Coolant temp: warms up to vehicle-specific normal temp
        if self.coolant_temp < p.normal_coolant_temp {
            self.coolant_temp += p.warmup_rate;
        }
        self.coolant_temp += rng.gen_range(-0.2..0.2);
        self.coolant_temp = self.coolant_temp.clamp(-40.0, 215.0);

        // Engine load: proportional to RPM fraction of max
        self.engine_load =
            (self.rpm / p.max_rpm) * 100.0 + rng.gen_range(-2.0..2.0);
        self.engine_load = self.engine_load.clamp(0.0, 100.0);

        // Battery voltage: slight fluctuation
        self.voltage = p.voltage + rng.gen_range(-0.3..0.3);

        // --- New PIDs ---

        // Throttle: follows RPM demand (8% idle → 93% WOT)
        let rpm_frac = (self.rpm - p.idle_rpm_warm).max(0.0) / (p.max_rpm - p.idle_rpm_warm);
        let target_throttle = 8.0 + rpm_frac * 85.0;
        self.throttle_position += (target_throttle - self.throttle_position) * 0.15
            + rng.gen_range(-0.5..0.5);
        self.throttle_position = self.throttle_position.clamp(0.0, 100.0);

        // Intake MAP: ~25 kPa idle → ~105 kPa WOT, follows load
        let target_map = 25.0 + (self.engine_load / 100.0) * 80.0;
        self.intake_map += (target_map - self.intake_map) * 0.1 + rng.gen_range(-0.5..0.5);
        self.intake_map = self.intake_map.clamp(10.0, 255.0);

        // MAF: proportional to RPM * load
        let target_maf = (self.rpm / 1000.0) * (self.engine_load / 100.0) * 15.0;
        self.maf += (target_maf - self.maf) * 0.12 + rng.gen_range(-0.2..0.2);
        self.maf = self.maf.clamp(0.0, 655.35);

        // Fuel pressure: steady ~300 kPa with noise
        self.fuel_pressure = 300.0 + rng.gen_range(-5.0..5.0);
        self.fuel_pressure = self.fuel_pressure.clamp(0.0, 765.0);

        // Fuel trims: random walk around 0%, mean-reverting
        // Short-term: faster response
        self.short_fuel_trim_b1 += (0.0 - self.short_fuel_trim_b1) * 0.05
            + rng.gen_range(-0.8..0.8);
        self.short_fuel_trim_b1 = self.short_fuel_trim_b1.clamp(-25.0, 25.0);

        self.short_fuel_trim_b2 += (0.0 - self.short_fuel_trim_b2) * 0.05
            + rng.gen_range(-0.8..0.8);
        self.short_fuel_trim_b2 = self.short_fuel_trim_b2.clamp(-25.0, 25.0);

        // Long-term: slower response
        self.long_fuel_trim_b1 += (0.0 - self.long_fuel_trim_b1) * 0.01
            + rng.gen_range(-0.3..0.3);
        self.long_fuel_trim_b1 = self.long_fuel_trim_b1.clamp(-25.0, 25.0);

        self.long_fuel_trim_b2 += (0.0 - self.long_fuel_trim_b2) * 0.01
            + rng.gen_range(-0.3..0.3);
        self.long_fuel_trim_b2 = self.long_fuel_trim_b2.clamp(-25.0, 25.0);

        // Intake air temp: ambient + engine bay warming (up to ~15°C above ambient)
        let bay_heat = (self.coolant_temp - 20.0).max(0.0) * 0.08;
        self.intake_air_temp = self.ambient_air_temp + bay_heat + rng.gen_range(-0.3..0.3);
        self.intake_air_temp = self.intake_air_temp.clamp(-40.0, 215.0);

        // Ambient air temp: steady ~22°C with slow drift
        self.ambient_air_temp += rng.gen_range(-0.05..0.05);
        self.ambient_air_temp = self.ambient_air_temp.clamp(15.0, 35.0);

        // Oil temp: warms like coolant but slower
        let target_oil = p.normal_coolant_temp - 5.0;
        if self.engine_oil_temp < target_oil {
            self.engine_oil_temp += p.warmup_rate * 0.7;
        }
        self.engine_oil_temp += rng.gen_range(-0.2..0.2);
        self.engine_oil_temp = self.engine_oil_temp.clamp(-40.0, 215.0);

        // Catalyst temps: warm up slowly to 400-600°C range
        // S1 hotter than S2, B1 slightly hotter than B2
        let target_cat_b1s1 = 500.0 + (self.engine_load / 100.0) * 100.0;
        let target_cat_b2s1 = target_cat_b1s1 - 10.0;
        let target_cat_b1s2 = target_cat_b1s1 - 80.0;
        let target_cat_b2s2 = target_cat_b1s2 - 10.0;

        let cat_rate = 0.02;
        self.catalyst_temp_b1s1 += (target_cat_b1s1 - self.catalyst_temp_b1s1) * cat_rate
            + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b2s1 += (target_cat_b2s1 - self.catalyst_temp_b2s1) * cat_rate
            + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b1s2 += (target_cat_b1s2 - self.catalyst_temp_b1s2) * cat_rate
            + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b2s2 += (target_cat_b2s2 - self.catalyst_temp_b2s2) * cat_rate
            + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b1s1 = self.catalyst_temp_b1s1.clamp(-40.0, 6513.5);
        self.catalyst_temp_b2s1 = self.catalyst_temp_b2s1.clamp(-40.0, 6513.5);
        self.catalyst_temp_b1s2 = self.catalyst_temp_b1s2.clamp(-40.0, 6513.5);
        self.catalyst_temp_b2s2 = self.catalyst_temp_b2s2.clamp(-40.0, 6513.5);

        // Transmission temp: warms slower than coolant, stabilizes ~80-95°C
        let target_trans = p.normal_coolant_temp - 10.0;
        if self.transmission_temp < target_trans {
            self.transmission_temp += p.warmup_rate * 0.5;
        }
        // Extra heat at higher speeds (torque converter / gear friction)
        let speed_heat = (self.speed / 255.0) * 5.0;
        self.transmission_temp += speed_heat * 0.01 + rng.gen_range(-0.2..0.2);
        self.transmission_temp = self.transmission_temp.clamp(-40.0, 215.0);

        // Oil pressure: ~350 kPa warm, drops at low RPM/idle (~200 kPa)
        let rpm_frac_for_pressure = (self.rpm / p.max_rpm).clamp(0.0, 1.0);
        let target_oil_pressure = 200.0 + rpm_frac_for_pressure * 200.0;
        self.oil_pressure += (target_oil_pressure - self.oil_pressure) * 0.1
            + rng.gen_range(-3.0..3.0);
        self.oil_pressure = self.oil_pressure.clamp(0.0, 1000.0);

        // Fuel tank level: starts 75%, slowly decreases
        self.fuel_tank_level -= 0.001 + rng.gen_range(0.0..0.001);
        self.fuel_tank_level = self.fuel_tank_level.clamp(0.0, 100.0);

        // Barometric pressure: steady ~101.3 kPa
        self.barometric_pressure = 101.3 + rng.gen_range(-0.2..0.2);

        // Control module voltage: tracks battery ±0.1V
        self.control_module_voltage = self.voltage + rng.gen_range(-0.1..0.1);

        // Fuel rate: proportional to RPM * load
        self.engine_fuel_rate = (self.rpm / 1000.0) * (self.engine_load / 100.0) * 8.0
            + rng.gen_range(-0.2..0.2);
        self.engine_fuel_rate = self.engine_fuel_rate.clamp(0.0, 3276.75);
    }
}

impl Default for MockObd2 {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Obd2Connection for MockObd2 {
    async fn initialize(&mut self) -> Result<(), Obd2Error> {
        tracing::info!("Mock OBD2 initialized ({})", self.profile.name);
        Ok(())
    }

    async fn query_pid(&mut self, pid: Pid) -> Result<PidReading, Obd2Error> {
        self.tick();

        // Simulate ~15ms serial delay per query
        tokio::time::sleep(std::time::Duration::from_millis(15)).await;

        let (value, unit) = match pid {
            Pid::EngineRpm => (self.rpm, pid.unit()),
            Pid::VehicleSpeed => (self.speed, pid.unit()),
            Pid::CoolantTemp => (self.coolant_temp, pid.unit()),
            Pid::EngineLoad => (self.engine_load, pid.unit()),
            Pid::ThrottlePosition => (self.throttle_position, pid.unit()),
            Pid::IntakeMap => (self.intake_map, pid.unit()),
            Pid::Maf => (self.maf, pid.unit()),
            Pid::FuelPressure => (self.fuel_pressure, pid.unit()),
            Pid::ShortFuelTrimBank1 => (self.short_fuel_trim_b1, pid.unit()),
            Pid::LongFuelTrimBank1 => (self.long_fuel_trim_b1, pid.unit()),
            Pid::ShortFuelTrimBank2 => (self.short_fuel_trim_b2, pid.unit()),
            Pid::LongFuelTrimBank2 => (self.long_fuel_trim_b2, pid.unit()),
            Pid::IntakeAirTemp => (self.intake_air_temp, pid.unit()),
            Pid::AmbientAirTemp => (self.ambient_air_temp, pid.unit()),
            Pid::EngineOilTemp => (self.engine_oil_temp, pid.unit()),
            Pid::CatalystTempB1S1 => (self.catalyst_temp_b1s1, pid.unit()),
            Pid::CatalystTempB2S1 => (self.catalyst_temp_b2s1, pid.unit()),
            Pid::CatalystTempB1S2 => (self.catalyst_temp_b1s2, pid.unit()),
            Pid::CatalystTempB2S2 => (self.catalyst_temp_b2s2, pid.unit()),
            Pid::FuelTankLevel => (self.fuel_tank_level, pid.unit()),
            Pid::BarometricPressure => (self.barometric_pressure, pid.unit()),
            Pid::ControlModuleVoltage => (self.control_module_voltage, pid.unit()),
            Pid::EngineFuelRate => (self.engine_fuel_rate, pid.unit()),
            Pid::TransmissionTemp => (self.transmission_temp, pid.unit()),
            Pid::OilPressure => (self.oil_pressure, pid.unit()),
        };

        Ok(PidReading::new(value, unit))
    }

    async fn read_voltage(&mut self) -> Result<f64, Obd2Error> {
        Ok(self.voltage)
    }

    async fn read_dtcs(&mut self) -> Result<Vec<Dtc>, Obd2Error> {
        let scenario = self.dtc_scenario.load(Ordering::Relaxed);
        Ok(dtc::scenario_dtcs(scenario))
    }

    fn adapter_info(&self) -> Option<&AdapterInfo> {
        // Return a static-like mock adapter info
        None
    }
}
