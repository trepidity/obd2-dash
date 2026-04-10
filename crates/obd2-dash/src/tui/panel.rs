use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders},
    Frame,
};

use crate::app::{AppState, PopupState};
use obd2_core::protocol::pid::Pid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    GaugesEngine,
    Temperatures,
    FuelSystem,
    SystemVehicle,
    Dtcs,
    FuelEconomy,
    Enhanced,
    O2Sensors,
}

#[allow(dead_code)]
pub struct PanelDef {
    pub kind: PanelKind,
    pub title: &'static str,
    pub index: usize,
}

#[allow(dead_code)]
pub struct GridRow {
    pub panels: Vec<PanelDef>,
    pub default_constraints: Vec<Constraint>,
    pub focused_expand_pct: u16,
}

impl GridRow {
    #[allow(dead_code)]
    pub fn constraints_for_focus(&self, focused_index: Option<usize>) -> Vec<Constraint> {
        let focused_local =
            focused_index.and_then(|fi| self.panels.iter().position(|p| p.index == fi));

        match focused_local {
            Some(pos) => {
                let n = self.panels.len();
                let expand = self.focused_expand_pct;
                let remaining = 100 - expand;
                let shrunk = remaining / (n as u16 - 1);
                (0..n)
                    .map(|i| {
                        if i == pos {
                            Constraint::Percentage(expand)
                        } else {
                            Constraint::Percentage(shrunk)
                        }
                    })
                    .collect()
            }
            None => self.default_constraints.clone(),
        }
    }
}

#[allow(dead_code)]
pub struct Grid {
    pub rows: Vec<GridRow>,
}

impl Grid {
    #[allow(dead_code)]
    pub fn full_layout() -> Grid {
        Grid {
            rows: vec![
                GridRow {
                    panels: vec![
                        PanelDef {
                            kind: PanelKind::GaugesEngine,
                            title: "GAUGES + ENGINE",
                            index: 0,
                        },
                        PanelDef {
                            kind: PanelKind::Temperatures,
                            title: "TEMPERATURES",
                            index: 1,
                        },
                    ],
                    default_constraints: vec![
                        Constraint::Percentage(60),
                        Constraint::Percentage(40),
                    ],
                    focused_expand_pct: 70,
                },
                GridRow {
                    panels: vec![
                        PanelDef {
                            kind: PanelKind::FuelSystem,
                            title: "FUEL SYSTEM",
                            index: 2,
                        },
                        PanelDef {
                            kind: PanelKind::SystemVehicle,
                            title: "SYSTEM / VEHICLE",
                            index: 3,
                        },
                        PanelDef {
                            kind: PanelKind::Dtcs,
                            title: "DTCs",
                            index: 4,
                        },
                    ],
                    default_constraints: vec![
                        Constraint::Percentage(35),
                        Constraint::Percentage(35),
                        Constraint::Percentage(30),
                    ],
                    focused_expand_pct: 50,
                },
                GridRow {
                    panels: vec![PanelDef {
                        kind: PanelKind::FuelEconomy,
                        title: "FUEL ECONOMY",
                        index: 5,
                    }],
                    default_constraints: vec![Constraint::Percentage(100)],
                    focused_expand_pct: 100,
                },
                GridRow {
                    panels: vec![
                        PanelDef {
                            kind: PanelKind::Enhanced,
                            title: "ENHANCED PIDs",
                            index: 6,
                        },
                        PanelDef {
                            kind: PanelKind::O2Sensors,
                            title: "O2 SENSORS",
                            index: 7,
                        },
                    ],
                    default_constraints: vec![
                        Constraint::Percentage(50),
                        Constraint::Percentage(50),
                    ],
                    focused_expand_pct: 65,
                },
            ],
        }
    }
}

// ─── Panel item types ────────────────────────────────────────────────────────

pub struct PanelItem {
    #[allow(dead_code)]
    pub label: String,
    pub detail: PanelItemDetail,
}

pub enum PanelItemDetail {
    Pid {
        pid_code: u8,
        current_value: Option<f64>,
        unit: &'static str,
        name: &'static str,
    },
    DerivedValue {
        label: &'static str,
        value: Option<f64>,
        unit: &'static str,
        description: &'static str,
    },
    Dtc {
        code: String,
        description: String,
        category: String,
        severity: Option<String>,
        notes: Option<String>,
    },
    VehicleField {
        field_name: &'static str,
        value: String,
    },
}

