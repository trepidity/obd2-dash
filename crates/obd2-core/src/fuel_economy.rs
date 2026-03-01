use std::collections::VecDeque;

use crate::obd2::types::VehicleData;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Specific gas constant for dry air (kPa·m³ / (kg·K))
const R_AIR: f64 = 0.287;

/// Standard atmosphere (kPa)
const STD_BARO: f64 = 101.325;

/// Standard temperature for air density reference (K)
const STD_TEMP_K: f64 = 293.15;

/// Liters per US gallon
const LITERS_PER_GALLON: f64 = 3.78541;

/// km per mile
const KM_PER_MILE: f64 = 1.60934;

/// History capacity for sparkline
const MPG_HISTORY_CAP: usize = 120;

// ─── Fuel type ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FuelType {
    Gasoline,
    Diesel,
    E85,
}

impl FuelType {
    /// Stoichiometric air-fuel ratio (mass)
    pub fn afr(&self) -> f64 {
        match self {
            FuelType::Gasoline => 14.7,
            FuelType::Diesel => 14.5,
            FuelType::E85 => 9.765,
        }
    }

    /// Fuel density in kg/L
    pub fn density(&self) -> f64 {
        match self {
            FuelType::Gasoline => 0.745,
            FuelType::Diesel => 0.832,
            FuelType::E85 => 0.785,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FuelType::Gasoline => "Gasoline",
            FuelType::Diesel => "Diesel",
            FuelType::E85 => "E85",
        }
    }

    /// Detect from vehicle info fuel_type string.
    pub fn from_str_hint(s: &str) -> Self {
        let lower = s.to_lowercase();
        if lower.contains("diesel") {
            FuelType::Diesel
        } else if lower.contains("e85") || lower.contains("flex") {
            FuelType::E85
        } else {
            FuelType::Gasoline
        }
    }
}

// ─── Gold standard source ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GoldSource {
    DirectFuelRate,
    MafDerived,
    Unavailable,
}

impl GoldSource {
    pub fn label(&self) -> &'static str {
        match self {
            GoldSource::DirectFuelRate => "Direct (0x5E)",
            GoldSource::MafDerived => "MAF-Derived (0x10)",
            GoldSource::Unavailable => "Unavailable",
        }
    }
}

// ─── Results ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct GoldStandardResult {
    pub source: GoldSource,
    pub fuel_rate_lph: f64,
    pub instant_mpg: f64,
    pub avg_mpg: f64,
}

#[derive(Debug, Clone)]
pub struct CorrectionFactors {
    pub cold_engine: f64,
    pub altitude: f64,
    pub air_density: f64,
    pub fuel_trims: f64,
    pub catalyst_warmup: f64,
    pub throttle_transient: f64,
    pub high_load_wot: f64,
}

impl CorrectionFactors {
    pub fn total(&self) -> f64 {
        self.cold_engine
            * self.altitude
            * self.air_density
            * self.fuel_trims
            * self.catalyst_warmup
            * self.throttle_transient
            * self.high_load_wot
    }
}

