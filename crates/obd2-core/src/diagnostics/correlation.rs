use std::collections::HashMap;

use obd2_db::models::{ResolvedThreshold, VehicleInfo};
use crate::obd2::dtc::Dtc;
use crate::obd2::types::VehicleData;
use crate::obd2::Pid;

use super::provider::{DiagnosticContext, SensorSnapshot, SensorStatus, VehicleInfoSummary};

// ─── DTC-to-PID correlation table ────────────────────────────────────────────

struct DtcCorrelation {
    pattern: &'static str,
    pids: &'static [Pid],
}

/// Static table mapping DTC codes (or prefixes) to related PIDs.
/// Exact matches are listed first, then prefix matches (shortest last).
static DTC_CORRELATIONS: &[DtcCorrelation] = &[
    // Fuel/Air — lean
    DtcCorrelation {
        pattern: "P0171",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
            Pid::FuelPressure,
            Pid::IntakeMap,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0174",
        pids: &[
            Pid::ShortFuelTrimBank2,
            Pid::LongFuelTrimBank2,
            Pid::Maf,
            Pid::FuelPressure,
            Pid::IntakeMap,
            Pid::EngineLoad,
        ],
    },
    // Fuel/Air — rich
    DtcCorrelation {
        pattern: "P0172",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
            Pid::FuelPressure,
            Pid::IntakeMap,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0175",
        pids: &[
            Pid::ShortFuelTrimBank2,
            Pid::LongFuelTrimBank2,
            Pid::Maf,
            Pid::FuelPressure,
            Pid::IntakeMap,
            Pid::EngineLoad,
        ],
    },
    // Misfire
    DtcCorrelation {
        pattern: "P0300",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
            Pid::IntakeMap,
        ],
    },
    DtcCorrelation {
        pattern: "P0301",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P0302",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P0303",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P0304",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
        ],
    },
    // Catalyst efficiency
    DtcCorrelation {
        pattern: "P0420",
        pids: &[
            Pid::CatalystTempB1S1,
            Pid::CatalystTempB1S2,
            Pid::CoolantTemp,
            Pid::EngineLoad,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
        ],
    },
    DtcCorrelation {
        pattern: "P0430",
        pids: &[
            Pid::CatalystTempB2S1,
            Pid::CatalystTempB2S2,
            Pid::CoolantTemp,
            Pid::EngineLoad,
            Pid::ShortFuelTrimBank2,
            Pid::LongFuelTrimBank2,
        ],
    },
    // Intake air temperature sensor
    DtcCorrelation {
        pattern: "P0110",
        pids: &[
            Pid::IntakeAirTemp,
            Pid::CoolantTemp,
            Pid::AmbientAirTemp,
            Pid::EngineLoad,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P0111",
        pids: &[
            Pid::IntakeAirTemp,
            Pid::CoolantTemp,
            Pid::AmbientAirTemp,
            Pid::EngineLoad,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P0112",
        pids: &[
            Pid::IntakeAirTemp,
            Pid::CoolantTemp,
            Pid::AmbientAirTemp,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0113",
        pids: &[
            Pid::IntakeAirTemp,
            Pid::CoolantTemp,
            Pid::AmbientAirTemp,
            Pid::EngineLoad,
        ],
    },
    // Idle control
    DtcCorrelation {
        pattern: "P0505",
        pids: &[
            Pid::EngineRpm,
            Pid::ThrottlePosition,
            Pid::EngineLoad,
            Pid::IntakeMap,
            Pid::CoolantTemp,
        ],
    },
    DtcCorrelation {
        pattern: "P0507",
        pids: &[
            Pid::EngineRpm,
            Pid::ThrottlePosition,
            Pid::EngineLoad,
            Pid::IntakeMap,
            Pid::CoolantTemp,
        ],
    },
    // Transmission
    DtcCorrelation {
        pattern: "P0700",
        pids: &[
            Pid::VehicleSpeed,
            Pid::TransmissionTemp,
            Pid::EngineLoad,
            Pid::EngineRpm,
        ],
    },
    DtcCorrelation {
        pattern: "P0715",
        pids: &[
            Pid::VehicleSpeed,
            Pid::TransmissionTemp,
            Pid::EngineLoad,
            Pid::EngineRpm,
        ],
    },
    DtcCorrelation {
        pattern: "P0720",
        pids: &[
            Pid::VehicleSpeed,
            Pid::TransmissionTemp,
            Pid::EngineLoad,
            Pid::EngineRpm,
        ],
    },
    // Turbo codes
    DtcCorrelation {
        pattern: "P0234",
        pids: &[
            Pid::IntakeMap,
            Pid::BarometricPressure,
            Pid::EngineLoad,
            Pid::EngineRpm,
            Pid::ThrottlePosition,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P0299",
        pids: &[
            Pid::IntakeMap,
            Pid::BarometricPressure,
            Pid::EngineLoad,
            Pid::EngineRpm,
            Pid::ThrottlePosition,
            Pid::Maf,
        ],
    },
    DtcCorrelation {
        pattern: "P1101",
        pids: &[
            Pid::Maf,
            Pid::IntakeMap,
            Pid::ThrottlePosition,
            Pid::EngineLoad,
            Pid::CommandedEquivRatio,
        ],
    },
    // VVT / camshaft timing
    DtcCorrelation {
        pattern: "P0010",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineOilTemp,
            Pid::EngineLoad,
            Pid::TimingAdvance,
        ],
    },
    DtcCorrelation {
        pattern: "P0011",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineOilTemp,
            Pid::EngineLoad,
            Pid::TimingAdvance,
        ],
    },
    DtcCorrelation {
        pattern: "P0014",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineOilTemp,
            Pid::EngineLoad,
            Pid::TimingAdvance,
        ],
    },
    DtcCorrelation {
        pattern: "P0016",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineOilTemp,
            Pid::EngineLoad,
            Pid::TimingAdvance,
        ],
    },
    // O2 heater circuits
    DtcCorrelation {
        pattern: "P0030",
        pids: &[
            Pid::ControlModuleVoltage,
            Pid::CoolantTemp,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0036",
        pids: &[
            Pid::ControlModuleVoltage,
            Pid::CoolantTemp,
            Pid::EngineLoad,
        ],
    },
    // O2 sensor voltage codes
    DtcCorrelation {
        pattern: "P0131",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::CommandedEquivRatio,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0132",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::CommandedEquivRatio,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0133",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::CommandedEquivRatio,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0134",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::CommandedEquivRatio,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0137",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::CommandedEquivRatio,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0138",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::CommandedEquivRatio,
            Pid::EngineLoad,
        ],
    },
    // Thermostat
    DtcCorrelation {
        pattern: "P0597",
        pids: &[
            Pid::CoolantTemp,
            Pid::IntakeAirTemp,
            Pid::ControlModuleVoltage,
        ],
    },
    // Transmission extras
    DtcCorrelation {
        pattern: "P0711",
        pids: &[
            Pid::TransmissionTemp,
            Pid::CoolantTemp,
            Pid::VehicleSpeed,
            Pid::EngineRpm,
        ],
    },
    DtcCorrelation {
        pattern: "P0717",
        pids: &[
            Pid::VehicleSpeed,
            Pid::TransmissionTemp,
            Pid::EngineRpm,
            Pid::EngineLoad,
        ],
    },
    DtcCorrelation {
        pattern: "P0741",
        pids: &[
            Pid::TransmissionTemp,
            Pid::VehicleSpeed,
            Pid::EngineRpm,
            Pid::EngineLoad,
        ],
    },
    // Idle low
    DtcCorrelation {
        pattern: "P0506",
        pids: &[
            Pid::EngineRpm,
            Pid::ThrottlePosition,
            Pid::EngineLoad,
            Pid::IntakeMap,
            Pid::CoolantTemp,
            Pid::CommandedThrottleActuator,
        ],
    },
    // EVAP extras
    DtcCorrelation {
        pattern: "P0449",
        pids: &[
            Pid::CommandedEvapPurge,
            Pid::FuelTankLevel,
            Pid::BarometricPressure,
        ],
    },
    DtcCorrelation {
        pattern: "P0496",
        pids: &[
            Pid::CommandedEvapPurge,
            Pid::FuelTankLevel,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
        ],
    },
    // ─── Prefix matches (fallback groups) ────────────────────────────────
    // P01xx — Fuel/Air metering
    DtcCorrelation {
        pattern: "P01",
        pids: &[
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
            Pid::Maf,
            Pid::FuelPressure,
            Pid::IntakeMap,
            Pid::EngineLoad,
        ],
    },
    // P03xx — Ignition/Misfire
    DtcCorrelation {
        pattern: "P03",
        pids: &[
            Pid::EngineRpm,
            Pid::EngineLoad,
            Pid::CoolantTemp,
            Pid::ShortFuelTrimBank1,
            Pid::Maf,
        ],
    },
    // P04xx — Emissions
    DtcCorrelation {
        pattern: "P04",
        pids: &[
            Pid::CatalystTempB1S1,
            Pid::CoolantTemp,
            Pid::EngineLoad,
            Pid::ShortFuelTrimBank1,
            Pid::LongFuelTrimBank1,
        ],
    },
    // P05xx — Idle/Speed
    DtcCorrelation {
        pattern: "P05",
        pids: &[
            Pid::EngineRpm,
            Pid::VehicleSpeed,
            Pid::ThrottlePosition,
            Pid::EngineLoad,
            Pid::IntakeMap,
        ],
    },
    // P02xx — Turbo / fuel system
    DtcCorrelation {
        pattern: "P02",
        pids: &[
            Pid::IntakeMap,
            Pid::BarometricPressure,
            Pid::Maf,
            Pid::EngineLoad,
            Pid::EngineRpm,
        ],
    },
    // P07xx — Transmission
    DtcCorrelation {
        pattern: "P07",
        pids: &[
            Pid::VehicleSpeed,
            Pid::TransmissionTemp,
            Pid::EngineLoad,
            Pid::EngineRpm,
        ],
    },
    // Bxxxx — Body
    DtcCorrelation {
        pattern: "B",
        pids: &[
            Pid::ControlModuleVoltage,
        ],
    },
    // Cxxxx — Chassis
    DtcCorrelation {
        pattern: "C",
        pids: &[
            Pid::VehicleSpeed,
            Pid::ControlModuleVoltage,
        ],
    },
    // Uxxxx — Network
    DtcCorrelation {
        pattern: "U",
        pids: &[
            Pid::ControlModuleVoltage,
        ],
    },
];