fn pid_value_from_vehicle(v: &crate::vehicle_data::VehicleData, pid: Pid) -> Option<f64> {
    let code = pid.0;
    match code {
        0x0C => v.rpm,
        0x0D => v.speed,
        0x04 => v.engine_load,
        0x11 => v.throttle_position,
        0x0B => v.intake_map,
        0x10 => v.maf,
        0x0A => v.fuel_pressure,
        0x05 => v.coolant_temp,
        0x5C => v.engine_oil_temp,
        0x0F => v.intake_air_temp,
        0x46 => v.ambient_air_temp,
        0x3C => v.catalyst_temp_b1s1,
        0x3D => v.catalyst_temp_b2s1,
        0x3E => v.catalyst_temp_b1s2,
        0x3F => v.catalyst_temp_b2s2,
        0x2F => v.fuel_tank_level,
        0x5E => v.engine_fuel_rate,
        0x06 => v.short_fuel_trim_b1,
        0x07 => v.long_fuel_trim_b1,
        0x08 => v.short_fuel_trim_b2,
        0x09 => v.long_fuel_trim_b2,
        0x33 => v.barometric_pressure,
        0x42 => v.control_module_voltage,
        0x0E => v.timing_advance,
        0x1F => v.run_time,
        0x21 => v.distance_with_mil,
        0x23 => v.fuel_rail_gauge_pressure,
        0x2C => v.commanded_egr,
        0x2E => v.commanded_evap_purge,
        0x31 => v.distance_since_dtc_clear,
        0x43 => v.absolute_load,
        0x44 => v.commanded_equiv_ratio,
        0x45 => v.relative_throttle_pos,
        0x47 => v.abs_throttle_pos_b,
        0x49 => v.accel_pedal_pos_d,
        0x4A => v.accel_pedal_pos_e,
        0x4C => v.commanded_throttle_actuator,
        0x59 => v.fuel_rail_abs_pressure,
        0x61 => v.demanded_torque,
        0x62 => v.actual_torque,
        0x63 => v.reference_torque,
        _ => None,
    }
}

fn pid_item(state: &AppState, pid: Pid) -> PanelItem {
    let reading_val = pid_value_from_vehicle(&state.domain.vehicle, pid);

    PanelItem {
        label: pid.name().to_string(),
        detail: PanelItemDetail::Pid {
            pid_code: pid.0,
            current_value: reading_val,
            unit: pid.unit(),
            name: pid.name(),
        },
    }
}

