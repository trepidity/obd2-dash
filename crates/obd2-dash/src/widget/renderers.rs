use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Gauge, Paragraph, Sparkline},
    Frame,
};

use super::WidgetKind;
use crate::app::AppState;
use crate::domain::{DiagnosticScanResult, DiagnosticScanScope, DtcService};
use crate::tui::ui;
use obd2_core::protocol::pid::Pid;

/// Render a widget of the given kind into the provided area.
pub fn render_widget(
    frame: &mut Frame,
    area: Rect,
    kind: WidgetKind,
    state: &AppState,
    focused: bool,
    selected_item: Option<usize>,
) {
    let block = make_widget_block(kind, focused, state);

    match kind {
        // Composite panels — delegate to existing renderers
        WidgetKind::GaugesAndEngine => {
            ui::render_full_gauges_and_engine(frame, area, state, block, selected_item);
        }
        WidgetKind::TemperaturesPanel => {
            ui::render_full_temperatures(frame, area, state, block, selected_item);
        }
        WidgetKind::FuelSystemPanel => {
            ui::render_full_fuel_system(frame, area, state, block, selected_item);
        }
        WidgetKind::SystemInfoPanel => {
            ui::render_full_system_info(frame, area, state, block, selected_item);
        }
        WidgetKind::DtcPanel => {
            ui::render_full_dtcs(frame, area, state, block, selected_item);
        }
        WidgetKind::FuelEconomyPanel => {
            ui::render_full_fuel_economy(frame, area, state, block, selected_item);
        }

        // Individual widgets
        WidgetKind::EngineRpmGauge => render_single_rpm(frame, area, state, block),
        WidgetKind::VehicleSpeedGauge => render_single_speed(frame, area, state, block),
        WidgetKind::EngineLoadGauge => render_single_load(frame, area, state, block),
        WidgetKind::ThrottleGauge => render_single_throttle(frame, area, state, block),
        WidgetKind::IntakeMapDisplay => render_intake_pressure(frame, area, state, block),
        WidgetKind::MafDisplay => render_single_value(
            frame,
            area,
            state,
            block,
            "MAF",
            state.domain.vehicle.maf,
            "g/s",
            0x10,
        ),
        WidgetKind::FuelPressureDisplay => render_single_value(
            frame,
            area,
            state,
            block,
            "Fuel P",
            state.domain.vehicle.fuel_pressure,
            "kPa",
            0x0A,
        ),
        WidgetKind::BoostPressureDisplay => render_single_boost(frame, area, state, block),
        WidgetKind::OilPressureDisplay => render_single_oil_pressure(frame, area, state, block),
        WidgetKind::FuelTankLevel => render_single_fuel_tank(frame, area, state, block),
        WidgetKind::EngineFuelRate => render_single_value(
            frame,
            area,
            state,
            block,
            "Fuel Rate",
            state.domain.vehicle.engine_fuel_rate.map(|r| r),
            "L/h",
            0x5E,
        ),
        WidgetKind::FuelTrimBank1 => render_single_fuel_trims(frame, area, state, block, 1),
        WidgetKind::FuelTrimBank2 => render_single_fuel_trims(frame, area, state, block, 2),
        WidgetKind::CoolantTemp => render_single_temp(
            frame,
            area,
            state,
            block,
            "Coolant",
            &state.domain.vehicle.coolant_temp,
            0x05,
        ),
        WidgetKind::OilTemp => render_single_temp(
            frame,
            area,
            state,
            block,
            "Oil",
            &state.domain.vehicle.engine_oil_temp,
            0x5C,
        ),
        WidgetKind::TransmissionTemp => render_single_temp(
            frame,
            area,
            state,
            block,
            "Trans",
            &state.domain.vehicle.transmission_temp,
            0xFE,
        ),
        WidgetKind::IntakeAirTemp => render_single_temp(
            frame,
            area,
            state,
            block,
            "Intake Air",
            &state.domain.vehicle.intake_air_temp,
            0x0F,
        ),
        WidgetKind::AmbientAirTemp => render_single_temp(
            frame,
            area,
            state,
            block,
            "Ambient",
            &state.domain.vehicle.ambient_air_temp,
            0x46,
        ),
        WidgetKind::CatalystTemps => render_single_catalyst_temps(frame, area, state, block),
        WidgetKind::RecordingStatus => render_recording_status(frame, area, state, block),
        WidgetKind::DrivingBehavior => render_driving_behavior(frame, area, state, block),
        WidgetKind::AlertsPanel => render_alerts_panel(frame, area, state, block),
        WidgetKind::DiagnosticScanPanel => render_diagnostic_scan_panel(frame, area, state, block),
        WidgetKind::EnhancedPidsPanel => {
            ui::render_full_enhanced(frame, area, state, block, selected_item);
        }
        WidgetKind::O2SensorsPanel => {
            ui::render_full_o2_sensors(frame, area, state, block, selected_item);
        }
        WidgetKind::ReadinessPanel => render_readiness_panel(frame, area, state, block),
    }
}

