use std::collections::VecDeque;

use crate::vehicle_data::VehicleData;

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

    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
            rpm: v.rpm,
            speed_kph: v.speed,
            coolant_temp_c: v.coolant_temp,
            engine_load_pct: v.engine_load,
            intake_map_kpa: v.intake_map,
            maf_gs: v.maf,
            intake_air_temp_c: v.intake_air_temp,
            ambient_air_temp_c: v.ambient_air_temp,
            throttle_pct: v.throttle_position,
            barometric_kpa: v.barometric_pressure,
            engine_fuel_rate_lph: v.engine_fuel_rate,
            stft_b1: v.short_fuel_trim_b1,
            ltft_b1: v.long_fuel_trim_b1,
            stft_b2: v.short_fuel_trim_b2,
            ltft_b2: v.long_fuel_trim_b2,
            catalyst_temp_b1s1: v.catalyst_temp_b1s1,
            catalyst_temp_b2s1: v.catalyst_temp_b2s1,
            catalyst_temp_b1s2: v.catalyst_temp_b1s2,
            catalyst_temp_b2s2: v.catalyst_temp_b2s2,
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
        let (source, fuel_rate_lph) = if let Some(rate) = snap.engine_fuel_rate_lph {
            (GoldSource::DirectFuelRate, rate)
        } else if let Some(maf) = snap.maf_gs {
            let rate = maf * 3.6 / (self.fuel_type.afr() * self.fuel_type.density());
            (GoldSource::MafDerived, rate)
        } else {
            return None;
        };

        let fuel_rate_gph = fuel_rate_lph / LITERS_PER_GALLON;

        if dt_secs > 0.0 && fuel_rate_gph > 0.001 {
            self.gold_total_gallons += fuel_rate_gph * (dt_secs / 3600.0);
        }

        let instant_mpg = match (speed_mph, fuel_rate_gph > 0.001) {
            (Some(mph), true) if mph > 0.5 => (mph / fuel_rate_gph).clamp(0.0, 199.9),
            _ => 0.0,
        };

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
        let rpm = snap.rpm?;
        let map_kpa = snap.intake_map_kpa?;

        if rpm < 1.0 {
            return None;
        }

        let iat_c = snap
            .intake_air_temp_c
            .or(snap.ambient_air_temp_c)
            .unwrap_or(25.0);
        let iat_k = iat_c + 273.15;

        let displacement_m3 = self.displacement_l / 1000.0;
        let base_maf_gs = (map_kpa * self.volumetric_efficiency * displacement_m3 * rpm)
            / (2.0 * 60.0 * R_AIR * iat_k)
            * 1000.0;

        let base_fuel_rate_lph =
            base_maf_gs * 3.6 / (self.fuel_type.afr() * self.fuel_type.density());

        let cold_engine = if let Some(coolant) = snap.coolant_temp_c {
            if coolant < 80.0 {
                let frac = ((80.0 - coolant) / 120.0).clamp(0.0, 1.0);
                1.0 + frac * 0.20
            } else {
                1.0
            }
        } else {
            1.0
        };

        let altitude = snap
            .barometric_kpa
            .map(|b| (b / STD_BARO).clamp(0.7, 1.3))
            .unwrap_or(1.0);

        let air_density_temp = snap
            .ambient_air_temp_c
            .or(snap.intake_air_temp_c)
            .unwrap_or(20.0);
        let air_density = (STD_TEMP_K / (air_density_temp + 273.15)).clamp(0.8, 1.2);

        let fuel_trims = calc_fuel_trim_correction(snap);
        let catalyst_warmup = calc_catalyst_correction(snap);

        let throttle_transient =
            if let (Some(current), Some(prev)) = (snap.throttle_pct, self.prev_throttle) {
                let delta = current - prev;
                if delta > 5.0 {
                    1.0 + (delta / 100.0 * 1.5).clamp(0.0, 0.15)
                } else {
                    1.0
                }
            } else {
                1.0
            };

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

        if dt_secs > 0.0 && corrected_gph > 0.001 {
            self.advanced_total_gallons += corrected_gph * (dt_secs / 3600.0);
        }

        let instant_mpg = match (speed_mph, corrected_gph > 0.001) {
            (Some(mph), true) if mph > 0.5 => (mph / corrected_gph).clamp(0.0, 199.9),
            _ => 0.0,
        };

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

    if avg < 300.0 {
        let frac = ((300.0 - avg) / 275.0).clamp(0.0, 1.0);
        1.0 + frac * 0.10
    } else {
        1.0
    }
}
