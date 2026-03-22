use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use obd2_core::protocol::pid::Pid;
use crate::domain::DomainState;
use crate::recording::format::FRAME_PID;

/// Per-PID statistical summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PidStats {
    pub name: String,
    pub unit: String,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub stddev: f64,
    pub samples: u64,
}

/// Snapshot of vehicle session data suitable for AI analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    /// Vehicle identification.
    pub vin: Option<String>,
    pub vehicle_name: Option<String>,

    /// Per-PID statistics.
    pub pid_stats: Vec<PidStats>,

    /// Diagnostic trouble codes.
    pub dtcs: Vec<String>,

    /// Fuel economy.
    pub fuel_economy_mpg: Option<f64>,
    pub fuel_rate_lph: Option<f64>,

    /// Driving behavior.
    pub hard_brake_count: u32,
    pub jackrabbit_count: u32,
    pub smoothness_score: f64,

    /// Session duration in seconds.
    pub duration_secs: Option<f64>,

    /// Alert history (recent warnings/errors).
    pub alert_history: Vec<String>,
}

impl SessionSummary {
    /// Build a summary from the current live domain state.
    pub fn from_live(domain: &DomainState) -> Self {
        let mut pid_stats = Vec::new();

        // Helper: add a single PID stat from the current reading (point-in-time snapshot).
        macro_rules! add_pid {
            ($reading:expr, $name:expr, $unit:expr) => {
                if let Some(val) = $reading {
                    pid_stats.push(PidStats {
                        name: $name.to_string(),
                        unit: $unit.to_string(),
                        min: val,
                        max: val,
                        avg: val,
                        stddev: 0.0,
                        samples: 1,
                    });
                }
            };
        }

        let v = &domain.vehicle;
        add_pid!(v.rpm, "Engine RPM", "rpm");
        add_pid!(v.speed, "Vehicle Speed", "km/h");
        add_pid!(v.coolant_temp, "Coolant Temp", "\u{00B0}C");
        add_pid!(v.engine_load, "Engine Load", "%");
        add_pid!(v.throttle_position, "Throttle Position", "%");
        add_pid!(v.intake_map, "Intake MAP", "kPa");
        add_pid!(v.intake_air_temp, "Intake Air Temp", "\u{00B0}C");
        add_pid!(v.maf, "MAF", "g/s");
        add_pid!(v.fuel_tank_level, "Fuel Tank Level", "%");
        add_pid!(v.engine_fuel_rate, "Engine Fuel Rate", "L/h");
        add_pid!(v.short_fuel_trim_b1, "Short Fuel Trim B1", "%");
        add_pid!(v.long_fuel_trim_b1, "Long Fuel Trim B1", "%");
        add_pid!(v.short_fuel_trim_b2, "Short Fuel Trim B2", "%");
        add_pid!(v.long_fuel_trim_b2, "Long Fuel Trim B2", "%");
        add_pid!(v.barometric_pressure, "Barometric Pressure", "kPa");
        add_pid!(v.ambient_air_temp, "Ambient Air Temp", "\u{00B0}C");
        add_pid!(v.engine_oil_temp, "Engine Oil Temp", "\u{00B0}C");
        add_pid!(v.transmission_temp, "Transmission Temp", "\u{00B0}C");
        add_pid!(v.catalyst_temp_b1s1, "Catalyst Temp B1S1", "\u{00B0}C");
        add_pid!(v.catalyst_temp_b2s1, "Catalyst Temp B2S1", "\u{00B0}C");
        add_pid!(v.oil_pressure, "Oil Pressure", "kPa");
        add_pid!(v.timing_advance, "Timing Advance", "\u{00B0}");
        add_pid!(v.fuel_pressure, "Fuel Pressure", "kPa");
        add_pid!(v.commanded_egr, "Commanded EGR", "%");
        add_pid!(v.commanded_evap_purge, "Commanded EVAP Purge", "%");
        add_pid!(v.commanded_equiv_ratio, "Commanded Equiv Ratio", "\u{03BB}");
        add_pid!(v.absolute_load, "Absolute Load", "%");

        if let Some(volts) = v.battery_voltage {
            pid_stats.push(PidStats {
                name: "Battery Voltage".to_string(),
                unit: "V".to_string(),
                min: volts,
                max: volts,
                avg: volts,
                stddev: 0.0,
                samples: 1,
            });
        }

        let dtcs = domain.stored_dtcs.iter().map(|d| d.code.clone()).collect();

        let fuel_economy_mpg = domain
            .fuel_economy
            .gold
            .as_ref()
            .map(|g| g.avg_mpg)
            .or(domain.fuel_economy.advanced.as_ref().map(|a| a.avg_mpg));

        let fuel_rate_lph = domain
            .fuel_economy
            .gold
            .as_ref()
            .map(|g| g.fuel_rate_lph)
            .or(domain
                .fuel_economy
                .advanced
                .as_ref()
                .map(|a| a.corrected_fuel_rate_lph));

        let duration_secs = v.run_time;

        let alert_history = domain.alert_history.iter().cloned().collect();

        SessionSummary {
            vin: domain.vehicle_info.as_ref().map(|i| i.vin.clone()),
            vehicle_name: domain.vehicle_info.as_ref().map(|i| i.display_name()),
            pid_stats,
            dtcs,
            fuel_economy_mpg,
            fuel_rate_lph,
            hard_brake_count: domain.driving.hard_brake_count,
            jackrabbit_count: domain.driving.jackrabbit_count,
            smoothness_score: domain.driving.smoothness_score,
            duration_secs,
            alert_history,
        }
    }