pub fn panel_items(kind: PanelKind, state: &AppState) -> Vec<PanelItem> {
    match kind {
        PanelKind::GaugesEngine => {
            let mut items = vec![
                pid_item(state, Pid::ENGINE_RPM),
                pid_item(state, Pid::VEHICLE_SPEED),
                pid_item(state, Pid::ENGINE_LOAD),
                pid_item(state, Pid::THROTTLE_POSITION),
            ];
            if state.domain.vehicle.intake_map.is_some() {
                items.push(pid_item(state, Pid::INTAKE_MAP));
            }
            if state.domain.vehicle.maf.is_some() {
                items.push(pid_item(state, Pid::MAF));
            }
            if state.domain.vehicle.fuel_pressure.is_some() {
                items.push(pid_item(state, Pid::FUEL_PRESSURE));
            }
            if state.domain.vehicle.boost_pressure.is_some() {
                items.push(PanelItem {
                    label: "Boost".to_string(),
                    detail: PanelItemDetail::DerivedValue {
                        label: "Boost Pressure",
                        value: state.domain.vehicle.boost_pressure,
                        unit: "kPa",
                        description: "Derived: MAP - Barometric Pressure",
                    },
                });
            }
            if state.domain.vehicle.oil_pressure.is_some() {
                items.push(pid_item(state, Pid(0xFD)));
            }
            if state.domain.vehicle.fuel_rail_gauge_pressure.is_some() {
                items.push(pid_item(state, Pid::FUEL_RAIL_GAUGE_PRESSURE));
            }
            if state.domain.vehicle.fuel_rail_abs_pressure.is_some() {
                items.push(pid_item(state, Pid::FUEL_RAIL_ABS_PRESSURE));
            }
            if state.domain.vehicle.timing_advance.is_some() {
                items.push(pid_item(state, Pid::TIMING_ADVANCE));
            }
            if state.domain.vehicle.demanded_torque.is_some() {
                items.push(pid_item(state, Pid::DEMANDED_TORQUE));
            }
            if state.domain.vehicle.actual_torque.is_some() {
                items.push(pid_item(state, Pid::ACTUAL_TORQUE));
            }
            if state.domain.vehicle.reference_torque.is_some() {
                items.push(pid_item(state, Pid::REFERENCE_TORQUE));
            }
            items
        }
        PanelKind::Temperatures => {
            vec![
                pid_item(state, Pid::COOLANT_TEMP),
                pid_item(state, Pid::ENGINE_OIL_TEMP),
                pid_item(state, Pid(0xFE)),
                pid_item(state, Pid::INTAKE_AIR_TEMP),
                pid_item(state, Pid::AMBIENT_AIR_TEMP),
                pid_item(state, Pid::CATALYST_TEMP_B1S1),
                pid_item(state, Pid::CATALYST_TEMP_B2S1),
                pid_item(state, Pid::CATALYST_TEMP_B1S2),
                pid_item(state, Pid::CATALYST_TEMP_B2S2),
            ]
        }
        PanelKind::FuelSystem => {
            let mut items = vec![pid_item(state, Pid::FUEL_TANK_LEVEL)];
            if state.domain.vehicle.engine_fuel_rate.is_some() {
                items.push(pid_item(state, Pid::ENGINE_FUEL_RATE));
            }
            items.push(pid_item(state, Pid::SHORT_FUEL_TRIM_B1));
            items.push(pid_item(state, Pid::LONG_FUEL_TRIM_B1));
            items.push(pid_item(state, Pid::SHORT_FUEL_TRIM_B2));
            items.push(pid_item(state, Pid::LONG_FUEL_TRIM_B2));
            if state.domain.vehicle.commanded_egr.is_some() {
                items.push(pid_item(state, Pid::COMMANDED_EGR));
            }
            if state.domain.vehicle.commanded_evap_purge.is_some() {
                items.push(pid_item(state, Pid::COMMANDED_EVAP_PURGE));
            }
            if state.domain.vehicle.commanded_equiv_ratio.is_some() {
                items.push(pid_item(state, Pid::COMMANDED_EQUIV_RATIO));
            }
            if state.domain.vehicle.absolute_load.is_some() {
                items.push(pid_item(state, Pid::ABSOLUTE_LOAD));
            }
            items
        }
        PanelKind::SystemVehicle => {
            let mut items = Vec::new();
            if state.domain.vehicle.battery_voltage.is_some() {
                items.push(PanelItem {
                    label: "Batt Voltage".to_string(),
                    detail: PanelItemDetail::VehicleField {
                        field_name: "Battery Voltage",
                        value: state
                            .domain
                            .vehicle
                            .battery_voltage
                            .map(|v| format!("{:.1}V", v))
                            .unwrap_or_default(),
                    },
                });
            }
            if state.domain.vehicle.control_module_voltage.is_some() {
                items.push(pid_item(state, Pid::CONTROL_MODULE_VOLTAGE));
            }
            if state.domain.vehicle.barometric_pressure.is_some() {
                items.push(pid_item(state, Pid::BAROMETRIC_PRESSURE));
            }
            if state.domain.vehicle.run_time.is_some() {
                items.push(pid_item(state, Pid::RUN_TIME));
            }
            if state.domain.vehicle.distance_with_mil.is_some() {
                items.push(pid_item(state, Pid::DISTANCE_WITH_MIL));
            }
            if state.domain.vehicle.distance_since_dtc_clear.is_some() {
                items.push(pid_item(state, Pid::DISTANCE_SINCE_CLEAR));
            }
            if let Some(info) = &state.domain.vehicle_info {
                items.push(PanelItem {
                    label: "VIN".to_string(),
                    detail: PanelItemDetail::VehicleField {
                        field_name: "VIN",
                        value: info.vin.clone(),
                    },
                });

                let mut engine_parts = Vec::new();
                if let Some(code) = &info.engine_family_code {
                    engine_parts.push(code.clone());
                }
                if let Some(disp) = info.displacement_l {
                    engine_parts.push(format!("{:.1}L", disp));
                }
                if let Some(cyl) = info.cylinders {
                    engine_parts.push(format!("{}cyl", cyl));
                }
                if !engine_parts.is_empty() {
                    items.push(PanelItem {
                        label: "Engine".to_string(),
                        detail: PanelItemDetail::VehicleField {
                            field_name: "Engine",
                            value: engine_parts.join(" "),
                        },
                    });
                }

                let mut detail_parts = Vec::new();
                if let Some(t) = &info.transmission_type {
                    detail_parts.push(t.clone());
                }
                if let Some(d) = &info.drive_type {
                    detail_parts.push(d.clone());
                }
                if let Some(f) = &info.fuel_type {
                    detail_parts.push(f.clone());
                }
                if !detail_parts.is_empty() {
                    items.push(PanelItem {
                        label: "Config".to_string(),
                        detail: PanelItemDetail::VehicleField {
                            field_name: "Configuration",
                            value: detail_parts.join("  "),
                        },
                    });
                }
            }
            items
        }
        PanelKind::Enhanced => {
            state
                .domain
                .enhanced_readings
                .iter()
                .map(|r| PanelItem {
                    label: r.name.clone(),
                    detail: PanelItemDetail::DerivedValue {
                        label: Box::leak(r.name.clone().into_boxed_str()),
                        value: Some(r.value),
                        unit: Box::leak(r.unit.clone().into_boxed_str()),
                        description: Box::leak(
                            format!("Enhanced DID 0x{:04X} [{}]", r.did, r.module)
                                .into_boxed_str(),
                        ),
                    },
                })
                .collect()
        }
        PanelKind::O2Sensors => {
            state
                .domain
                .o2_readings
                .iter()
                .map(|r| PanelItem {
                    label: format!("{} {}", r.sensor, r.test_name),
                    detail: PanelItemDetail::DerivedValue {
                        label: Box::leak(
                            format!("{} {}", r.sensor, r.test_name).into_boxed_str(),
                        ),
                        value: Some(r.value),
                        unit: Box::leak(r.unit.clone().into_boxed_str()),
                        description: Box::leak(
                            format!("O2 {} - {}", r.sensor, r.test_name).into_boxed_str(),
                        ),
                    },
                })
                .collect()
        }
        PanelKind::Dtcs => state
            .domain
            .stored_dtcs
            .iter()
            .map(|dtc| {
                let cat = format!("{:?}", dtc.category);
                PanelItem {
                    label: dtc.code.clone(),
                    detail: PanelItemDetail::Dtc {
                        code: dtc.code.clone(),
                        description: dtc.description.as_deref().unwrap_or("").to_string(),
                        category: cat,
                        severity: dtc.severity.as_ref().map(|s| format!("{:?}", s)),
                        notes: dtc.notes.clone(),
                    },
                }
            })
            .collect(),
        PanelKind::FuelEconomy => {
            let fe = &state.domain.fuel_economy;
            let mut items = Vec::new();

            // Gold standard items (4)
            let gold_source = fe
                .gold
                .as_ref()
                .map(|g| g.source.label().to_string())
                .unwrap_or_else(|| "Waiting...".to_string());
            items.push(PanelItem {
                label: "Gold Source".to_string(),
                detail: PanelItemDetail::VehicleField {
                    field_name: "ECU Source",
                    value: gold_source,
                },
            });

            let gold_instant = fe.gold.as_ref().map(|g| g.instant_mpg);
            items.push(PanelItem {
                label: "Gold Instant MPG".to_string(),
                detail: PanelItemDetail::DerivedValue {
                    label: "ECU Instant MPG",
                    value: gold_instant,
                    unit: "MPG",
                    description: "Gold standard instantaneous fuel economy from ECU",
                },
            });

            let gold_avg = fe.gold.as_ref().map(|g| g.avg_mpg);
            items.push(PanelItem {
                label: "Gold Avg MPG".to_string(),
                detail: PanelItemDetail::DerivedValue {
                    label: "ECU Average MPG",
                    value: gold_avg,
                    unit: "MPG",
                    description: "Gold standard trip average fuel economy",
                },
            });

            let gold_rate = fe.gold.as_ref().map(|g| g.fuel_rate_lph);
            items.push(PanelItem {
                label: "Gold Fuel Rate".to_string(),
                detail: PanelItemDetail::DerivedValue {
                    label: "ECU Fuel Rate",
                    value: gold_rate,
                    unit: "L/h",
                    description: "Gold standard fuel consumption rate",
                },
            });

            // Advanced items (4)
            items.push(PanelItem {
                label: "Adv Method".to_string(),
                detail: PanelItemDetail::VehicleField {
                    field_name: "Calc Method",
                    value: "Speed-Density".to_string(),
                },
            });

            let adv_instant = fe.advanced.as_ref().map(|a| a.instant_mpg);
            items.push(PanelItem {
                label: "Adv Instant MPG".to_string(),
                detail: PanelItemDetail::DerivedValue {
                    label: "Calc Instant MPG",
                    value: adv_instant,
                    unit: "MPG",
                    description: "Speed-density calculated instantaneous fuel economy",
                },
            });

            let adv_avg = fe.advanced.as_ref().map(|a| a.avg_mpg);
            items.push(PanelItem {
                label: "Adv Avg MPG".to_string(),
                detail: PanelItemDetail::DerivedValue {
                    label: "Calc Average MPG",
                    value: adv_avg,
                    unit: "MPG",
                    description: "Speed-density calculated trip average fuel economy",
                },
            });

            let adv_rate = fe.advanced.as_ref().map(|a| a.corrected_fuel_rate_lph);
            items.push(PanelItem {
                label: "Adv Fuel Rate".to_string(),
                detail: PanelItemDetail::DerivedValue {
                    label: "Calc Fuel Rate",
                    value: adv_rate,
                    unit: "L/h",
                    description: "Speed-density corrected fuel consumption rate",
                },
            });

            items
        }
    }
}