/// Look up correlated PIDs for a DTC code.
/// Tries exact match first, then longest prefix match.
fn get_correlated_pids(dtc_code: &str) -> &'static [Pid] {
    // Exact match
    for entry in DTC_CORRELATIONS {
        if entry.pattern == dtc_code {
            return entry.pids;
        }
    }
    // Longest prefix match
    let mut best: Option<&DtcCorrelation> = None;
    for entry in DTC_CORRELATIONS {
        if dtc_code.starts_with(entry.pattern) {
            match best {
                None => best = Some(entry),
                Some(current) => {
                    if entry.pattern.len() > current.pattern.len() {
                        best = Some(entry);
                    }
                }
            }
        }
    }
    best.map(|e| e.pids).unwrap_or(&[])
}

// ─── Sensor value extraction ─────────────────────────────────────────────────

/// Get the current value for a PID from VehicleData.
fn pid_value(v: &VehicleData, pid: Pid) -> Option<f64> {
    match pid {
        Pid::EngineLoad => v.engine_load.as_ref().map(|r| r.value),
        Pid::CoolantTemp => v.coolant_temp.as_ref().map(|r| r.value),
        Pid::ShortFuelTrimBank1 => v.short_fuel_trim_b1.as_ref().map(|r| r.value),
        Pid::LongFuelTrimBank1 => v.long_fuel_trim_b1.as_ref().map(|r| r.value),
        Pid::ShortFuelTrimBank2 => v.short_fuel_trim_b2.as_ref().map(|r| r.value),
        Pid::LongFuelTrimBank2 => v.long_fuel_trim_b2.as_ref().map(|r| r.value),
        Pid::FuelPressure => v.fuel_pressure.as_ref().map(|r| r.value),
        Pid::IntakeMap => v.intake_map.as_ref().map(|r| r.value),
        Pid::EngineRpm => v.rpm.as_ref().map(|r| r.value),
        Pid::VehicleSpeed => v.speed.as_ref().map(|r| r.value),
        Pid::IntakeAirTemp => v.intake_air_temp.as_ref().map(|r| r.value),
        Pid::Maf => v.maf.as_ref().map(|r| r.value),
        Pid::ThrottlePosition => v.throttle_position.as_ref().map(|r| r.value),
        Pid::FuelTankLevel => v.fuel_tank_level.as_ref().map(|r| r.value),
        Pid::BarometricPressure => v.barometric_pressure.as_ref().map(|r| r.value),
        Pid::CatalystTempB1S1 => v.catalyst_temp_b1s1.as_ref().map(|r| r.value),
        Pid::CatalystTempB2S1 => v.catalyst_temp_b2s1.as_ref().map(|r| r.value),
        Pid::CatalystTempB1S2 => v.catalyst_temp_b1s2.as_ref().map(|r| r.value),
        Pid::CatalystTempB2S2 => v.catalyst_temp_b2s2.as_ref().map(|r| r.value),
        Pid::ControlModuleVoltage => v.control_module_voltage.as_ref().map(|r| r.value),
        Pid::AmbientAirTemp => v.ambient_air_temp.as_ref().map(|r| r.value),
        Pid::EngineOilTemp => v.engine_oil_temp.as_ref().map(|r| r.value),
        Pid::EngineFuelRate => v.engine_fuel_rate.as_ref().map(|r| r.value),
        Pid::TransmissionTemp => v.transmission_temp.as_ref().map(|r| r.value),
        Pid::OilPressure => v.oil_pressure.as_ref().map(|r| r.value),
        Pid::TimingAdvance => v.timing_advance.as_ref().map(|r| r.value),
        Pid::RunTimeSinceStart => v.run_time.as_ref().map(|r| r.value),
        Pid::DistanceWithMil => v.distance_with_mil.as_ref().map(|r| r.value),
        Pid::FuelRailGaugePressure => v.fuel_rail_gauge_pressure.as_ref().map(|r| r.value),
        Pid::CommandedEgr => v.commanded_egr.as_ref().map(|r| r.value),
        Pid::CommandedEvapPurge => v.commanded_evap_purge.as_ref().map(|r| r.value),
        Pid::DistanceSinceDtcClear => v.distance_since_dtc_clear.as_ref().map(|r| r.value),
        Pid::AbsoluteLoad => v.absolute_load.as_ref().map(|r| r.value),
        Pid::CommandedEquivRatio => v.commanded_equiv_ratio.as_ref().map(|r| r.value),
        Pid::RelativeThrottlePos => v.relative_throttle_pos.as_ref().map(|r| r.value),
        Pid::AbsThrottlePosB => v.abs_throttle_pos_b.as_ref().map(|r| r.value),
        Pid::AccelPedalPosD => v.accel_pedal_pos_d.as_ref().map(|r| r.value),
        Pid::AccelPedalPosE => v.accel_pedal_pos_e.as_ref().map(|r| r.value),
        Pid::CommandedThrottleActuator => v.commanded_throttle_actuator.as_ref().map(|r| r.value),
        Pid::FuelRailAbsPressure => v.fuel_rail_abs_pressure.as_ref().map(|r| r.value),
        Pid::DemandedTorque => v.demanded_torque.as_ref().map(|r| r.value),
        Pid::ActualTorque => v.actual_torque.as_ref().map(|r| r.value),
        Pid::ReferenceTorque => v.reference_torque.as_ref().map(|r| r.value),
    }
}

