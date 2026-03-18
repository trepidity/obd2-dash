use std::collections::HashSet;
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

    // New PIDs
    timing_advance: f64,
    run_time: f64,
    distance_with_mil: f64,
    fuel_rail_gauge_pressure: f64,
    commanded_egr: f64,
    commanded_evap_purge: f64,
    distance_since_dtc_clear: f64,
    absolute_load: f64,
    commanded_equiv_ratio: f64,
    relative_throttle_pos: f64,
    abs_throttle_pos_b: f64,
    accel_pedal_pos_d: f64,
    accel_pedal_pos_e: f64,
    commanded_throttle_actuator: f64,
    fuel_rail_abs_pressure: f64,
    demanded_torque: f64,
    actual_torque: f64,
    reference_torque: f64,

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

            timing_advance: 10.0,
            run_time: 0.0,
            distance_with_mil: 0.0,
            fuel_rail_gauge_pressure: 35000.0,
            commanded_egr: 0.0,
            commanded_evap_purge: 0.0,
            distance_since_dtc_clear: 500.0,
            absolute_load: 15.0,
            commanded_equiv_ratio: 1.0,
            relative_throttle_pos: 0.0,
            abs_throttle_pos_b: 12.0,
            accel_pedal_pos_d: 0.0,
            accel_pedal_pos_e: 0.0,
            commanded_throttle_actuator: 8.0,
            fuel_rail_abs_pressure: 35000.0,
            demanded_torque: 0.0,
            actual_torque: 0.0,
            reference_torque: 250.0,

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
        self.rpm += (target_rpm - self.rpm) * p.rpm_responsiveness + rng.gen_range(-20.0..20.0);
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
        self.engine_load = (self.rpm / p.max_rpm) * 100.0 + rng.gen_range(-2.0..2.0);
        self.engine_load = self.engine_load.clamp(0.0, 100.0);

        // Battery voltage: slight fluctuation
        self.voltage = p.voltage + rng.gen_range(-0.3..0.3);

        // --- New PIDs ---

        // Throttle: follows RPM demand (8% idle → 93% WOT)
        let rpm_frac = (self.rpm - p.idle_rpm_warm).max(0.0) / (p.max_rpm - p.idle_rpm_warm);
        let target_throttle = 8.0 + rpm_frac * 85.0;
        self.throttle_position +=
            (target_throttle - self.throttle_position) * 0.15 + rng.gen_range(-0.5..0.5);
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
        self.short_fuel_trim_b1 +=
            (0.0 - self.short_fuel_trim_b1) * 0.05 + rng.gen_range(-0.8..0.8);
        self.short_fuel_trim_b1 = self.short_fuel_trim_b1.clamp(-25.0, 25.0);

        self.short_fuel_trim_b2 +=
            (0.0 - self.short_fuel_trim_b2) * 0.05 + rng.gen_range(-0.8..0.8);
        self.short_fuel_trim_b2 = self.short_fuel_trim_b2.clamp(-25.0, 25.0);

        // Long-term: slower response
        self.long_fuel_trim_b1 += (0.0 - self.long_fuel_trim_b1) * 0.01 + rng.gen_range(-0.3..0.3);
        self.long_fuel_trim_b1 = self.long_fuel_trim_b1.clamp(-25.0, 25.0);

        self.long_fuel_trim_b2 += (0.0 - self.long_fuel_trim_b2) * 0.01 + rng.gen_range(-0.3..0.3);
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
        self.catalyst_temp_b1s1 +=
            (target_cat_b1s1 - self.catalyst_temp_b1s1) * cat_rate + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b2s1 +=
            (target_cat_b2s1 - self.catalyst_temp_b2s1) * cat_rate + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b1s2 +=
            (target_cat_b1s2 - self.catalyst_temp_b1s2) * cat_rate + rng.gen_range(-1.0..1.0);
        self.catalyst_temp_b2s2 +=
            (target_cat_b2s2 - self.catalyst_temp_b2s2) * cat_rate + rng.gen_range(-1.0..1.0);
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
        self.oil_pressure +=
            (target_oil_pressure - self.oil_pressure) * 0.1 + rng.gen_range(-3.0..3.0);
        self.oil_pressure = self.oil_pressure.clamp(0.0, 1000.0);

        // Fuel tank level: starts 75%, slowly decreases
        self.fuel_tank_level -= 0.001 + rng.gen_range(0.0..0.001);
        self.fuel_tank_level = self.fuel_tank_level.clamp(0.0, 100.0);

        // Barometric pressure: steady ~101.3 kPa
        self.barometric_pressure = 101.3 + rng.gen_range(-0.2..0.2);

        // Control module voltage: tracks battery ±0.1V
        self.control_module_voltage = self.voltage + rng.gen_range(-0.1..0.1);

        // Fuel rate: proportional to RPM * load
        self.engine_fuel_rate =
            (self.rpm / 1000.0) * (self.engine_load / 100.0) * 8.0 + rng.gen_range(-0.2..0.2);
        self.engine_fuel_rate = self.engine_fuel_rate.clamp(0.0, 3276.75);

        // --- Additional PIDs ---

        // Timing advance: oscillates ~10-25° tracking RPM
        let rpm_frac_timing = (self.rpm - p.idle_rpm_warm).max(0.0) / (p.max_rpm - p.idle_rpm_warm);
        self.timing_advance = 10.0 + rpm_frac_timing * 15.0 + rng.gen_range(-0.5..0.5);
        self.timing_advance = self.timing_advance.clamp(-64.0, 63.5);

        // Run time: increments each tick (~50ms per tick)
        self.run_time += 0.05;

        // Distance with MIL: slowly incrementing (only when MIL is on, simulated)
        self.distance_with_mil += 0.001;

        // Fuel rail gauge/abs pressure: direct injection ~35000-40000 kPa
        let rail_base = 35000.0 + (self.engine_load / 100.0) * 5000.0;
        self.fuel_rail_gauge_pressure = rail_base + rng.gen_range(-200.0..200.0);
        self.fuel_rail_abs_pressure = self.fuel_rail_gauge_pressure + 101.0;

        // Commanded EGR: low percentage, increases with load
        self.commanded_egr = (self.engine_load / 100.0) * 15.0 + rng.gen_range(-0.5..0.5);
        self.commanded_egr = self.commanded_egr.clamp(0.0, 100.0);

        // Commanded EVAP purge: low percentage
        self.commanded_evap_purge = 5.0 + rng.gen_range(-1.0..1.0);
        self.commanded_evap_purge = self.commanded_evap_purge.clamp(0.0, 100.0);

        // Distance since DTC clear: slowly incrementing
        self.distance_since_dtc_clear += 0.002;

        // Absolute load: tracks engine load
        self.absolute_load = self.engine_load + rng.gen_range(-1.0..1.0);
        self.absolute_load = self.absolute_load.clamp(0.0, 25700.0);

        // Commanded equiv ratio: ~1.0 ± small drift
        self.commanded_equiv_ratio = 1.0 + rng.gen_range(-0.02..0.02);
        self.commanded_equiv_ratio = self.commanded_equiv_ratio.clamp(0.0, 2.0);

        // Relative throttle position
        self.relative_throttle_pos = self.throttle_position * 0.9 + rng.gen_range(-0.5..0.5);
        self.relative_throttle_pos = self.relative_throttle_pos.clamp(0.0, 100.0);

        // Abs throttle pos B: slightly offset from throttle
        self.abs_throttle_pos_b = self.throttle_position + 4.0 + rng.gen_range(-0.3..0.3);
        self.abs_throttle_pos_b = self.abs_throttle_pos_b.clamp(0.0, 100.0);

        // Accel pedal pos D: tracks throttle
        self.accel_pedal_pos_d = self.throttle_position * 0.95 + rng.gen_range(-0.3..0.3);
        self.accel_pedal_pos_d = self.accel_pedal_pos_d.clamp(0.0, 100.0);

        // Accel pedal pos E: similar to D
        self.accel_pedal_pos_e = self.accel_pedal_pos_d + rng.gen_range(-0.5..0.5);
        self.accel_pedal_pos_e = self.accel_pedal_pos_e.clamp(0.0, 100.0);

        // Commanded throttle actuator
        self.commanded_throttle_actuator = self.throttle_position + rng.gen_range(-0.5..0.5);
        self.commanded_throttle_actuator = self.commanded_throttle_actuator.clamp(0.0, 100.0);

        // Demanded torque: proportional to load
        self.demanded_torque = (self.engine_load / 100.0) * 80.0 - 10.0 + rng.gen_range(-1.0..1.0);
        self.demanded_torque = self.demanded_torque.clamp(-125.0, 130.0);

        // Actual torque: follows demanded with small lag
        self.actual_torque +=
            (self.demanded_torque - self.actual_torque) * 0.3 + rng.gen_range(-0.5..0.5);
        self.actual_torque = self.actual_torque.clamp(-125.0, 130.0);

        // Reference torque: constant for engine type
        // (stays at initial value — 250 Nm for 1.5L turbo)
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
            Pid::TimingAdvance => (self.timing_advance, pid.unit()),
            Pid::RunTimeSinceStart => (self.run_time, pid.unit()),
            Pid::DistanceWithMil => (self.distance_with_mil, pid.unit()),
            Pid::FuelRailGaugePressure => (self.fuel_rail_gauge_pressure, pid.unit()),
            Pid::CommandedEgr => (self.commanded_egr, pid.unit()),
            Pid::CommandedEvapPurge => (self.commanded_evap_purge, pid.unit()),
            Pid::DistanceSinceDtcClear => (self.distance_since_dtc_clear, pid.unit()),
            Pid::AbsoluteLoad => (self.absolute_load, pid.unit()),
            Pid::CommandedEquivRatio => (self.commanded_equiv_ratio, pid.unit()),
            Pid::RelativeThrottlePos => (self.relative_throttle_pos, pid.unit()),
            Pid::AbsThrottlePosB => (self.abs_throttle_pos_b, pid.unit()),
            Pid::AccelPedalPosD => (self.accel_pedal_pos_d, pid.unit()),
            Pid::AccelPedalPosE => (self.accel_pedal_pos_e, pid.unit()),
            Pid::CommandedThrottleActuator => (self.commanded_throttle_actuator, pid.unit()),
            Pid::FuelRailAbsPressure => (self.fuel_rail_abs_pressure, pid.unit()),
            Pid::DemandedTorque => (self.demanded_torque, pid.unit()),
            Pid::ActualTorque => (self.actual_torque, pid.unit()),
            Pid::ReferenceTorque => (self.reference_torque, pid.unit()),
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

    async fn read_vin(&mut self) -> Result<String, Obd2Error> {
        Ok(self.profile.vin.clone())
    }

    fn adapter_info(&self) -> Option<&AdapterInfo> {
        // Return a static-like mock adapter info
        None
    }

    fn supported_pids(&self) -> &HashSet<u8> {
        // Mock supports all PIDs — use a leaked static set so we can return a reference.
        // This is fine for mock/testing; the set is created once.
        static ALL_PIDS: std::sync::LazyLock<HashSet<u8>> = std::sync::LazyLock::new(|| {
            Pid::all_known_codes().into_iter().collect()
        });
        &ALL_PIDS
    }
}