fn panel_kind_for_index(index: usize) -> Option<PanelKind> {
    match index {
        0 => Some(PanelKind::GaugesEngine),
        1 => Some(PanelKind::Temperatures),
        2 => Some(PanelKind::FuelSystem),
        3 => Some(PanelKind::SystemVehicle),
        4 => Some(PanelKind::Dtcs),
        5 => Some(PanelKind::FuelEconomy),
        6 => Some(PanelKind::Enhanced),
        7 => Some(PanelKind::O2Sensors),
        _ => None,
    }
}

pub fn panel_item_count(panel_index: usize, state: &AppState) -> usize {
    panel_kind_for_index(panel_index)
        .map(|kind| panel_items(kind, state).len())
        .unwrap_or(0)
}

pub fn build_popup(panel_index: usize, item_index: usize, state: &AppState) -> Option<PopupState> {
    let kind = panel_kind_for_index(panel_index)?;
    let items = panel_items(kind, state);
    let item = items.get(item_index)?;

    let (title, body) = match &item.detail {
        PanelItemDetail::Pid {
            pid_code,
            current_value,
            unit,
            name,
        } => {
            let mut lines = vec![
                format!("PID: 0x{:02X}", pid_code),
                format!("Name: {}", name),
                format!(
                    "Value: {}",
                    current_value
                        .map(|v| format!("{:.2} {}", v, unit))
                        .unwrap_or_else(|| "-- (no data)".to_string())
                ),
                String::new(),
            ];

            if let Some(threshold) = state.domain.thresholds_cache.get(pid_code) {
                lines.push("Thresholds:".to_string());
                if let Some(lc) = threshold.low_critical {
                    lines.push(format!("  Low Critical:  {:.1} {}", lc, threshold.unit));
                }
                if let Some(lw) = threshold.low_warning {
                    lines.push(format!("  Low Warning:   {:.1} {}", lw, threshold.unit));
                }
                if let Some(hw) = threshold.high_warning {
                    lines.push(format!("  High Warning:  {:.1} {}", hw, threshold.unit));
                }
                if let Some(hc) = threshold.high_critical {
                    lines.push(format!("  High Critical: {:.1} {}", hc, threshold.unit));
                }
                lines.push(format!(
                    "  Range: {:.1} - {:.1} {}",
                    threshold.min_value, threshold.max_value, threshold.unit
                ));
            } else {
                lines.push("No thresholds configured".to_string());
            }

            (name.to_string(), lines)
        }
        PanelItemDetail::DerivedValue {
            label,
            value,
            unit,
            description,
        } => {
            let lines = vec![
                format!(
                    "Value: {}",
                    value
                        .map(|v| format!("{:.2} {}", v, unit))
                        .unwrap_or_else(|| "-- (no data)".to_string())
                ),
                String::new(),
                format!("Source: {}", description),
            ];
            (label.to_string(), lines)
        }
        PanelItemDetail::Dtc {
            code,
            description,
            category,
            severity,
            notes,
        } => {
            let mut lines = vec![
                format!("Code: {}", code),
                format!("Category: {}", category),
            ];
            if let Some(sev) = severity {
                lines.push(format!("Severity: {}", sev));
            }
            lines.push(String::new());
            lines.push(description.clone());
            if let Some(n) = notes {
                lines.push(String::new());
                lines.push(format!("Notes: {}", n));
            }

            // Freeze-frame data (on-demand, may still be loading)
            if state.domain.freeze_frame_pending {
                lines.push(String::new());
                lines.push("Freeze-Frame: loading...".to_string());
            } else if let Some(ref ff) = state.domain.freeze_frame_data {
                if ff.dtc_code == *code {
                    lines.push(String::new());
                    lines.push("Freeze-Frame Snapshot:".to_string());
                    for (pid, value, unit) in &ff.readings {
                        lines.push(format!("  {}: {:.1} {}", pid.name(), value, unit));
                    }
                }
            }

            (code.clone(), lines)
        }
        PanelItemDetail::VehicleField { field_name, value } => {
            let lines = vec![format!("{}: {}", field_name, value)];
            (field_name.to_string(), lines)
        }
    };

    Some(PopupState { title, body })
}