/// Build a standard block for a widget.
fn make_widget_block(kind: WidgetKind, focused: bool, state: &AppState) -> Block<'static> {
    let title = widget_title(kind, state);
    let (border_type, border_color) = if focused {
        (BorderType::Double, Color::Cyan)
    } else {
        let color = match kind {
            WidgetKind::DtcPanel if !state.domain.stored_dtcs.is_empty() => {
                if state.domain.stored_dtcs.len() >= 3 {
                    Color::Red
                } else {
                    Color::Yellow
                }
            }
            WidgetKind::AlertsPanel => match state.domain.worst_alert_level() {
                Some(obd2_db::models::AlertLevel::Critical) => Color::Red,
                Some(obd2_db::models::AlertLevel::Warning) => Color::Yellow,
                None if state.domain.last_error.is_some() => Color::Red,
                None => Color::DarkGray,
            },
            WidgetKind::DiagnosticScanPanel
                if state
                    .domain
                    .diagnostic_scan
                    .iter()
                    .any(|entry| matches!(entry.result, DiagnosticScanResult::Error(_))) =>
            {
                Color::Red
            }
            _ => Color::DarkGray,
        };
        (BorderType::Plain, color)
    };

    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
}

fn widget_title(kind: WidgetKind, state: &AppState) -> String {
    match kind {
        WidgetKind::GaugesAndEngine => " GAUGES + ENGINE ".to_string(),
        WidgetKind::TemperaturesPanel => " TEMPERATURES ".to_string(),
        WidgetKind::FuelSystemPanel => " FUEL SYSTEM ".to_string(),
        WidgetKind::SystemInfoPanel => " SYSTEM / VEHICLE ".to_string(),
        WidgetKind::DtcPanel => {
            if state.domain.stored_dtcs.is_empty() {
                " DTCs ".to_string()
            } else {
                format!(" DTCs ({}) ", state.domain.stored_dtcs.len())
            }
        }
        WidgetKind::FuelEconomyPanel => " FUEL ECONOMY ".to_string(),
        WidgetKind::EngineRpmGauge => " ENGINE RPM ".to_string(),
        WidgetKind::VehicleSpeedGauge => " VEHICLE SPEED ".to_string(),
        WidgetKind::EngineLoadGauge => " ENGINE LOAD ".to_string(),
        WidgetKind::ThrottleGauge => " THROTTLE ".to_string(),
        WidgetKind::IntakeMapDisplay => " INTAKE MAP ".to_string(),
        WidgetKind::MafDisplay => " MAF ".to_string(),
        WidgetKind::FuelPressureDisplay => " FUEL PRESSURE ".to_string(),
        WidgetKind::BoostPressureDisplay => " BOOST PRESSURE ".to_string(),
        WidgetKind::OilPressureDisplay => " OIL PRESSURE ".to_string(),
        WidgetKind::FuelTankLevel => " FUEL TANK ".to_string(),
        WidgetKind::EngineFuelRate => " FUEL RATE ".to_string(),
        WidgetKind::FuelTrimBank1 => " FUEL TRIM B1 ".to_string(),
        WidgetKind::FuelTrimBank2 => " FUEL TRIM B2 ".to_string(),
        WidgetKind::CoolantTemp => " COOLANT TEMP ".to_string(),
        WidgetKind::OilTemp => " OIL TEMP ".to_string(),
        WidgetKind::TransmissionTemp => " TRANS TEMP ".to_string(),
        WidgetKind::IntakeAirTemp => " INTAKE AIR TEMP ".to_string(),
        WidgetKind::AmbientAirTemp => " AMBIENT TEMP ".to_string(),
        WidgetKind::CatalystTemps => " CATALYST TEMPS ".to_string(),
        WidgetKind::RecordingStatus => " RECORDING ".to_string(),
        WidgetKind::DrivingBehavior => " DRIVING BEHAVIOR ".to_string(),
        WidgetKind::AlertsPanel => {
            let count = state.domain.active_alerts.len()
                + state.domain.stored_dtcs.len()
                + if state.domain.last_error.is_some() {
                    1
                } else {
                    0
                };
            if count == 0 {
                " ALERTS ".to_string()
            } else {
                format!(" ALERTS ({}) ", count)
            }
        }
        WidgetKind::DiagnosticScanPanel => {
            if state.domain.diagnostic_scan.is_empty() {
                " MODULE SCAN ".to_string()
            } else {
                let targets = count_diagnostic_scan_targets(state);
                format!(" MODULE SCAN ({targets}) ")
            }
        }
        WidgetKind::EnhancedPidsPanel => {
            let count = state.domain.enhanced_readings.len();
            if count == 0 {
                " ENHANCED PIDs ".to_string()
            } else {
                format!(" ENHANCED PIDs ({}) ", count)
            }
        }
        WidgetKind::O2SensorsPanel => {
            let count = state.domain.o2_readings.len();
            if count == 0 {
                " O2 SENSORS ".to_string()
            } else {
                format!(" O2 SENSORS ({}) ", count)
            }
        }
        WidgetKind::ReadinessPanel => {
            if let Some(ref status) = state.domain.readiness {
                let supported = status.monitors.iter().filter(|m| m.supported).count();
                let complete = status
                    .monitors
                    .iter()
                    .filter(|m| m.supported && m.complete)
                    .count();
                format!(" READINESS ({}/{}) ", complete, supported)
            } else {
                " READINESS ".to_string()
            }
        }
    }
}