    /// Build a summary by replaying a `.obd2rec` recording file.
    #[allow(dead_code)]
    pub fn from_recording(path: &Path) -> anyhow::Result<Self> {
        let (header, frames) = crate::recording::reader::read_recording(path)?;

        struct Accumulator {
            name: String,
            unit: String,
            min: f64,
            max: f64,
            sum: f64,
            sum_sq: f64,
            count: u64,
        }

        let mut accumulators: HashMap<u8, Accumulator> = HashMap::new();
        let mut dtc_set: Vec<String> = Vec::new();
        let mut max_offset_ms: u32 = 0;

        for frame in &frames {
            if frame.offset_ms > max_offset_ms {
                max_offset_ms = frame.offset_ms;
            }

            if frame.frame_type == FRAME_PID {
                let pid_code = frame.pid_code;
                let value = frame.value;

                let acc = accumulators.entry(pid_code).or_insert_with(|| {
                    let pid = Pid::from_code(pid_code);
                    let name = pid.name().to_string();
                    let unit = pid.unit().to_string();
                    Accumulator {
                        name,
                        unit,
                        min: f64::MAX,
                        max: f64::MIN,
                        sum: 0.0,
                        sum_sq: 0.0,
                        count: 0,
                    }
                });

                if value < acc.min {
                    acc.min = value;
                }
                if value > acc.max {
                    acc.max = value;
                }
                acc.sum += value;
                acc.sum_sq += value * value;
                acc.count += 1;
            } else if frame.frame_type == crate::recording::format::FRAME_DTC {
                if let Some(code) = frame.decode_dtc_code() {
                    if !dtc_set.contains(&code) {
                        dtc_set.push(code);
                    }
                }
            }
        }

        let mut pid_stats: Vec<PidStats> = accumulators
            .into_values()
            .map(|acc| {
                let avg = if acc.count > 0 {
                    acc.sum / acc.count as f64
                } else {
                    0.0
                };
                let variance = if acc.count > 1 {
                    (acc.sum_sq / acc.count as f64) - (avg * avg)
                } else {
                    0.0
                };
                let stddev = if variance > 0.0 { variance.sqrt() } else { 0.0 };
                PidStats {
                    name: acc.name,
                    unit: acc.unit,
                    min: acc.min,
                    max: acc.max,
                    avg,
                    stddev,
                    samples: acc.count,
                }
            })
            .collect();

        pid_stats.sort_by(|a, b| a.name.cmp(&b.name));

        let duration_secs = Some(max_offset_ms as f64 / 1000.0);

        Ok(SessionSummary {
            vin: header.vin,
            vehicle_name: header.vehicle_name,
            pid_stats,
            dtcs: dtc_set,
            fuel_economy_mpg: None,
            fuel_rate_lph: None,
            hard_brake_count: 0,
            jackrabbit_count: 0,
            smoothness_score: 0.0,
            duration_secs,
            alert_history: Vec::new(),
        })
    }
}