/// Determine sensor status from threshold checks.
fn sensor_status(thresholds: &HashMap<u8, ResolvedThreshold>, pid: Pid, value: Option<f64>) -> SensorStatus {
    let value = match value {
        Some(v) => v,
        None => return SensorStatus::NoData,
    };

    if let Some(threshold) = thresholds.get(&pid.code()) {
        // Critical checks
        if let Some(hc) = threshold.high_critical {
            if value > hc {
                return SensorStatus::Critical;
            }
        }
        if let Some(lc) = threshold.low_critical {
            if value < lc {
                return SensorStatus::Critical;
            }
        }
        // Warning checks
        if let Some(hw) = threshold.high_warning {
            if value > hw {
                return SensorStatus::Warning;
            }
        }
        if let Some(lw) = threshold.low_warning {
            if value < lw {
                return SensorStatus::Warning;
            }
        }
        SensorStatus::Ok
    } else {
        SensorStatus::Ok
    }
}

/// Compact PID name for popup display.
fn short_name(pid: Pid) -> &'static str {
    match pid {
        Pid::EngineRpm => "RPM",
        Pid::VehicleSpeed => "Speed",
        Pid::EngineLoad => "Eng Load",
        Pid::CoolantTemp => "Coolant",
        Pid::ShortFuelTrimBank1 => "STFT B1",
        Pid::LongFuelTrimBank1 => "LTFT B1",
        Pid::ShortFuelTrimBank2 => "STFT B2",
        Pid::LongFuelTrimBank2 => "LTFT B2",
        Pid::FuelPressure => "Fuel P",
        Pid::IntakeMap => "MAP",
        Pid::IntakeAirTemp => "IAT",
        Pid::Maf => "MAF",
        Pid::ThrottlePosition => "Throttle",
        Pid::FuelTankLevel => "Fuel Level",
        Pid::BarometricPressure => "Baro",
        Pid::CatalystTempB1S1 => "Cat B1S1",
        Pid::CatalystTempB2S1 => "Cat B2S1",
        Pid::CatalystTempB1S2 => "Cat B1S2",
        Pid::CatalystTempB2S2 => "Cat B2S2",
        Pid::ControlModuleVoltage => "Ctrl Volt",
        Pid::AmbientAirTemp => "Ambient",
        Pid::EngineOilTemp => "Oil Temp",
        Pid::EngineFuelRate => "Fuel Rate",
        Pid::TransmissionTemp => "Trans Temp",
        Pid::OilPressure => "Oil P",
        Pid::TimingAdvance => "Timing",
        Pid::RunTimeSinceStart => "Run Time",
        Pid::DistanceWithMil => "Dist MIL",
        Pid::FuelRailGaugePressure => "Rail P",
        Pid::CommandedEgr => "EGR",
        Pid::CommandedEvapPurge => "EVAP Purge",
        Pid::DistanceSinceDtcClear => "Dist CLR",
        Pid::AbsoluteLoad => "Abs Load",
        Pid::CommandedEquivRatio => "Equiv λ",
        Pid::RelativeThrottlePos => "Rel Throt",
        Pid::AbsThrottlePosB => "Throt B",
        Pid::AccelPedalPosD => "Pedal D",
        Pid::AccelPedalPosE => "Pedal E",
        Pid::CommandedThrottleActuator => "Cmd Throt",
        Pid::FuelRailAbsPressure => "Rail Abs P",
        Pid::DemandedTorque => "Dem Torq",
        Pid::ActualTorque => "Act Torq",
        Pid::ReferenceTorque => "Ref Torq",
    }
}

