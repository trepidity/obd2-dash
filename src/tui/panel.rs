use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, BorderType, Borders},
    Frame,
};

use crate::app::{AppState, PopupState};
use crate::obd2::Pid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKind {
    GaugesEngine,
    Temperatures,
    FuelSystem,
    SystemVehicle,
    Dtcs,
    FuelEconomy,
}

pub struct PanelDef {
    pub kind: PanelKind,
    pub title: &'static str,
    pub index: usize,
}

pub struct GridRow {
    pub panels: Vec<PanelDef>,
    pub default_constraints: Vec<Constraint>,
    pub focused_expand_pct: u16,
}

impl GridRow {
    pub fn constraints_for_focus(&self, focused_index: Option<usize>) -> Vec<Constraint> {
        let focused_local = focused_index.and_then(|fi| {
            self.panels.iter().position(|p| p.index == fi)
        });

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

pub struct Grid {
    pub rows: Vec<GridRow>,
}

impl Grid {
    pub fn full_layout() -> Grid {
        Grid {
            rows: vec![
                GridRow {
                    panels: vec![
                        PanelDef { kind: PanelKind::GaugesEngine, title: "GAUGES + ENGINE", index: 0 },
                        PanelDef { kind: PanelKind::Temperatures, title: "TEMPERATURES", index: 1 },
                    ],
                    default_constraints: vec![
                        Constraint::Percentage(60),
                        Constraint::Percentage(40),
                    ],
                    focused_expand_pct: 70,
                },
                GridRow {
                    panels: vec![
                        PanelDef { kind: PanelKind::FuelSystem, title: "FUEL SYSTEM", index: 2 },
                        PanelDef { kind: PanelKind::SystemVehicle, title: "SYSTEM / VEHICLE", index: 3 },
                        PanelDef { kind: PanelKind::Dtcs, title: "DTCs", index: 4 },
                    ],
                    default_constraints: vec![
                        Constraint::Percentage(35),
                        Constraint::Percentage(35),
                        Constraint::Percentage(30),
                    ],
                    focused_expand_pct: 50,
                },
                GridRow {
                    panels: vec![
                        PanelDef { kind: PanelKind::FuelEconomy, title: "FUEL ECONOMY", index: 5 },
                    ],
                    default_constraints: vec![
                        Constraint::Percentage(100),
                    ],
                    focused_expand_pct: 100,
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
    Pid { pid_code: u8, current_value: Option<f64>, unit: &'static str, name: &'static str },
    DerivedValue { label: &'static str, value: Option<f64>, unit: &'static str, description: &'static str },
    Dtc { code: String, description: String, category: String },
    VehicleField { field_name: &'static str, value: String },
}

fn pid_item(state: &AppState, pid: Pid) -> PanelItem {
    let reading_val = match pid {
        Pid::EngineRpm => state.vehicle.rpm.as_ref().map(|r| r.value),
        Pid::VehicleSpeed => state.vehicle.speed.as_ref().map(|r| r.value),
        Pid::EngineLoad => state.vehicle.engine_load.as_ref().map(|r| r.value),
        Pid::ThrottlePosition => state.vehicle.throttle_position.as_ref().map(|r| r.value),
        Pid::IntakeMap => state.vehicle.intake_map.as_ref().map(|r| r.value),
        Pid::Maf => state.vehicle.maf.as_ref().map(|r| r.value),
        Pid::FuelPressure => state.vehicle.fuel_pressure.as_ref().map(|r| r.value),
        Pid::OilPressure => state.vehicle.oil_pressure.as_ref().map(|r| r.value),
        Pid::CoolantTemp => state.vehicle.coolant_temp.as_ref().map(|r| r.value),
        Pid::EngineOilTemp => state.vehicle.engine_oil_temp.as_ref().map(|r| r.value),
        Pid::TransmissionTemp => state.vehicle.transmission_temp.as_ref().map(|r| r.value),
        Pid::IntakeAirTemp => state.vehicle.intake_air_temp.as_ref().map(|r| r.value),
        Pid::AmbientAirTemp => state.vehicle.ambient_air_temp.as_ref().map(|r| r.value),
        Pid::CatalystTempB1S1 => state.vehicle.catalyst_temp_b1s1.as_ref().map(|r| r.value),
        Pid::CatalystTempB2S1 => state.vehicle.catalyst_temp_b2s1.as_ref().map(|r| r.value),
        Pid::CatalystTempB1S2 => state.vehicle.catalyst_temp_b1s2.as_ref().map(|r| r.value),
        Pid::CatalystTempB2S2 => state.vehicle.catalyst_temp_b2s2.as_ref().map(|r| r.value),
        Pid::FuelTankLevel => state.vehicle.fuel_tank_level.as_ref().map(|r| r.value),
        Pid::EngineFuelRate => state.vehicle.engine_fuel_rate.as_ref().map(|r| r.value),
        Pid::ShortFuelTrimBank1 => state.vehicle.short_fuel_trim_b1.as_ref().map(|r| r.value),
        Pid::LongFuelTrimBank1 => state.vehicle.long_fuel_trim_b1.as_ref().map(|r| r.value),
        Pid::ShortFuelTrimBank2 => state.vehicle.short_fuel_trim_b2.as_ref().map(|r| r.value),
        Pid::LongFuelTrimBank2 => state.vehicle.long_fuel_trim_b2.as_ref().map(|r| r.value),
        Pid::BarometricPressure => state.vehicle.barometric_pressure.as_ref().map(|r| r.value),
        Pid::ControlModuleVoltage => state.vehicle.control_module_voltage.as_ref().map(|r| r.value),
    };

    PanelItem {
        label: pid.name().to_string(),
        detail: PanelItemDetail::Pid {
            pid_code: pid.code(),
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
                pid_item(state, Pid::EngineRpm),
                pid_item(state, Pid::VehicleSpeed),
                pid_item(state, Pid::EngineLoad),
                pid_item(state, Pid::ThrottlePosition),
            ];
            if state.vehicle.intake_map.is_some() {
                items.push(pid_item(state, Pid::IntakeMap));
            }
            if state.vehicle.maf.is_some() {
                items.push(pid_item(state, Pid::Maf));
            }
            if state.vehicle.fuel_pressure.is_some() {
                items.push(pid_item(state, Pid::FuelPressure));
            }
            if state.vehicle.boost_pressure.is_some() {
                items.push(PanelItem {
                    label: "Boost".to_string(),
                    detail: PanelItemDetail::DerivedValue {
                        label: "Boost Pressure",
                        value: state.vehicle.boost_pressure,
                        unit: "kPa",
                        description: "Derived: MAP - Barometric Pressure",
                    },
                });
            }
            if state.vehicle.oil_pressure.is_some() {
                items.push(pid_item(state, Pid::OilPressure));
            }
            items
        }
        PanelKind::Temperatures => {
            vec![
                pid_item(state, Pid::CoolantTemp),
                pid_item(state, Pid::EngineOilTemp),
                pid_item(state, Pid::TransmissionTemp),
                pid_item(state, Pid::IntakeAirTemp),
                pid_item(state, Pid::AmbientAirTemp),
                pid_item(state, Pid::CatalystTempB1S1),
                pid_item(state, Pid::CatalystTempB2S1),
                pid_item(state, Pid::CatalystTempB1S2),
                pid_item(state, Pid::CatalystTempB2S2),
            ]
        }
        PanelKind::FuelSystem => {
            let mut items = vec![
                pid_item(state, Pid::FuelTankLevel),
            ];
            if state.vehicle.engine_fuel_rate.is_some() {
                items.push(pid_item(state, Pid::EngineFuelRate));
            }
            items.push(pid_item(state, Pid::ShortFuelTrimBank1));
            items.push(pid_item(state, Pid::LongFuelTrimBank1));
            items.push(pid_item(state, Pid::ShortFuelTrimBank2));
            items.push(pid_item(state, Pid::LongFuelTrimBank2));
            items
        }
        PanelKind::SystemVehicle => {
            let mut items = Vec::new();
            if state.vehicle.battery_voltage.is_some() {
                items.push(PanelItem {
                    label: "Batt Voltage".to_string(),
                    detail: PanelItemDetail::VehicleField {
                        field_name: "Battery Voltage",
                        value: state.vehicle.battery_voltage
                            .map(|v| format!("{:.1}V", v))
                            .unwrap_or_default(),
                    },
                });
            }
            if state.vehicle.control_module_voltage.is_some() {
                items.push(pid_item(state, Pid::ControlModuleVoltage));
            }
            if state.vehicle.barometric_pressure.is_some() {
                items.push(pid_item(state, Pid::BarometricPressure));
            }
            if let Some(info) = &state.vehicle_info {
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
        PanelKind::Dtcs => {
            state.stored_dtcs.iter().map(|dtc| {
                let cat = format!("{:?}", dtc.category);
                PanelItem {
                    label: dtc.code.clone(),
                    detail: PanelItemDetail::Dtc {
                        code: dtc.code.clone(),
                        description: dtc.description.to_string(),
                        category: cat,
                    },
                }
            }).collect()
        }
        PanelKind::FuelEconomy => {
            let fe = &state.fuel_economy;
            let mut items = Vec::new();

            // Gold standard items (4)
            let gold_source = fe.gold.as_ref()
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
        PanelItemDetail::Pid { pid_code, current_value, unit, name } => {
            let mut lines = vec![
                format!("PID: 0x{:02X}", pid_code),
                format!("Name: {}", name),
                format!("Value: {}", current_value
                    .map(|v| format!("{:.2} {}", v, unit))
                    .unwrap_or_else(|| "-- (no data)".to_string())),
                String::new(),
            ];

            if let Some(threshold) = state.thresholds_cache.get(pid_code) {
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
                lines.push(format!("  Range: {:.1} - {:.1} {}", threshold.min_value, threshold.max_value, threshold.unit));
            } else {
                lines.push("No thresholds configured".to_string());
            }

            (name.to_string(), lines)
        }
        PanelItemDetail::DerivedValue { label, value, unit, description } => {
            let lines = vec![
                format!("Value: {}", value
                    .map(|v| format!("{:.2} {}", v, unit))
                    .unwrap_or_else(|| "-- (no data)".to_string())),
                String::new(),
                format!("Source: {}", description),
            ];
            (label.to_string(), lines)
        }
        PanelItemDetail::Dtc { code, description, category } => {
            let mut lines = vec![
                format!("Code: {}", code),
                format!("Category: {}", category),
                String::new(),
                description.clone(),
            ];

            // Build diagnostic context and run local analysis
            let context = crate::diagnostics::correlation::build_diagnostic_context(
                state, code, description, category,
            );
            let provider = crate::diagnostics::provider::LocalDiagnosticProvider;
            if let Some(result) = provider.diagnose_sync(&context) {
                lines.extend(result.to_popup_lines());
            }

            (code.clone(), lines)
        }
        PanelItemDetail::VehicleField { field_name, value } => {
            let lines = vec![
                format!("{}: {}", field_name, value),
            ];
            (field_name.to_string(), lines)
        }
    };

    Some(PopupState { title, body })
}

// ─── Block / rendering ───────────────────────────────────────────────────────

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

    let title = if panel.kind == PanelKind::Dtcs && !state.stored_dtcs.is_empty() {
        format!(" {} ({}) ", panel.title, state.stored_dtcs.len())
    } else {
        format!(" {} ", panel.title)
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
}

fn dtc_border_color(state: &AppState) -> Color {
    if state.stored_dtcs.is_empty() {
        Color::DarkGray
    } else if state.stored_dtcs.len() >= 3 {
        Color::Red
    } else {
        Color::Yellow
    }
}

pub fn render_panel(frame: &mut Frame, area: Rect, panel: &PanelDef, focused: bool, state: &AppState) {
    let block = panel_block(panel, focused, state);
    let selected = if focused {
        state.panel_selections.get(&panel.index).copied()
    } else {
        None
    };
    match panel.kind {
        PanelKind::GaugesEngine => super::ui::render_full_gauges_and_engine(frame, area, state, block, selected),
        PanelKind::Temperatures => super::ui::render_full_temperatures(frame, area, state, block, selected),
        PanelKind::FuelSystem => super::ui::render_full_fuel_system(frame, area, state, block, selected),
        PanelKind::SystemVehicle => super::ui::render_full_system_info(frame, area, state, block, selected),
        PanelKind::Dtcs => super::ui::render_full_dtcs(frame, area, state, block, selected),
        PanelKind::FuelEconomy => super::ui::render_full_fuel_economy(frame, area, state, block, selected),
    }
}

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