// ─── Block / rendering ───────────────────────────────────────────────────────

#[allow(dead_code)]
pub fn panel_block(panel: &PanelDef, focused: bool, state: &AppState) -> Block<'static> {
    let (border_type, border_color) = if focused {
        (BorderType::Double, Color::Cyan)
    } else {
        let color = if panel.kind == PanelKind::Dtcs {
            dtc_border_color(state)
        } else {
            Color::DarkGray
        };
        (BorderType::Plain, color)
    };

    let title = if panel.kind == PanelKind::Dtcs && !state.domain.stored_dtcs.is_empty() {
        format!(" {} ({}) ", panel.title, state.domain.stored_dtcs.len())
    } else {
        format!(" {} ", panel.title)
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
}

#[allow(dead_code)]
fn dtc_border_color(state: &AppState) -> Color {
    if state.domain.stored_dtcs.is_empty() {
        Color::DarkGray
    } else if state.domain.stored_dtcs.len() >= 3 {
        Color::Red
    } else {
        Color::Yellow
    }
}

#[allow(dead_code)]
pub fn render_panel(
    frame: &mut Frame,
    area: Rect,
    panel: &PanelDef,
    focused: bool,
    state: &AppState,
) {
    let block = panel_block(panel, focused, state);
    let selected = if focused {
        state.panel_selections.get(&panel.index).copied()
    } else {
        None
    };
    match panel.kind {
        PanelKind::GaugesEngine => {
            super::ui::render_full_gauges_and_engine(frame, area, state, block, selected)
        }
        PanelKind::Temperatures => {
            super::ui::render_full_temperatures(frame, area, state, block, selected)
        }
        PanelKind::FuelSystem => {
            super::ui::render_full_fuel_system(frame, area, state, block, selected)
        }
        PanelKind::SystemVehicle => {
            super::ui::render_full_system_info(frame, area, state, block, selected)
        }
        PanelKind::Dtcs => super::ui::render_full_dtcs(frame, area, state, block, selected),
        PanelKind::FuelEconomy => {
            super::ui::render_full_fuel_economy(frame, area, state, block, selected)
        }
        PanelKind::Enhanced => {
            super::ui::render_full_enhanced(frame, area, state, block, selected)
        }
        PanelKind::O2Sensors => {
            super::ui::render_full_o2_sensors(frame, area, state, block, selected)
        }
    }
}

#[allow(dead_code)]
pub fn render_grid(frame: &mut Frame, rows_areas: &[Rect], state: &AppState) {
    let grid = Grid::full_layout();

    for (row, &row_area) in grid.rows.iter().zip(rows_areas.iter()) {
        let constraints = row.constraints_for_focus(state.focused_panel);
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(constraints)
            .split(row_area);

        for (panel, &col_area) in row.panels.iter().zip(cols.iter()) {
            let focused = state.focused_panel == Some(panel.index);
            render_panel(frame, col_area, panel, focused, state);
        }
    }
}