// ─── Public builders ─────────────────────────────────────────────────────────

/// Build sensor snapshots for the given PIDs from current state.
fn build_sensor_snapshots(vehicle: &VehicleData, thresholds: &HashMap<u8, ResolvedThreshold>, pids: &[Pid]) -> Vec<SensorSnapshot> {
    pids.iter()
        .map(|&pid| {
            let value = pid_value(vehicle, pid);
            let status = sensor_status(thresholds, pid, value);
            SensorSnapshot {
                pid_code: pid.code(),
                name: short_name(pid),
                value,
                unit: pid.unit(),
                status,
            }
        })
        .collect()
}

/// Assemble the full DiagnosticContext for a DTC popup.
pub fn build_diagnostic_context(
    vehicle: &VehicleData,
    thresholds: &HashMap<u8, ResolvedThreshold>,
    stored_dtcs: &[Dtc],
    vehicle_info: Option<&VehicleInfo>,
    code: &str,
    description: &str,
    category: &str,
) -> DiagnosticContext {
    let correlated_pids = get_correlated_pids(code);
    let related_sensors = build_sensor_snapshots(vehicle, thresholds, correlated_pids);

    let other_dtcs: Vec<(String, String)> = stored_dtcs
        .iter()
        .filter(|dtc| dtc.code != code)
        .map(|dtc| (dtc.code.clone(), dtc.description.to_string()))
        .collect();

    let vehicle_info = vehicle_info.map(VehicleInfoSummary::from);

    DiagnosticContext {
        dtc_code: code.to_string(),
        dtc_description: description.to_string(),
        dtc_category: category.to_string(),
        related_sensors,
        other_dtcs,
        vehicle_info,
    }
}