impl Default for CorrectionFactors {
    fn default() -> Self {
        Self {
            cold_engine: 1.0,
            altitude: 1.0,
            air_density: 1.0,
            fuel_trims: 1.0,
            catalyst_warmup: 1.0,
            throttle_transient: 1.0,
            high_load_wot: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdvancedCalcResult {
    pub base_maf_gs: f64,
    pub base_fuel_rate_lph: f64,
    pub corrected_fuel_rate_lph: f64,
    pub corrections: CorrectionFactors,
    pub instant_mpg: f64,
    pub avg_mpg: f64,
}

// ─── Sensor snapshot ────────────────────────────────────────────────────────

/// Flat struct extracted from VehicleData to avoid borrow issues during calculation.
pub struct SensorSnapshot {
    pub rpm: Option<f64>,
    pub speed_kph: Option<f64>,
    pub coolant_temp_c: Option<f64>,
    pub engine_load_pct: Option<f64>,
    pub intake_map_kpa: Option<f64>,
    pub maf_gs: Option<f64>,
    pub intake_air_temp_c: Option<f64>,
    pub ambient_air_temp_c: Option<f64>,
    pub throttle_pct: Option<f64>,
    pub barometric_kpa: Option<f64>,
    pub engine_fuel_rate_lph: Option<f64>,
    pub stft_b1: Option<f64>,
    pub ltft_b1: Option<f64>,
    pub stft_b2: Option<f64>,
    pub ltft_b2: Option<f64>,
    pub catalyst_temp_b1s1: Option<f64>,
    pub catalyst_temp_b2s1: Option<f64>,
    pub catalyst_temp_b1s2: Option<f64>,
    pub catalyst_temp_b2s2: Option<f64>,
}

impl SensorSnapshot {
    pub fn from_vehicle(v: &VehicleData) -> Self {
        Self {
            rpm: v.rpm.as_ref().map(|r| r.value),
            speed_kph: v.speed.as_ref().map(|r| r.value),
            coolant_temp_c: v.coolant_temp.as_ref().map(|r| r.value),
            engine_load_pct: v.engine_load.as_ref().map(|r| r.value),
            intake_map_kpa: v.intake_map.as_ref().map(|r| r.value),
            maf_gs: v.maf.as_ref().map(|r| r.value),
            intake_air_temp_c: v.intake_air_temp.as_ref().map(|r| r.value),
            ambient_air_temp_c: v.ambient_air_temp.as_ref().map(|r| r.value),
            throttle_pct: v.throttle_position.as_ref().map(|r| r.value),
            barometric_kpa: v.barometric_pressure.as_ref().map(|r| r.value),
            engine_fuel_rate_lph: v.engine_fuel_rate.as_ref().map(|r| r.value),
            stft_b1: v.short_fuel_trim_b1.as_ref().map(|r| r.value),
            ltft_b1: v.long_fuel_trim_b1.as_ref().map(|r| r.value),
            stft_b2: v.short_fuel_trim_b2.as_ref().map(|r| r.value),
            ltft_b2: v.long_fuel_trim_b2.as_ref().map(|r| r.value),
            catalyst_temp_b1s1: v.catalyst_temp_b1s1.as_ref().map(|r| r.value),
            catalyst_temp_b2s1: v.catalyst_temp_b2s1.as_ref().map(|r| r.value),
            catalyst_temp_b1s2: v.catalyst_temp_b1s2.as_ref().map(|r| r.value),
            catalyst_temp_b2s2: v.catalyst_temp_b2s2.as_ref().map(|r| r.value),
        }
    }
}

// ─── Top-level fuel economy state ───────────────────────────────────────────

pub struct FuelEconomyState {
    // Configuration
    pub fuel_type: FuelType,
    pub displacement_l: f64,
    pub volumetric_efficiency: f64,

    // Results
    pub gold: Option<GoldStandardResult>,
    pub advanced: Option<AdvancedCalcResult>,

    // Sparkline history (instant MPG values scaled to u64)
    pub gold_mpg_history: VecDeque<u64>,
    pub advanced_mpg_history: VecDeque<u64>,

    // Trip accumulators
    gold_total_miles: f64,
    gold_total_gallons: f64,
    advanced_total_miles: f64,
    advanced_total_gallons: f64,

    // Previous throttle for transient detection
    prev_throttle: Option<f64>,
}

impl FuelEconomyState {
    pub fn new() -> Self {
        Self {
            fuel_type: FuelType::Gasoline,
            displacement_l: 2.0,
            volumetric_efficiency: 0.85,
            gold: None,
            advanced: None,
            gold_mpg_history: VecDeque::with_capacity(MPG_HISTORY_CAP),
            advanced_mpg_history: VecDeque::with_capacity(MPG_HISTORY_CAP),
            gold_total_miles: 0.0,
            gold_total_gallons: 0.0,
            advanced_total_miles: 0.0,
            advanced_total_gallons: 0.0,
            prev_throttle: None,
        }
    }

    /// Configure from vehicle info after loading.
    pub fn configure(&mut self, displacement_l: Option<f64>, fuel_type_hint: Option<&str>) {
        if let Some(d) = displacement_l {
            self.displacement_l = d;
        }
        if let Some(hint) = fuel_type_hint {
            self.fuel_type = FuelType::from_str_hint(hint);
        }
    }

    /// Recalculate both gold and advanced fuel economy from current sensor snapshot.
    /// `dt_secs` is the time since last calculation (for trip accumulators).
    pub fn recalculate(&mut self, snap: &SensorSnapshot, dt_secs: f64) {
        let speed_mph = snap.speed_kph.map(|s| s / KM_PER_MILE);

        // Accumulate distance traveled
        if let Some(mph) = speed_mph {
            if dt_secs > 0.0 && mph > 0.5 {
                let miles_this_tick = mph * (dt_secs / 3600.0);
                self.gold_total_miles += miles_this_tick;
                self.advanced_total_miles += miles_this_tick;
            }
        }

        // ── Gold Standard ───────────────────────────────────────────────
        self.gold = self.calc_gold(snap, speed_mph, dt_secs);

        // ── Advanced Calculated ─────────────────────────────────────────
        self.advanced = self.calc_advanced(snap, speed_mph, dt_secs);

        // Update throttle history for transient detection
        self.prev_throttle = snap.throttle_pct;

        // Push to sparkline histories
        if let Some(g) = &self.gold {
            push_history(&mut self.gold_mpg_history, g.instant_mpg);
        }
        if let Some(a) = &self.advanced {
            push_history(&mut self.advanced_mpg_history, a.instant_mpg);
        }
    }

    fn calc_gold(
        &mut self,
        snap: &SensorSnapshot,
        speed_mph: Option<f64>,
        dt_secs: f64,
    ) -> Option<GoldStandardResult> {
        // Tier 1: Direct fuel rate from PID 0x5E
        let (source, fuel_rate_lph) = if let Some(rate) = snap.engine_fuel_rate_lph {
            (GoldSource::DirectFuelRate, rate)
        } else if let Some(maf) = snap.maf_gs {
            // Tier 2 fallback: MAF-derived
            // fuel_rate (L/h) = MAF(g/s) / (AFR × density(kg/L)) × 3600 / 1000
            // = MAF / (AFR × density × 1000) × 3600
            // Simplify: MAF × 3.6 / (AFR × density)
            let rate = maf * 3.6 / (self.fuel_type.afr() * self.fuel_type.density());
            (GoldSource::MafDerived, rate)
        } else {
            return None;
        };

        let fuel_rate_gph = fuel_rate_lph / LITERS_PER_GALLON;

        // Accumulate gallons
        if dt_secs > 0.0 && fuel_rate_gph > 0.001 {
            self.gold_total_gallons += fuel_rate_gph * (dt_secs / 3600.0);
        }

        // Instant MPG = speed(mph) / fuel_rate(gal/h)
        let instant_mpg = match (speed_mph, fuel_rate_gph > 0.001) {
            (Some(mph), true) if mph > 0.5 => (mph / fuel_rate_gph).clamp(0.0, 199.9),
            _ => 0.0,
        };

        // Trip average
        let avg_mpg = if self.gold_total_gallons > 0.001 {
            (self.gold_total_miles / self.gold_total_gallons).clamp(0.0, 199.9)
        } else {
            0.0
        };

        Some(GoldStandardResult {
            source,
            fuel_rate_lph,
            instant_mpg,
            avg_mpg,
        })
    }

    fn calc_advanced(
        &mut self,
        snap: &SensorSnapshot,
        speed_mph: Option<f64>,
        dt_secs: f64,
    ) -> Option<AdvancedCalcResult> {
        // Need RPM and MAP at minimum for speed-density
        let rpm = snap.rpm?;
        let map_kpa = snap.intake_map_kpa?;

        if rpm < 1.0 {
            return None;
        }

        // IAT: prefer intake_air_temp, fallback to ambient, fallback 25°C
        let iat_c = snap
            .intake_air_temp_c
            .or(snap.ambient_air_temp_c)
            .unwrap_or(25.0);
        let iat_k = iat_c + 273.15;

        // Base MAF via ideal gas law:
        // MAF(g/s) = (MAP × VE × displacement_m³ × RPM) / (2 × 60 × R_AIR × IAT_K) × 1000
        let displacement_m3 = self.displacement_l / 1000.0;
        let base_maf_gs = (map_kpa * self.volumetric_efficiency * displacement_m3 * rpm)
            / (2.0 * 60.0 * R_AIR * iat_k)
            * 1000.0;

        // Base fuel rate: MAF / (AFR × density) × 3.6
        let base_fuel_rate_lph =
            base_maf_gs * 3.6 / (self.fuel_type.afr() * self.fuel_type.density());

        // ── Correction Factors ──────────────────────────────────────────

        // 1. Cold engine: linear -40°C → +20%, 80°C → +0%
        let cold_engine = if let Some(coolant) = snap.coolant_temp_c {
            if coolant < 80.0 {
                let frac = ((80.0 - coolant) / 120.0).clamp(0.0, 1.0); // -40→80 = 120° range
                1.0 + frac * 0.20
            } else {
                1.0
            }
        } else {
            1.0
        };

        // 2. Altitude: baro / std_baro
        let altitude = snap
            .barometric_kpa
            .map(|b| (b / STD_BARO).clamp(0.7, 1.3))
            .unwrap_or(1.0);

        // 3. Air density: 293.15K / actual_K
        let air_density_temp = snap
            .ambient_air_temp_c
            .or(snap.intake_air_temp_c)
            .unwrap_or(20.0);
        let air_density = (STD_TEMP_K / (air_density_temp + 273.15)).clamp(0.8, 1.2);

        // 4. Fuel trims: 1.0 + avg_combined_trim / 100
        let fuel_trims = calc_fuel_trim_correction(snap);

        // 5. Catalyst warmup: linear 25°C → +10%, 300°C → +0%
        let catalyst_warmup = calc_catalyst_correction(snap);

        // 6. Throttle transient: rapid opening → up to +15%
        let throttle_transient =
            if let (Some(current), Some(prev)) = (snap.throttle_pct, self.prev_throttle) {
                let delta = current - prev;
                if delta > 5.0 {
                    // Opening rapidly
                    1.0 + (delta / 100.0 * 1.5).clamp(0.0, 0.15)
                } else {
                    1.0
                }
            } else {
                1.0
            };

        // 7. High-load / WOT: >85% load → up to +20%
        let high_load_wot = if let Some(load) = snap.engine_load_pct {
            if load > 85.0 {
                let frac = ((load - 85.0) / 15.0).clamp(0.0, 1.0);
                1.0 + frac * 0.20
            } else {
                1.0
            }
        } else {
            1.0
        };

        let corrections = CorrectionFactors {
            cold_engine,
            altitude,
            air_density,
            fuel_trims,
            catalyst_warmup,
            throttle_transient,
            high_load_wot,
        };

        let corrected_fuel_rate_lph = base_fuel_rate_lph * corrections.total();
        let corrected_gph = corrected_fuel_rate_lph / LITERS_PER_GALLON;

        // Accumulate gallons
        if dt_secs > 0.0 && corrected_gph > 0.001 {
            self.advanced_total_gallons += corrected_gph * (dt_secs / 3600.0);
        }

        // Instant MPG
        let instant_mpg = match (speed_mph, corrected_gph > 0.001) {
            (Some(mph), true) if mph > 0.5 => (mph / corrected_gph).clamp(0.0, 199.9),
            _ => 0.0,
        };

        // Trip average
        let avg_mpg = if self.advanced_total_gallons > 0.001 {
            (self.advanced_total_miles / self.advanced_total_gallons).clamp(0.0, 199.9)
        } else {
            0.0
        };

        Some(AdvancedCalcResult {
            base_maf_gs,
            base_fuel_rate_lph,
            corrected_fuel_rate_lph,
            corrections,
            instant_mpg,
            avg_mpg,
        })
    }
}

impl Default for FuelEconomyState {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn push_history(history: &mut VecDeque<u64>, mpg: f64) {
    if history.len() >= MPG_HISTORY_CAP {
        history.pop_front();
    }
    // Scale MPG × 10 for sparkline resolution (e.g., 28.4 → 284)
    history.push_back((mpg * 10.0).clamp(0.0, 2000.0) as u64);
}

fn calc_fuel_trim_correction(snap: &SensorSnapshot) -> f64 {
    let mut sum = 0.0;
    let mut count = 0;

    if let Some(v) = snap.stft_b1 {
        sum += v;
        count += 1;
    }
    if let Some(v) = snap.ltft_b1 {
        sum += v;
        count += 1;
    }
    if let Some(v) = snap.stft_b2 {
        sum += v;
        count += 1;
    }
    if let Some(v) = snap.ltft_b2 {
        sum += v;
        count += 1;
    }

    if count > 0 {
        let avg = sum / count as f64;
        (1.0 + avg / 100.0).clamp(0.7, 1.3)
    } else {
        1.0
    }
}

fn calc_catalyst_correction(snap: &SensorSnapshot) -> f64 {
    let temps: Vec<f64> = [
        snap.catalyst_temp_b1s1,
        snap.catalyst_temp_b2s1,
        snap.catalyst_temp_b1s2,
        snap.catalyst_temp_b2s2,
    ]
    .iter()
    .filter_map(|t| *t)
    .collect();

    if temps.is_empty() {
        return 1.0;
    }

    let avg = temps.iter().sum::<f64>() / temps.len() as f64;

    // Linear: 25°C → +10%, 300°C → +0%
    if avg < 300.0 {
        let frac = ((300.0 - avg) / 275.0).clamp(0.0, 1.0);
        1.0 + frac * 0.10
    } else {
        1.0
    }
}