// ─── Individual widget renderers ─────────────────────────────────────────────

fn render_single_rpm(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let rpm_val = state.domain.vehicle.rpm.map(|r| r).unwrap_or(0.0);
    let color =
        ui::threshold_color_for_pid(state, 0x0C, rpm_val, || ui::rpm_color_default(rpm_val));
    let max_rpm = state
        .domain
        .thresholds_cache
        .get(&0x0C)
        .and_then(|t| t.high_critical)
        .map(|c| c * 1.15)
        .unwrap_or(8000.0);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(2)])
        .split(inner);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(color))
        .label(format!("{:.0} rpm", rpm_val))
        .ratio((rpm_val / max_rpm).clamp(0.0, 1.0));
    frame.render_widget(gauge, chunks[0]);

    let hist: Vec<u64> = state
        .domain
        .vehicle
        .rpm_history
        .readings
        .iter()
        .copied()
        .collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&hist)
        .max(max_rpm as u64)
        .style(Style::default().fg(color));
    frame.render_widget(sparkline, chunks[1]);
}

fn render_single_speed(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let speed_unit_fallback = if state.domain.uses_us_customary_units() {
        "mph"
    } else {
        "km/h"
    };
    let (speed_val, speed_unit) = state
        .domain
        .display_speed()
        .unwrap_or((0.0, speed_unit_fallback));
    let color = ui::threshold_color_for_pid(state, 0x0D, speed_val, || Color::Blue);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(2)])
        .split(inner);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(color))
        .label(format!("{:.0} {}", speed_val, speed_unit))
        .ratio((speed_val / 260.0).clamp(0.0, 1.0));
    frame.render_widget(gauge, chunks[0]);

    let hist: Vec<u64> = state
        .domain
        .vehicle
        .speed_history
        .readings
        .iter()
        .copied()
        .collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&hist)
        .max(260)
        .style(Style::default().fg(color));
    frame.render_widget(sparkline, chunks[1]);
}

fn render_single_load(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let load_val = state.domain.vehicle.engine_load.map(|r| r).unwrap_or(0.0);
    let has_data = state.domain.vehicle.engine_load.is_some();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Value display
            Constraint::Length(1), // Segmented bar
            Constraint::Length(1), // Scale labels
            Constraint::Min(2),    // Sparkline history
        ])
        .split(inner);

    // Row 1: Large percentage value
    let val_color =
        ui::threshold_color_for_pid(state, 0x04, load_val, || load_zone_color(load_val));
    let val_text = if has_data {
        format!("{:.1}%", load_val)
    } else {
        "--%".to_string()
    };
    let val_line = Paragraph::new(Line::from(vec![Span::styled(
        val_text,
        Style::default().fg(val_color).add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center);
    frame.render_widget(val_line, chunks[0]);

    // Row 2: Segmented bar with color zones
    let bar_width = chunks[1].width.saturating_sub(2) as usize; // padding
    let filled = ((load_val / 100.0) * bar_width as f64).round() as usize;
    let mut spans = vec![Span::raw(" ")];
    for i in 0..bar_width {
        let segment_pct = (i as f64 / bar_width as f64) * 100.0;
        if i < filled {
            let color = load_zone_color(segment_pct);
            spans.push(Span::styled("\u{2588}", Style::default().fg(color))); // █
        } else {
            spans.push(Span::styled(
                "\u{2591}",
                Style::default().fg(Color::DarkGray),
            )); // ░
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), chunks[1]);

    // Row 3: Scale labels
    let scale_width = chunks[2].width as usize;
    let mut scale = format!(" 0{:^w$}100", "25      50      75", w = scale_width - 7);
    scale.truncate(scale_width);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            scale,
            Style::default().fg(Color::DarkGray),
        ))),
        chunks[2],
    );

    // Row 4: Sparkline history
    let hist: Vec<u64> = state
        .domain
        .vehicle
        .load_history
        .readings
        .iter()
        .copied()
        .collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&hist)
        .max(100)
        .style(Style::default().fg(val_color));
    frame.render_widget(sparkline, chunks[3]);
}