// ─── Static guidance ─────────────────────────────────────────────────────────

/// Return common causes for a known DTC code.
pub fn common_causes(code: &str) -> Option<&'static [&'static str]> {
    match code {
        // Fuel/Air — lean
        "P0171" | "P0174" => Some(&[
            "Vacuum leak (intake manifold, hoses, PCV)",
            "Dirty or faulty MAF sensor",
            "Weak fuel pump or clogged fuel filter",
            "Faulty O2 sensor (upstream)",
            "Exhaust leak before O2 sensor",
        ]),
        // Fuel/Air — rich
        "P0172" | "P0175" => Some(&[
            "Leaking fuel injector(s)",
            "High fuel pressure (faulty regulator)",
            "Dirty or faulty MAF sensor",
            "Stuck-open EVAP purge valve",
            "Faulty O2 sensor (upstream)",
        ]),
        // Misfire
        "P0300" => Some(&[
            "Worn or fouled spark plugs",
            "Faulty ignition coil(s)",
            "Vacuum leak causing lean misfire",
            "Low fuel pressure",
            "Low compression (head gasket, valves)",
        ]),
        "P0301" | "P0302" | "P0303" | "P0304" => Some(&[
            "Worn or fouled spark plug (specific cylinder)",
            "Faulty ignition coil (specific cylinder)",
            "Leaking fuel injector (specific cylinder)",
            "Low compression (specific cylinder)",
            "Vacuum leak near specific cylinder",
        ]),
        // Emissions — catalyst
        "P0420" | "P0430" => Some(&[
            "Worn or contaminated catalytic converter",
            "Faulty downstream O2 sensor",
            "Exhaust leak before or after catalyst",
            "Engine misfire damaging catalyst",
            "Rich/lean condition degrading catalyst",
        ]),
        // Emissions — EVAP
        "P0440" => Some(&[
            "Loose or damaged fuel cap",
            "Cracked or disconnected EVAP hose",
            "Faulty EVAP purge valve",
            "Faulty EVAP vent valve",
            "Leak in charcoal canister",
        ]),
        "P0442" => Some(&[
            "Loose fuel cap",
            "Small crack in EVAP hose",
            "Faulty purge valve (slight leak)",
            "Faulty vent valve (slight leak)",
            "Hairline crack in fuel tank",
        ]),
        "P0455" => Some(&[
            "Missing or damaged fuel cap",
            "Disconnected EVAP hose",
            "Faulty EVAP purge solenoid (stuck open)",
            "Faulty EVAP vent solenoid (stuck open)",
            "Cracked charcoal canister",
        ]),
        // EGR
        "P0401" => Some(&[
            "Clogged EGR passages (carbon buildup)",
            "Faulty EGR valve (stuck closed)",
            "Faulty EGR position sensor",
            "Vacuum supply issue to EGR valve",
            "Faulty EGR solenoid",
        ]),
        // Idle control
        "P0505" | "P0507" => Some(&[
            "Faulty idle air control (IAC) valve",
            "Vacuum leak affecting idle",
            "Dirty throttle body",
            "Faulty throttle position sensor",
            "Intake manifold gasket leak",
        ]),
        // Speed sensor
        "P0500" => Some(&[
            "Faulty vehicle speed sensor",
            "Damaged wiring to speed sensor",
            "Corroded connector at speed sensor",
            "Faulty instrument cluster",
            "Damaged tone ring",
        ]),
        // Transmission
        "P0700" => Some(&[
            "Internal transmission solenoid failure",
            "Low or contaminated transmission fluid",
            "Damaged wiring to transmission",
            "Faulty transmission control module (TCM)",
            "Worn clutch packs or bands",
        ]),
        "P0715" => Some(&[
            "Faulty input/turbine speed sensor",
            "Damaged wiring to input speed sensor",
            "Corroded connector at sensor",
            "Low transmission fluid",
            "Internal transmission damage",
        ]),
        "P0720" => Some(&[
            "Faulty output speed sensor",
            "Damaged wiring to output speed sensor",
            "Corroded connector at sensor",
            "Low transmission fluid",
            "Internal transmission damage",
        ]),
        // Sensor circuits
        "P0100" | "P0101" | "P0102" => Some(&[
            "Dirty or contaminated MAF sensor element",
            "Air leak between MAF sensor and throttle",
            "Damaged wiring or connector at MAF",
            "Faulty MAF sensor",
            "Restricted air filter",
        ]),
        "P0110" => Some(&[
            "Faulty intake air temperature sensor",
            "Damaged wiring to IAT sensor",
            "Corroded IAT sensor connector",
            "IAT sensor installed incorrectly",
            "Open or short in IAT circuit",
        ]),
        "P0111" => Some(&[
            "IAT sensor reading outside expected range for conditions",
            "Dirty or contaminated IAT sensor element",
            "IAT sensor positioned too close to heat source",
            "Intake air leak affecting sensor reading",
            "Faulty IAT sensor (slow response or drift)",
            "Corroded or loose IAT sensor connector",
        ]),
        "P0112" => Some(&[
            "Short to ground in IAT sensor circuit",
            "Faulty IAT sensor (internal short)",
            "Damaged wiring to IAT sensor",
            "Water intrusion in IAT sensor connector",
            "Faulty ECM/PCM IAT input circuit",
        ]),
        "P0113" => Some(&[
            "Open circuit in IAT sensor wiring",
            "Disconnected IAT sensor connector",
            "Faulty IAT sensor (open element)",
            "Corroded terminals at IAT sensor",
            "Faulty ECM/PCM IAT input circuit",
        ]),
        "P0120" => Some(&[
            "Faulty throttle position sensor (TPS)",
            "Damaged wiring to TPS",
            "Corroded TPS connector",
            "Throttle body mechanical failure",
            "TPS out of adjustment",
        ]),
        "P0130" => Some(&[
            "Faulty O2 sensor (Bank 1 Sensor 1)",
            "Damaged wiring to O2 sensor",
            "Exhaust leak near O2 sensor",
            "Contaminated O2 sensor (coolant/oil)",
            "Faulty PCM O2 heater circuit",
        ]),
        // Turbo codes
        "P0234" => Some(&[
            "Stuck wastegate actuator",
            "Faulty boost pressure sensor",
            "Boost solenoid stuck closed",
            "Restricted exhaust causing backpressure",
            "Faulty turbo control valve",
        ]),
        "P0299" => Some(&[
            "Boost leak in charge piping or intercooler",
            "Stuck-open wastegate",
            "Worn turbocharger (shaft play)",
            "Restricted air intake or clogged filter",
            "Faulty boost pressure solenoid",
        ]),
        // GM intake airflow
        "P1101" => Some(&[
            "Vacuum leak in intake system",
            "Dirty or faulty MAF sensor",
            "Restricted air filter",
            "PCV valve or hose leak",
            "Throttle body carbon buildup",
        ]),
        // VVT / camshaft timing
        "P0010" | "P0011" | "P0014" | "P0016" => Some(&[
            "Low or dirty engine oil (VVT depends on oil pressure)",
            "Faulty camshaft position actuator solenoid",
            "Clogged oil passages to VVT actuator",
            "Stretched or jumped timing chain",
            "Faulty camshaft position sensor",
        ]),
        // Idle low
        "P0506" => Some(&[
            "Vacuum leak causing low idle",
            "Dirty throttle body",
            "Faulty idle air control valve",
            "Carbon buildup in intake manifold",
            "Faulty throttle position sensor",
        ]),
        // O2 heater circuits
        "P0030" | "P0036" => Some(&[
            "Blown O2 heater fuse",
            "Faulty O2 sensor heater element",
            "Damaged wiring to O2 sensor heater",
            "Corroded O2 sensor connector",
            "Faulty PCM heater driver circuit",
        ]),
        // O2 sensor voltage
        "P0131" | "P0132" | "P0133" | "P0134"
        | "P0137" | "P0138" => Some(&[
            "Faulty O2 sensor",
            "Exhaust leak near O2 sensor",
            "Wiring damage or short in O2 circuit",
            "Contaminated O2 sensor (oil/coolant leak)",
            "Fuel system imbalance (rich or lean)",
        ]),
        // Thermostat
        "P0597" | "P0598" | "P0599" => Some(&[
            "Faulty electronically controlled thermostat",
            "Damaged wiring to thermostat heater",
            "Corroded thermostat connector",
            "Faulty PCM thermostat driver circuit",
            "Low coolant level affecting thermostat",
        ]),
        // Transmission extras
        "P0711" => Some(&[
            "Faulty transmission fluid temperature sensor",
            "Damaged wiring to trans temp sensor",
            "Corroded sensor connector",
            "Low or contaminated transmission fluid",
            "Internal transmission damage",
        ]),
        "P0741" => Some(&[
            "Faulty torque converter clutch solenoid",
            "Low or contaminated transmission fluid",
            "Worn torque converter clutch",
            "Damaged wiring to TCC solenoid",
            "Internal transmission valve body issue",
        ]),
        _ => None,
    }
}

/// Return suggested diagnostic actions for a known DTC code.
pub fn suggested_actions(code: &str) -> Option<&'static [&'static str]> {
    match code {
        "P0171" | "P0174" => Some(&[
            "Check for vacuum leaks (smoke test)",
            "Clean or replace MAF sensor",
            "Check fuel pressure at the rail",
            "Inspect O2 sensor readings (live data)",
            "Check for exhaust leaks before O2 sensor",
        ]),
        "P0172" | "P0175" => Some(&[
            "Check fuel pressure (should not be too high)",
            "Inspect fuel injectors for leaks",
            "Clean or replace MAF sensor",
            "Check EVAP purge valve operation",
            "Inspect O2 sensor readings (live data)",
        ]),
        "P0300" => Some(&[
            "Inspect spark plugs and replace if worn",
            "Test ignition coils one at a time",
            "Check for vacuum leaks (smoke test)",
            "Check fuel pressure and injector balance",
            "Perform compression test if above pass",
        ]),
        "P0301" | "P0302" | "P0303" | "P0304" => Some(&[
            "Swap ignition coil with adjacent cylinder",
            "Inspect/replace spark plug on that cylinder",
            "Check injector operation (noid light test)",
            "Perform compression test on that cylinder",
            "Check for vacuum leak near that cylinder",
        ]),
        "P0420" | "P0430" => Some(&[
            "Check for exhaust leaks before and after cat",
            "Compare upstream vs downstream O2 signals",
            "Check for active misfire codes (P03xx)",
            "Inspect catalyst for rattle or damage",
            "Monitor catalyst temperature differential",
        ]),
        "P0440" | "P0442" | "P0455" => Some(&[
            "Inspect and tighten fuel cap (replace if worn)",
            "Perform EVAP smoke test for leaks",
            "Check EVAP purge solenoid operation",
            "Check EVAP vent solenoid operation",
            "Inspect EVAP hoses and charcoal canister",
        ]),
        "P0401" => Some(&[
            "Clean EGR valve and passages",
            "Test EGR valve vacuum operation",
            "Check EGR position sensor signal",
            "Inspect vacuum lines to EGR valve",
            "Check EGR solenoid operation",
        ]),
        "P0505" | "P0507" => Some(&[
            "Clean throttle body and IAC passages",
            "Test IAC valve operation",
            "Check for vacuum leaks (smoke test)",
            "Check throttle position sensor signal",
            "Reset idle learn procedure after repairs",
        ]),
        "P0500" => Some(&[
            "Inspect vehicle speed sensor and wiring",
            "Check sensor connector for corrosion",
            "Test sensor output with multimeter",
            "Inspect tone ring for damage",
            "Check for related ABS codes",
        ]),
        "P0700" | "P0715" | "P0720" => Some(&[
            "Check transmission fluid level and condition",
            "Scan for specific transmission sub-codes",
            "Inspect wiring to transmission sensors",
            "Test speed sensor resistance/output",
            "Check for TCM software updates",
        ]),
        "P0100" | "P0101" | "P0102" => Some(&[
            "Clean MAF sensor with MAF cleaner spray",
            "Inspect air filter and intake tract",
            "Check MAF sensor wiring and connector",
            "Test MAF sensor output with scan tool",
            "Replace MAF sensor if cleaning fails",
        ]),
        "P0110" => Some(&[
            "Test IAT sensor resistance (should vary with temp)",
            "Inspect wiring and connector for damage",
            "Check for corrosion at sensor connector",
            "Verify sensor reads ambient temp when cold",
            "Replace IAT sensor if out of spec",
        ]),
        "P0111" => Some(&[
            "Compare IAT reading to ambient temp with engine cold",
            "Monitor IAT reading while driving (should track conditions)",
            "Check for heat soak from exhaust or engine near sensor",
            "Inspect IAT sensor for contamination (oil, debris)",
            "Clean IAT sensor element with electronics cleaner",
            "Check for intake air leaks between filter and throttle",
            "Replace IAT sensor if reading is sluggish or erratic",
        ]),
        "P0112" => Some(&[
            "Check IAT sensor connector for water or corrosion",
            "Measure IAT sensor resistance (compare to spec chart)",
            "Inspect wiring for short to ground",
            "Disconnect sensor and check if code changes to P0113",
            "Replace IAT sensor if resistance is too low",
        ]),
        "P0113" => Some(&[
            "Check IAT sensor connector is fully seated",
            "Measure voltage at IAT sensor connector (key on)",
            "Check wiring continuity from sensor to ECM/PCM",
            "Inspect for broken or corroded terminals",
            "Replace IAT sensor if circuit tests good",
        ]),
        "P0120" => Some(&[
            "Check TPS voltage sweep (smooth 0.5-4.5V)",
            "Inspect TPS wiring for damage or chafing",
            "Check TPS connector for corrosion",
            "Verify throttle plate moves freely",
            "Replace TPS if signal is erratic",
        ]),
        "P0130" => Some(&[
            "Check O2 sensor heater operation",
            "Inspect O2 sensor wiring for damage",
            "Check for exhaust leaks near sensor",
            "Monitor O2 sensor switching rate (live data)",
            "Replace O2 sensor if lazy or non-responsive",
        ]),
        // Turbo codes
        "P0234" => Some(&[
            "Check wastegate actuator movement",
            "Inspect boost pressure sensor reading",
            "Test boost control solenoid",
            "Check for restricted exhaust (catalytic converter)",
            "Verify turbo control valve operation",
        ]),
        "P0299" => Some(&[
            "Pressure-test charge piping and intercooler",
            "Inspect wastegate for sticking or leaks",
            "Check turbocharger shaft play (radial/axial)",
            "Inspect air filter for restriction",
            "Test boost pressure solenoid operation",
        ]),
        "P1101" => Some(&[
            "Check for vacuum leaks (smoke test)",
            "Clean or replace MAF sensor",
            "Inspect air filter for restriction",
            "Check PCV valve and hoses",
            "Clean throttle body",
        ]),
        // VVT / camshaft timing
        "P0010" | "P0011" | "P0014" | "P0016" => Some(&[
            "Check engine oil level and condition",
            "Test camshaft position actuator solenoid",
            "Inspect timing chain for stretch (compare cam/crank signals)",
            "Check oil passages for blockage",
            "Inspect camshaft position sensor wiring",
        ]),
        // Idle low
        "P0506" => Some(&[
            "Check for vacuum leaks (smoke test)",
            "Clean throttle body and idle passages",
            "Test idle air control valve",
            "Check for intake manifold leaks",
            "Reset idle learn procedure after repairs",
        ]),
        // O2 heater
        "P0030" | "P0036" => Some(&[
            "Check O2 heater fuse",
            "Measure O2 heater resistance (should be 2-30 ohms)",
            "Inspect O2 sensor connector for corrosion",
            "Check wiring from PCM to O2 heater",
            "Replace O2 sensor if heater is open/short",
        ]),
        // O2 sensor voltage
        "P0131" | "P0132" | "P0133" | "P0134"
        | "P0137" | "P0138" => Some(&[
            "Inspect O2 sensor wiring for damage",
            "Check for exhaust leaks near sensor",
            "Monitor O2 sensor voltage (live data)",
            "Check fuel trim values for imbalance",
            "Replace O2 sensor if signal is stuck or lazy",
        ]),
        // Thermostat
        "P0597" | "P0598" | "P0599" => Some(&[
            "Test thermostat heater circuit resistance",
            "Inspect wiring to thermostat for damage",
            "Check connector for corrosion",
            "Verify coolant level is adequate",
            "Replace electronically controlled thermostat",
        ]),
        // Transmission extras
        "P0711" => Some(&[
            "Check transmission fluid level and condition",
            "Test trans temp sensor resistance vs. temperature",
            "Inspect sensor wiring and connector",
            "Compare trans temp to coolant temp (cold start)",
            "Replace sensor if out of specification",
        ]),
        "P0741" => Some(&[
            "Check transmission fluid level and condition",
            "Test TCC solenoid resistance",
            "Inspect TCC solenoid wiring",
            "Check for transmission valve body issues",
            "Monitor slip speed during TCC engagement",
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        let pids = get_correlated_pids("P0171");
        assert!(!pids.is_empty());
        assert!(pids.contains(&Pid::ShortFuelTrimBank1));
        assert!(pids.contains(&Pid::Maf));
    }

    #[test]
    fn test_prefix_fallback() {
        // P0150 has no exact match, but starts with "P01" so falls back to prefix group
        let pids = get_correlated_pids("P0150");
        assert!(!pids.is_empty());
        assert!(pids.contains(&Pid::ShortFuelTrimBank1));
    }

    #[test]
    fn test_unknown_code() {
        let pids = get_correlated_pids("X9999");
        assert!(pids.is_empty());
    }

    #[test]
    fn test_body_prefix_fallback() {
        let pids = get_correlated_pids("B9999");
        assert!(!pids.is_empty());
        assert!(pids.contains(&Pid::ControlModuleVoltage));
    }

    #[test]
    fn test_common_causes_known() {
        assert!(common_causes("P0171").is_some());
        assert!(common_causes("P0420").is_some());
        assert!(common_causes("P0700").is_some());
    }

    #[test]
    fn test_common_causes_unknown() {
        assert!(common_causes("P9999").is_none());
    }

    #[test]
    fn test_suggested_actions_known() {
        assert!(suggested_actions("P0171").is_some());
        assert!(suggested_actions("P0300").is_some());
    }

    #[test]
    fn test_suggested_actions_unknown() {
        assert!(suggested_actions("P9999").is_none());
    }

    #[test]
    fn test_short_names() {
        assert_eq!(short_name(Pid::ShortFuelTrimBank1), "STFT B1");
        assert_eq!(short_name(Pid::ThrottlePosition), "Throttle");
        assert_eq!(short_name(Pid::EngineRpm), "RPM");
        assert_eq!(short_name(Pid::CatalystTempB1S1), "Cat B1S1");
    }

    #[test]
    fn test_catalyst_bank2_match() {
        let pids = get_correlated_pids("P0430");
        assert!(pids.contains(&Pid::CatalystTempB2S1));
        assert!(pids.contains(&Pid::ShortFuelTrimBank2));
    }
}