/// Color zones for engine load: green < 50%, yellow 50-75%, red > 75%.
fn load_zone_color(pct: f64) -> Color {
    if pct < 50.0 {
        Color::Green
    } else if pct < 75.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn render_single_throttle(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let val = state
        .domain
        .vehicle
        .throttle_position
        .map(|r| r)
        .unwrap_or(0.0);
    let color = ui::threshold_color_for_pid(state, 0x11, val, || Color::Cyan);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(2)])
        .split(inner);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(color))
        .label(format!("{:.1}%", val))
        .ratio((val / 100.0).clamp(0.0, 1.0));
    frame.render_widget(gauge, chunks[0]);

    let hist: Vec<u64> = state
        .domain
        .vehicle
        .throttle_history
        .readings
        .iter()
        .copied()
        .collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&hist)
        .max(100)
        .style(Style::default().fg(color));
    frame.render_widget(sparkline, chunks[1]);
}

#[allow(clippy::too_many_arguments)]
fn render_single_value(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    _label: &str,
    value: Option<f64>,
    unit: &str,
    pid_code: u8,
) {
    let val = value.unwrap_or(0.0);
    let color = ui::threshold_color_for_pid(state, pid_code, val, || Color::White);

    let text = if value.is_some() {
        let (display_val, display_unit) = state.domain.display_pid_value(Pid(pid_code), val);
        format!("{:.1} {}", display_val, display_unit)
    } else {
        let display_unit = state.domain.display_pid_value(Pid(pid_code), 0.0).1;
        let fallback_unit = if display_unit.is_empty() {
            unit
        } else {
            display_unit
        };
        format!("-- {}", fallback_unit)
    };

    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center)
    .block(block);

    frame.render_widget(paragraph, area);
}

fn render_intake_pressure(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let unit = state.domain.display_pressure_value(0.0).1;
    let mut lines = Vec::with_capacity(3);

    let actual_color = state
        .domain
        .vehicle
        .intake_map
        .map(|value| ui::threshold_color_for_pid(state, 0x0B, value, || Color::White))
        .unwrap_or(Color::DarkGray);
    lines.push(pressure_line(
        "Actual",
        state.domain.vehicle.intake_map,
        unit,
        actual_color,
        state,
        None,
    ));

    let desired_map = enhanced_reading(state, 0x1542);
    let desired_map_value = desired_map.and_then(|reading| reading.value);
    let desired_map_color = if desired_map_value.is_some() {
        Color::Cyan
    } else if desired_map.is_some_and(|reading| reading.last_error.is_some()) {
        Color::Red
    } else {
        Color::DarkGray
    };
    lines.push(pressure_line(
        "Desired",
        desired_map_value,
        unit,
        desired_map_color,
        state,
        desired_map.and_then(|reading| reading.confidence.as_deref()),
    ));

    let gm_baro = enhanced_reading(state, 0x1251).and_then(|reading| reading.value);
    let baro = gm_baro.or(state.domain.vehicle.barometric_pressure);
    let baro_color = state
        .domain
        .vehicle
        .barometric_pressure
        .or(gm_baro)
        .map(|value| ui::threshold_color_for_pid(state, 0x33, value, || Color::White))
        .unwrap_or(Color::DarkGray);
    lines.push(pressure_line(
        "Baro",
        baro,
        unit,
        baro_color,
        state,
        enhanced_reading(state, 0x1251).and_then(|reading| reading.confidence.as_deref()),
    ));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn pressure_line(
    label: &'static str,
    value: Option<f64>,
    fallback_unit: &'static str,
    color: Color,
    state: &AppState,
    metadata: Option<&str>,
) -> Line<'static> {
    let text = match value {
        Some(value) => {
            let (display_val, unit) = state.domain.display_pressure_value(value);
            match metadata {
                Some(metadata) => {
                    format!("{:<7} {:>6.1} {} {}", label, display_val, unit, metadata)
                }
                None => format!("{:<7} {:>6.1} {}", label, display_val, unit),
            }
        }
        None => match metadata {
            Some(metadata) => format!("{:<7}     -- {} {}", label, fallback_unit, metadata),
            None => format!("{:<7}     -- {}", label, fallback_unit),
        },
    };
    Line::from(Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    ))
}

fn enhanced_reading<'a>(
    state: &'a AppState,
    did: u16,
) -> Option<&'a crate::domain::EnhancedReading> {
    state
        .domain
        .enhanced_readings
        .iter()
        .find(|reading| reading.did == did)
}

fn render_single_boost(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let (text, color) = match state.domain.vehicle.boost_pressure {
        Some(val) => {
            let color = if val > 0.5 {
                Color::Green
            } else {
                Color::DarkGray
            };
            let (display_val, unit) = state.domain.display_pressure_value(val);
            (format!("{:.1} {}", display_val, unit), color)
        }
        None => {
            let unit = state.domain.display_pressure_value(0.0).1;
            (format!("-- {}", unit), Color::DarkGray)
        }
    };
    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center)
    .block(block);

    frame.render_widget(paragraph, area);
}

fn render_single_oil_pressure(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let val = state.domain.vehicle.oil_pressure;
    let color = match val {
        Some(v) => ui::threshold_color_for_pid(state, 0xFD, v, || {
            if v < 100.0 {
                Color::Red
            } else if v < 150.0 {
                Color::Yellow
            } else {
                Color::Green
            }
        }),
        None => Color::DarkGray,
    };

    let text = val
        .map(|v| {
            let (display_val, unit) = state.domain.display_pressure_value(v);
            format!("{:.1} {}", display_val, unit)
        })
        .unwrap_or_else(|| {
            let unit = state.domain.display_pressure_value(0.0).1;
            format!("-- {}", unit)
        });

    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center)
    .block(block);

    frame.render_widget(paragraph, area);
}

fn render_single_fuel_tank(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let val = state
        .domain
        .vehicle
        .fuel_tank_level
        .map(|r| r)
        .unwrap_or(0.0);
    let has_data = state.domain.vehicle.fuel_tank_level.is_some();

    let fuel_color = ui::threshold_color_for_pid(state, 0x2F, val, || fuel_zone_color(val));

    // Layout: vertical tank on the left, info on the right
    let h_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(6), Constraint::Min(8)])
        .split(inner);

    // Left side: vertical fuel tank gauge
    let tank_height = h_chunks[0].height as usize;
    let filled_rows = ((val / 100.0) * tank_height as f64).round() as usize;

    // Block characters for partial fill: ▁▂▃▄▅▆▇█
    let fill_chars = [
        ' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];

    let mut tank_lines: Vec<Line> = Vec::new();
    for row in 0..tank_height {
        let row_from_bottom = tank_height - 1 - row;
        let row_pct = (row_from_bottom as f64 / tank_height as f64) * 100.0;
        let color = fuel_zone_color(row_pct);

        let fill = if row_from_bottom < filled_rows {
            // Fully filled row
            Span::styled(" \u{2588}\u{2588}\u{2588} ", Style::default().fg(color))
        } else if row_from_bottom == filled_rows {
            // Partial fill row - use fractional block
            let frac = (val / 100.0) * tank_height as f64 - filled_rows as f64;
            let idx = (frac * 8.0).round() as usize;
            let ch = fill_chars[idx.min(8)];
            if ch == ' ' {
                Span::styled("      ", Style::default().fg(Color::DarkGray))
            } else {
                let s = format!(" {}{}{} ", ch, ch, ch);
                Span::styled(s, Style::default().fg(color))
            }
        } else {
            // Empty row
            Span::styled("      ", Style::default().fg(Color::DarkGray))
        };
        tank_lines.push(Line::from(fill));
    }
    frame.render_widget(Paragraph::new(tank_lines), h_chunks[0]);

    // Right side: value + scale labels
    let r_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Value
            Constraint::Min(1),    // Scale
        ])
        .split(h_chunks[1]);

    // Value display
    let val_text = if has_data {
        format!("{:.1}%", val)
    } else {
        "--%".to_string()
    };
    let val_para = Paragraph::new(vec![
        Line::from(Span::styled(
            val_text,
            Style::default().fg(fuel_color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            fuel_level_label(val),
            Style::default().fg(fuel_color),
        )),
    ]);
    frame.render_widget(val_para, r_chunks[0]);

    // Scale markers
    let scale_height = r_chunks[1].height as usize;
    let markers = ["F", "3/4", "1/2", "1/4", "E"];
    let mut scale_lines: Vec<Line> = Vec::new();
    for row in 0..scale_height {
        let frac = row as f64 / scale_height.max(1) as f64;
        let marker_idx = (frac * (markers.len() - 1) as f64).round() as usize;
        let label = if scale_height >= markers.len() {
            // Only show marker if this row is close enough to the target position
            let target_row = (marker_idx as f64 / (markers.len() - 1) as f64
                * (scale_height - 1) as f64)
                .round() as usize;
            if row == target_row {
                markers[marker_idx]
            } else {
                ""
            }
        } else if row < markers.len() {
            markers[row]
        } else {
            ""
        };
        let color = if label == "E" {
            Color::Red
        } else if label == "F" {
            Color::Green
        } else {
            Color::DarkGray
        };
        scale_lines.push(Line::from(Span::styled(
            format!(" {}", label),
            Style::default().fg(color),
        )));
    }
    frame.render_widget(Paragraph::new(scale_lines), r_chunks[1]);
}

fn fuel_zone_color(pct: f64) -> Color {
    if pct < 15.0 {
        Color::Red
    } else if pct < 25.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

fn fuel_level_label(pct: f64) -> &'static str {
    if pct < 5.0 {
        "CRITICAL"
    } else if pct < 25.0 {
        "Low"
    } else if pct < 50.0 {
        "Quarter+"
    } else if pct < 75.0 {
        "Half+"
    } else if pct < 90.0 {
        "Good"
    } else {
        "Full"
    }
}

fn render_single_fuel_trims(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    bank: u8,
) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let (short, long, short_code, long_code) = if bank == 1 {
        (
            &state.domain.vehicle.short_fuel_trim_b1,
            &state.domain.vehicle.long_fuel_trim_b1,
            0x06u8,
            0x07u8,
        )
    } else {
        (
            &state.domain.vehicle.short_fuel_trim_b2,
            &state.domain.vehicle.long_fuel_trim_b2,
            0x08u8,
            0x09u8,
        )
    };

    let mut lines = Vec::new();

    let short_line = make_trim_display("STFT", short, short_code, state);
    lines.push(short_line);

    let long_line = make_trim_display("LTFT", long, long_code, state);
    lines.push(long_line);

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn make_trim_display<'a>(
    label: &str,
    reading: &Option<f64>,
    pid_code: u8,
    state: &AppState,
) -> Line<'a> {
    if let Some(r) = reading {
        let color = ui::threshold_color_for_pid(state, pid_code, *r, || {
            if (*r).abs() > 10.0 {
                Color::Yellow
            } else {
                Color::Green
            }
        });
        let sign = if *r >= 0.0 { "+" } else { "" };
        Line::from(vec![
            Span::styled(
                format!(" {:<6}", label),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(format!("{}{:.1}%", sign, r), Style::default().fg(color)),
        ])
    } else {
        Line::from(vec![
            Span::styled(
                format!(" {:<6}", label),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled("--", Style::default().fg(Color::DarkGray)),
        ])
    }
}

fn render_single_temp(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    _label: &str,
    reading: &Option<f64>,
    pid_code: u8,
) {
    let (text, color) = if let Some(r) = reading {
        let (val, unit) = state.domain.display_temp_value(*r);
        let c = ui::threshold_color_for_pid(state, pid_code, *r, || ui::temp_color_default(*r));
        (format!("{:.1}{}", val, unit), c)
    } else {
        ("--".to_string(), Color::DarkGray)
    };

    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center)
    .block(block);

    frame.render_widget(paragraph, area);
}

fn render_single_catalyst_temps(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sensors = [
        ("B1S1", &state.domain.vehicle.catalyst_temp_b1s1, 0x3Cu8),
        ("B2S1", &state.domain.vehicle.catalyst_temp_b2s1, 0x3D),
        ("B1S2", &state.domain.vehicle.catalyst_temp_b1s2, 0x3E),
        ("B2S2", &state.domain.vehicle.catalyst_temp_b2s2, 0x3F),
    ];

    let lines: Vec<Line> = sensors
        .iter()
        .map(|(label, reading, pid_code)| {
            if let Some(r) = reading {
                let (val, unit) = state.domain.display_temp_value(*r);
                let color = ui::threshold_color_for_pid(state, *pid_code, *r, || {
                    ui::temp_color_default(*r)
                });
                Line::from(vec![
                    Span::styled(
                        format!(" {:<6}", label),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(format!("{:.1}{}", val, unit), Style::default().fg(color)),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {:<6}", label),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled("--", Style::default().fg(Color::DarkGray)),
                ])
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_driving_behavior(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let driving = &state.domain.driving;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Smoothness gauge
            Constraint::Min(2),    // Accel sparkline
            Constraint::Length(1), // Current accel value
            Constraint::Length(1), // Event counters
        ])
        .split(inner);

    // Row 1: Smoothness gauge
    let score = driving.smoothness_score;
    let gauge_color = if score >= 80.0 {
        Color::Green
    } else if score >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(format!(" Smoothness: {} ", driving.smoothness_label()))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(gauge_color))
        .label(format!("{:.0}", score))
        .ratio((score / 100.0).clamp(0.0, 1.0));
    frame.render_widget(gauge, chunks[0]);

    // Row 2: Acceleration sparkline (absolute values)
    let hist = driving.accel_display_history();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&hist)
        .max(500) // 5.0 m/s² * 100
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(sparkline, chunks[1]);

    // Row 3: Current acceleration value
    let accel = driving.current_accel;
    let accel_color = if accel.abs() < 1.0 {
        Color::Green
    } else if accel.abs() < 2.0 {
        Color::Yellow
    } else {
        Color::Red
    };
    let sign = if accel >= 0.0 { "+" } else { "" };
    let accel_line = Paragraph::new(Line::from(vec![
        Span::styled(" Accel: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}{:.1} m/s\u{00B2}", sign, accel),
            Style::default()
                .fg(accel_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(accel_line, chunks[2]);

    // Row 4: Event counters
    let events_line = Paragraph::new(Line::from(vec![
        Span::styled(" Hard Brakes: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", driving.hard_brake_count),
            Style::default().fg(if driving.hard_brake_count > 0 {
                Color::Red
            } else {
                Color::Green
            }),
        ),
        Span::styled("  Jackrabbits: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", driving.jackrabbit_count),
            Style::default().fg(if driving.jackrabbit_count > 0 {
                Color::Yellow
            } else {
                Color::Green
            }),
        ),
    ]));
    frame.render_widget(events_line, chunks[3]);
}

fn render_recording_status(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    match &state.domain.recording {
        crate::recording::RecordingState::Idle => {
            lines.push(Line::from(Span::styled(
                " Idle",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                " Press 'r' to start recording",
                Style::default().fg(Color::DarkGray),
            )));
        }
        crate::recording::RecordingState::Recording { start_instant, .. } => {
            let elapsed = start_instant.elapsed();
            let secs = elapsed.as_secs();
            let mins = secs / 60;
            let hours = mins / 60;
            lines.push(Line::from(vec![
                Span::styled(
                    " REC ",
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Red)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("  {:02}:{:02}:{:02}", hours, mins % 60, secs % 60),
                    Style::default().fg(Color::Red),
                ),
            ]));
            lines.push(Line::from(Span::styled(
                " Press 'r' to stop",
                Style::default().fg(Color::DarkGray),
            )));
        }
        crate::recording::RecordingState::Replaying(controller) => {
            let speed_label = controller.speed_label();
            lines.push(Line::from(vec![Span::styled(
                format!(" REPLAY {} ", speed_label),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            )]));
            let progress = controller.progress_text();
            lines.push(Line::from(Span::styled(
                format!(" {}", progress),
                Style::default().fg(Color::Magenta),
            )));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_alerts_panel(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.domain.alert_history.is_empty() {
        let paragraph = Paragraph::new(Line::from(Span::styled(
            " No active alerts",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(paragraph, inner);
        return;
    }

    let body_height = inner.height as usize;
    let total = state.domain.alert_history.len();

    // Show the most recent entries that fit in the visible area
    let start = total.saturating_sub(body_height);
    let visible: Vec<Line> = state
        .domain
        .alert_history
        .iter()
        .skip(start)
        .map(|msg| {
            let color = if msg.starts_with("[CRIT]") {
                Color::Red
            } else if msg.starts_with("[WARN]") {
                Color::Yellow
            } else {
                Color::Red // errors
            };
            Line::from(Span::styled(
                format!(" {}", msg),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let paragraph = Paragraph::new(visible);
    frame.render_widget(paragraph, inner);
}

fn render_diagnostic_scan_panel(frame: &mut Frame, area: Rect, state: &AppState, block: Block) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if state.domain.diagnostic_scan.is_empty() {
        let paragraph = Paragraph::new(vec![
            Line::from(Span::styled(
                " No DTC scan yet",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                " Console messages require decoded Class 2 frames",
                Style::default().fg(Color::DarkGray),
            )),
        ]);
        frame.render_widget(paragraph, inner);
        return;
    }

    let mut lines = Vec::new();
    let entries = &state.domain.diagnostic_scan;
    let mut idx = 0;
    while idx < entries.len() {
        let scope = entries[idx].scope.clone();
        let mut grouped = Vec::new();

        while idx < entries.len() && entries[idx].scope == scope {
            grouped.push((entries[idx].service, &entries[idx].result));
            idx += 1;
        }

        let mut spans = vec![Span::styled(
            format!(" {:<9}", diagnostic_scan_scope_label(&scope)),
            Style::default().fg(Color::White),
        )];
        for (service, result) in grouped {
            push_scan_result_span(&mut spans, service, Some(result));
        }
        lines.push(Line::from(spans));
    }

    let body_height = inner.height as usize;
    let start = lines.len().saturating_sub(body_height);
    let visible = lines.into_iter().skip(start).collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(visible), inner);
}

fn push_scan_result_span<'a>(
    spans: &mut Vec<Span<'a>>,
    service: DtcService,
    result: Option<&DiagnosticScanResult>,
) {
    let (text, style) = match result {
        Some(result) => (
            diagnostic_scan_result_label(result),
            Style::default().fg(diagnostic_scan_result_color(result)),
        ),
        None => ("--".to_string(), Style::default().fg(Color::DarkGray)),
    };

    spans.push(Span::raw(" "));
    spans.push(Span::styled(
        format!("{:<4}:{:<7}", service.label(), text),
        style,
    ));
}

fn diagnostic_scan_scope_label(scope: &DiagnosticScanScope) -> String {
    let raw = match scope {
        DiagnosticScanScope::Broadcast => "broadcast",
        DiagnosticScanScope::Module(module) => module.as_str(),
    };
    if raw.len() <= 9 {
        raw.to_string()
    } else {
        raw[..9].to_string()
    }
}

fn diagnostic_scan_result_label(result: &DiagnosticScanResult) -> String {
    match result {
        DiagnosticScanResult::Codes(count) => format!("{count} dtc"),
        DiagnosticScanResult::Empty => "empty".to_string(),
        DiagnosticScanResult::NoData => "no data".to_string(),
        DiagnosticScanResult::Unsupported(_) => "unsup".to_string(),
        DiagnosticScanResult::Error(_) => "error".to_string(),
    }
}

fn diagnostic_scan_result_color(result: &DiagnosticScanResult) -> Color {
    match result {
        DiagnosticScanResult::Codes(_) => Color::Red,
        DiagnosticScanResult::Empty => Color::Green,
        DiagnosticScanResult::NoData => Color::DarkGray,
        DiagnosticScanResult::Unsupported(_) => Color::Yellow,
        DiagnosticScanResult::Error(_) => Color::Red,
    }
}

fn count_diagnostic_scan_targets(state: &AppState) -> usize {
    let mut count = 0;
    let mut last_scope: Option<&DiagnosticScanScope> = None;
    for entry in &state.domain.diagnostic_scan {
        if last_scope != Some(&entry.scope) {
            count += 1;
            last_scope = Some(&entry.scope);
        }
    }
    count
}

fn render_readiness_panel(frame: &mut Frame, area: Rect, state: &AppState, block: Block<'_>) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    match &state.domain.readiness {
        Some(status) => {
            let mil_style = if status.mil_on {
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Green)
            };
            let mil_text = if status.mil_on { "ON" } else { "OFF" };
            lines.push(Line::from(vec![
                Span::raw("  MIL: "),
                Span::styled(mil_text, mil_style),
                Span::raw(format!("  DTCs: {}  ", status.dtc_count)),
                Span::raw(state.domain.readiness_ignition_label(status)),
            ]));
            lines.push(Line::from(""));

            let supported: Vec<_> = status.monitors.iter().filter(|m| m.supported).collect();
            let complete_count = supported.iter().filter(|m| m.complete).count();
            lines.push(Line::from(format!(
                "  Monitors: {}/{} complete",
                complete_count,
                supported.len()
            )));
            lines.push(Line::from(""));

            for monitor in &status.monitors {
                if !monitor.supported {
                    continue;
                }
                let (icon, style) = if monitor.complete {
                    ("OK", Style::default().fg(Color::Green))
                } else {
                    ("--", Style::default().fg(Color::Yellow))
                };
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(format!("{:>2}", icon), style),
                    Span::raw(format!("  {}", monitor.name)),
                ]));
            }
        }
        None => {
            lines.push(Line::from("  No readiness data"));
            lines.push(Line::from("  (waiting for connection)"));
        }
    }

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
