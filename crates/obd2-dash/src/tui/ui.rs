use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Gauge, Paragraph, Sparkline},
    Frame,
};

use crate::app::{AppState, DashboardLayout, PopupState, ScanMode};
use obd2_core::ConnectionState;
use obd2_db::models::AlertLevel;
use crate::debug_log::LogBuffer;
use obd2_core::obd2::dtc::DtcCategory;
use obd2_core::PidReading;
use obd2_core::RecordingState;
use crate::widget::config::RowHeight;
use crate::widget::edit_mode::EditPhase;
use crate::widget::renderers;

pub fn render(frame: &mut Frame, state: &AppState, log_buffer: &LogBuffer) {
    // Full-screen debug log takeover
    if state.show_debug_log {
        render_debug_log(frame, state, log_buffer);
        return;
    }

    match state.layout {
        DashboardLayout::Compact => render_compact(frame, state),
        DashboardLayout::Full => render_full(frame, state),
    }

    // Edit mode overlay (rendered on top of dashboard)
    if let Some(edit) = &state.edit_mode {
        render_edit_overlay(frame, edit);
    }

    // Popup overlay (rendered last, on top of everything)
    if let Some(popup) = &state.popup {
        render_popup(frame, popup);
    }

    // Session picker overlay
    if state.show_session_picker {
        render_session_picker(frame, state);
    }

    // Device picker overlay
    if state.scan_mode != ScanMode::Idle {
        render_device_picker(frame, state);
    }
}

// ─── Debug log viewer ────────────────────────────────────────────────────────

fn render_debug_log(frame: &mut Frame, state: &AppState, log_buffer: &LogBuffer) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Header
            Constraint::Min(1),   // Log body
            Constraint::Length(1), // Footer
        ])
        .split(area);

    // Header
    let entry_count = log_buffer.len();
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " DEBUG LOG ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  {} entries", entry_count),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            if state.debug_log_scroll > 0 {
                format!("  [scroll: +{}]", state.debug_log_scroll)
            } else {
                "  [auto-scroll]".to_string()
            },
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(header, chunks[0]);

    // Log body
    let body_height = chunks[1].height as usize;
    let lines_data = log_buffer.lines();
    let total = lines_data.len();

    // scroll=0 means bottom (newest), higher values scroll up (older)
    let scroll = state.debug_log_scroll.min(total.saturating_sub(body_height));
    let end = total.saturating_sub(scroll);
    let start = end.saturating_sub(body_height);

    let visible_lines: Vec<Line> = lines_data[start..end]
        .iter()
        .map(|s| {
            // Color-code by level
            let color = if s.contains(" ERROR ") {
                Color::Red
            } else if s.contains(" WARN ") {
                Color::Yellow
            } else if s.contains("  INFO ") {
                Color::Green
            } else if s.contains(" DEBUG ") {
                Color::Cyan
            } else if s.contains(" TRACE ") {
                Color::DarkGray
            } else {
                Color::White
            };
            Line::from(Span::styled(s.clone(), Style::default().fg(color)))
        })
        .collect();

    let log_para = Paragraph::new(visible_lines);
    frame.render_widget(log_para, chunks[1]);

    // Footer
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            " Up/Down/PgUp/PgDn:scroll  Home:top  End:bottom  l/Esc:close  q:quit ",
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    frame.render_widget(footer, chunks[2]);
}

// ─── Compact layout (original) ───────────────────────────────────────────────

fn render_compact(frame: &mut Frame, state: &AppState) {
    let area = frame.area();

    let footer_height = if state.domain.active_alerts.is_empty() { 3 } else { 4 };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(footer_height),
            Constraint::Length(1), // bottom margin
        ])
        .split(area);

    render_compact_header(frame, chunks[0], state);
    render_compact_body(frame, chunks[1], state);
    render_footer(frame, chunks[2], state);
}

fn render_compact_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let (status_text, status_color) = connection_style(&state.domain.connection);

    let voltage_text = state
        .domain.vehicle
        .battery_voltage
        .map(|v| format!("{v:.1}V"))
        .unwrap_or_else(|| "--.-V".into());

    let vehicle_name = state
        .domain.vehicle_info
        .as_ref()
        .map(|v| v.display_name())
        .unwrap_or_else(|| "OBD2 Dashboard".to_string());

    let mut spans = vec![
        Span::styled(
            format!(" {} ", vehicle_name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("    "),
        Span::styled(status_text, Style::default().fg(status_color)),
        Span::raw("    "),
        Span::styled(voltage_text, Style::default().fg(Color::Yellow)),
    ];

    if let Some(ref info) = state.domain.adapter_info {
        spans.push(Span::raw("    "));
        spans.push(Span::styled(
            format!("{}", info.chipset),
            Style::default().fg(Color::White),
        ));
    }

    let header = Paragraph::new(Line::from(spans))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

    frame.render_widget(header, area);
}

fn render_compact_body(frame: &mut Frame, area: Rect, state: &AppState) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let top_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[0]);

    let bottom_cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    render_rpm(frame, top_cols[0], state);
    render_speed(frame, top_cols[1], state);
    render_coolant(frame, bottom_cols[0], state);
    render_compact_driving(frame, bottom_cols[1], state);
}

// ─── Full detail layout ──────────────────────────────────────────────────────

fn render_full(frame: &mut Frame, state: &AppState) {
    let area = frame.area();
    let config = &state.dashboard_config;

    let footer_height = if state.domain.active_alerts.is_empty() { 3 } else { 4 };

    // Build row height constraints from config
    let mut constraints = vec![Constraint::Length(3)]; // Header
    for row in &config.rows {
        match row.height {
            RowHeight::Fixed(h) => constraints.push(Constraint::Length(h)),
            RowHeight::Min(h) => constraints.push(Constraint::Min(h)),
            RowHeight::Proportional(w) => constraints.push(Constraint::Ratio(w as u32, 10)),
        }
    }
    constraints.push(Constraint::Length(footer_height)); // Footer
    constraints.push(Constraint::Length(1)); // bottom margin

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    render_full_header(frame, chunks[0], state);

    // Render each config row
    let mut flat_idx = 0usize;
    for (row_idx, row) in config.rows.iter().enumerate() {
        let row_area = chunks[1 + row_idx]; // +1 for header

        // Determine focused column within this row
        let focused_col = state.focused_widget.and_then(|fw| {
            let (r, c) = config.flat_to_row_col(fw)?;
            if r == row_idx { Some(c) } else { None }
        });

        let percentages = config.row_widget_percentages(row_idx, focused_col);
        let col_constraints: Vec<Constraint> = percentages
            .iter()
            .map(|&p| Constraint::Percentage(p))
            .collect();

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(row_area);

        for (col_idx, slot) in row.widgets.iter().enumerate() {
            let is_focused = state.focused_widget == Some(flat_idx);
            let selected = if is_focused {
                state.widget_selections.get(&flat_idx).copied()
            } else {
                None
            };

            if let Some(&col_area) = cols.get(col_idx) {
                renderers::render_widget(frame, col_area, slot.kind, state, is_focused, selected);
            }
            flat_idx += 1;
        }
    }

    // Footer is second-to-last chunk (last is the bottom margin)
    let footer_idx = chunks.len() - 2;
    render_footer(frame, chunks[footer_idx], state);
}

fn render_full_header(frame: &mut Frame, area: Rect, state: &AppState) {
    let voltage_text = state
        .domain.vehicle
        .battery_voltage
        .map(|v| format!("{v:.1}V"))
        .unwrap_or_else(|| "--.-V".into());

    let vehicle_name = state
        .domain.vehicle_info
        .as_ref()
        .map(|v| v.display_name())
        .unwrap_or_else(|| "OBD2 Dashboard".to_string());

    let vin_short = state
        .domain.vehicle_info
        .as_ref()
        .map(|v| {
            if v.vin.len() > 10 {
                format!("{}...", &v.vin[..10])
            } else {
                v.vin.clone()
            }
        })
        .unwrap_or_default();

    let engine_code = state
        .domain.vehicle_info
        .as_ref()
        .and_then(|v| v.engine_family_code.as_deref())
        .unwrap_or("");

    let mut spans = vec![
        Span::styled(
            format!(" {} ", vehicle_name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if !vin_short.is_empty() {
        spans.push(Span::styled(" | VIN: ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(vin_short, Style::default().fg(Color::White)));
    }

    if !engine_code.is_empty() {
        spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(engine_code, Style::default().fg(Color::White)));
    }

    spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));

    // Show replay status or connection status
    match &state.domain.recording {
        RecordingState::Replaying(controller) => {
            let speed = controller.speed_label();
            spans.push(Span::styled(
                format!("REPLAY {}", speed),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        RecordingState::Recording { start_instant, .. } => {
            let elapsed = start_instant.elapsed().as_secs();
            spans.push(Span::styled(
                format!("REC {:02}:{:02}", elapsed / 60, elapsed % 60),
                Style::default()
                    .fg(Color::Red)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        RecordingState::Idle => {
            let (status_text, status_color) = connection_style(&state.domain.connection);
            spans.push(Span::styled(status_text, Style::default().fg(status_color)));
        }
    }

    spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
    spans.push(Span::styled(voltage_text, Style::default().fg(Color::Yellow)));

    // Show adapter chipset if detected
    if let Some(ref info) = state.domain.adapter_info {
        spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            format!("{}", info.chipset),
            Style::default().fg(Color::White),
        ));
    }

    let header = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(header, area);
}

pub(crate) fn render_full_gauges_and_engine(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    selected_item: Option<usize>,
) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let sub_border = Style::default().fg(Color::DarkGray);

    // RPM gauge+sparkline, Speed gauge+sparkline, then Engine panel
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // RPM gauge
            Constraint::Length(2), // RPM sparkline
            Constraint::Length(3), // Speed gauge
            Constraint::Length(2), // Speed sparkline
            Constraint::Min(1),   // Engine data panel
        ])
        .split(inner);

    // RPM gauge — item 0
    let rpm_val = state.domain.vehicle.rpm.as_ref().map(|r| r.value).unwrap_or(0.0);
    let rpm_color = threshold_color_for_pid(state, 0x0C, rpm_val, || rpm_color_default(rpm_val));
    let max_rpm = state
        .domain.thresholds_cache
        .get(&0x0C)
        .and_then(|t| t.high_critical)
        .map(|c| c * 1.15)
        .unwrap_or(8000.0);

    let rpm_border = if selected_item == Some(0) {
        Style::default().fg(Color::Yellow)
    } else {
        sub_border
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" RPM ")
                .borders(Borders::ALL)
                .border_style(rpm_border),
        )
        .gauge_style(Style::default().fg(rpm_color))
        .label(format!("{:.0} rpm", rpm_val))
        .ratio((rpm_val / max_rpm).clamp(0.0, 1.0));
    frame.render_widget(gauge, chunks[0]);

    let rpm_hist: Vec<u64> = state.domain.vehicle.rpm_history.readings.iter().copied().collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&rpm_hist)
        .max(max_rpm as u64)
        .style(Style::default().fg(rpm_color));
    frame.render_widget(sparkline, chunks[1]);

    // Speed gauge — item 1
    let (speed_val, speed_unit_str) = state.domain.display_speed().unwrap_or((0.0, "km/h"));
    let speed_color = threshold_color_for_pid(state, 0x0D, speed_val, || Color::Blue);

    let speed_border = if selected_item == Some(1) {
        Style::default().fg(Color::Yellow)
    } else {
        sub_border
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Speed ")
                .borders(Borders::ALL)
                .border_style(speed_border),
        )
        .gauge_style(Style::default().fg(speed_color))
        .label(format!("{:.0} {}", speed_val, speed_unit_str))
        .ratio((speed_val / 260.0).clamp(0.0, 1.0));
    frame.render_widget(gauge, chunks[2]);

    let speed_hist: Vec<u64> = state.domain.vehicle.speed_history.readings.iter().copied().collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&speed_hist)
        .max(260)
        .style(Style::default().fg(speed_color));
    frame.render_widget(sparkline, chunks[3]);

    // Engine panel — items 2+ are engine data lines
    let mut lines = Vec::new();
    let engine_item_start = 2; // Items 0=RPM, 1=Speed, 2+=engine lines

    // Load gauge line
    let load_val = state.domain.vehicle.engine_load.as_ref().map(|r| r.value).unwrap_or(0.0);
    let load_color = threshold_color_for_pid(state, 0x04, load_val, || Color::Magenta);
    lines.push(make_engine_line("Load", &format!("{:.1}%", load_val), load_color, selected_item, engine_item_start, 0));

    // Throttle
    let throttle_val = state.domain.vehicle.throttle_position.as_ref().map(|r| r.value).unwrap_or(0.0);
    let throttle_color = threshold_color_for_pid(state, 0x11, throttle_val, || Color::Cyan);
    lines.push(make_engine_line("Thrtl", &format!("{:.1}%", throttle_val), throttle_color, selected_item, engine_item_start, 1));

    // Conditional engine items
    let mut engine_idx = 2usize;

    // MAP
    if let Some(r) = &state.domain.vehicle.intake_map {
        let color = threshold_color_for_pid(state, 0x0B, r.value, || Color::White);
        lines.push(make_engine_line("MAP", &format!("{:.0} kPa", r.value), color, selected_item, engine_item_start, engine_idx));
        engine_idx += 1;
    }

    // MAF
    if let Some(r) = &state.domain.vehicle.maf {
        let color = threshold_color_for_pid(state, 0x10, r.value, || Color::White);
        lines.push(make_engine_line("MAF", &format!("{:.1} g/s", r.value), color, selected_item, engine_item_start, engine_idx));
        engine_idx += 1;
    }

    // Fuel pressure
    if let Some(r) = &state.domain.vehicle.fuel_pressure {
        let color = threshold_color_for_pid(state, 0x0A, r.value, || Color::White);
        lines.push(make_engine_line("Fuel P", &format!("{:.0} kPa", r.value), color, selected_item, engine_item_start, engine_idx));
        engine_idx += 1;
    }

    // Boost pressure (derived: MAP - Barometric)
    if let Some(boost) = state.domain.vehicle.boost_pressure {
        let color = if boost > 0.5 { Color::Green } else { Color::DarkGray };
        lines.push(make_engine_line("Boost", &format!("{:.1} kPa", boost), color, selected_item, engine_item_start, engine_idx));
        engine_idx += 1;
    }

    // Oil pressure
    if let Some(r) = &state.domain.vehicle.oil_pressure {
        let color = threshold_color_for_pid(state, 0xFD, r.value, || {
            if r.value < 100.0 { Color::Red } else if r.value < 150.0 { Color::Yellow } else { Color::Green }
        });
        lines.push(make_engine_line("Oil P", &format!("{:.0} kPa", r.value), color, selected_item, engine_item_start, engine_idx));
        let _ = engine_idx; // suppress unused warning
    }

    let engine_block = Paragraph::new(lines).block(
        Block::default()
            .title(" ENGINE ")
            .borders(Borders::ALL)
            .border_style(sub_border),
    );
    frame.render_widget(engine_block, chunks[4]);
}

pub(crate) fn render_full_temperatures(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    selected_item: Option<usize>,
) {
    let mut lines = Vec::new();

    let add_temp = |lines: &mut Vec<Line>, label: &str, reading: &Option<PidReading>, pid_code: u8, state: &AppState, idx: usize, sel: Option<usize>| {
        let line = if let Some(r) = reading {
            let (val, unit) = state.domain.display_temp_value(r);
            let color = threshold_color_for_pid(state, pid_code, r.value, || temp_color_default(r.value));
            make_value_line(label, &format!("{:.1}{}", val, unit), color)
        } else {
            make_value_line(label, "--", Color::DarkGray)
        };
        lines.push(maybe_highlight(line, sel, idx));
    };

    add_temp(&mut lines, "Coolant", &state.domain.vehicle.coolant_temp, 0x05, state, 0, selected_item);
    add_temp(&mut lines, "Oil", &state.domain.vehicle.engine_oil_temp, 0x5C, state, 1, selected_item);
    add_temp(&mut lines, "Trans", &state.domain.vehicle.transmission_temp, 0xFE, state, 2, selected_item);
    add_temp(&mut lines, "Intake Air", &state.domain.vehicle.intake_air_temp, 0x0F, state, 3, selected_item);
    add_temp(&mut lines, "Ambient", &state.domain.vehicle.ambient_air_temp, 0x46, state, 4, selected_item);
    add_temp(&mut lines, "Cat B1S1", &state.domain.vehicle.catalyst_temp_b1s1, 0x3C, state, 5, selected_item);
    add_temp(&mut lines, "Cat B2S1", &state.domain.vehicle.catalyst_temp_b2s1, 0x3D, state, 6, selected_item);
    add_temp(&mut lines, "Cat B1S2", &state.domain.vehicle.catalyst_temp_b1s2, 0x3E, state, 7, selected_item);
    add_temp(&mut lines, "Cat B2S2", &state.domain.vehicle.catalyst_temp_b2s2, 0x3F, state, 8, selected_item);

    let temps = Paragraph::new(lines).block(block);
    frame.render_widget(temps, area);
}

pub(crate) fn render_full_fuel_system(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    selected_item: Option<usize>,
) {
    let mut lines = Vec::new();
    let mut item_idx = 0usize;

    // Fuel tank level — always item 0
    let tank_line = if let Some(r) = &state.domain.vehicle.fuel_tank_level {
        let color = threshold_color_for_pid(state, 0x2F, r.value, || {
            if r.value < 15.0 { Color::Red } else if r.value < 25.0 { Color::Yellow } else { Color::Green }
        });
        make_value_line("Tank", &format!("{:.1}%", r.value), color)
    } else {
        make_value_line("Tank", "--", Color::DarkGray)
    };
    lines.push(maybe_highlight(tank_line, selected_item, item_idx));
    item_idx += 1;

    // Fuel rate — conditional
    if let Some(r) = &state.domain.vehicle.engine_fuel_rate {
        let color = threshold_color_for_pid(state, 0x5E, r.value, || Color::White);
        let line = make_value_line("Rate", &format!("{:.1} L/h", r.value), color);
        lines.push(maybe_highlight(line, selected_item, item_idx));
        item_idx += 1;
    }

    // Fuel trims
    let trim_line = make_trim_line("STFT B1", &state.domain.vehicle.short_fuel_trim_b1, 0x06, state);
    lines.push(maybe_highlight(trim_line, selected_item, item_idx));
    item_idx += 1;

    let trim_line = make_trim_line("LTFT B1", &state.domain.vehicle.long_fuel_trim_b1, 0x07, state);
    lines.push(maybe_highlight(trim_line, selected_item, item_idx));
    item_idx += 1;

    let trim_line = make_trim_line("STFT B2", &state.domain.vehicle.short_fuel_trim_b2, 0x08, state);
    lines.push(maybe_highlight(trim_line, selected_item, item_idx));
    item_idx += 1;

    let trim_line = make_trim_line("LTFT B2", &state.domain.vehicle.long_fuel_trim_b2, 0x09, state);
    lines.push(maybe_highlight(trim_line, selected_item, item_idx));
    let _ = item_idx;

    let fuel = Paragraph::new(lines).block(block);
    frame.render_widget(fuel, area);
}

pub(crate) fn render_full_system_info(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    selected_item: Option<usize>,
) {
    let mut lines = Vec::new();
    let mut item_idx = 0usize;

    // Battery voltage
    if let Some(v) = state.domain.vehicle.battery_voltage {
        let line = make_value_line("Batt Voltage", &format!("{:.1}V", v), Color::Yellow);
        lines.push(maybe_highlight(line, selected_item, item_idx));
        item_idx += 1;
    }

    // Control module voltage
    if let Some(r) = &state.domain.vehicle.control_module_voltage {
        let color = threshold_color_for_pid(state, 0x42, r.value, || Color::Yellow);
        let line = make_value_line("Module Volt", &format!("{:.1}V", r.value), color);
        lines.push(maybe_highlight(line, selected_item, item_idx));
        item_idx += 1;
    }

    // Barometric
    if let Some(r) = &state.domain.vehicle.barometric_pressure {
        let color = threshold_color_for_pid(state, 0x33, r.value, || Color::White);
        let line = make_value_line("Barometric", &format!("{:.1} kPa", r.value), color);
        lines.push(maybe_highlight(line, selected_item, item_idx));
        item_idx += 1;
    }

    // Blank separator line (not selectable)
    lines.push(Line::from(""));

    // Vehicle details
    if let Some(info) = &state.domain.vehicle_info {
        let line = make_value_line("VIN", &info.vin, Color::White);
        lines.push(maybe_highlight(line, selected_item, item_idx));
        item_idx += 1;

        // Engine description
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
            let line = make_value_line("Engine", &engine_parts.join(" "), Color::White);
            lines.push(maybe_highlight(line, selected_item, item_idx));
            item_idx += 1;
        }

        // Trans / Drive / Fuel
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
            let line = make_value_line("Config", &detail_parts.join("  "), Color::White);
            lines.push(maybe_highlight(line, selected_item, item_idx));
            let _ = item_idx;
        }
    }

    let sys = Paragraph::new(lines).block(block);
    frame.render_widget(sys, area);
}

pub(crate) fn render_full_dtcs(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    selected_item: Option<usize>,
) {
    let dtcs = &state.domain.stored_dtcs;

    let mut lines: Vec<Line> = Vec::new();

    if dtcs.is_empty() {
        lines.push(Line::from(Span::styled(
            " No stored codes",
            Style::default().fg(Color::Green),
        )));
    } else {
        for (i, dtc) in dtcs.iter().enumerate() {
            let color = dtc_color(dtc);
            let line = Line::from(vec![
                Span::styled(
                    format!(" {:<6}", dtc.code),
                    Style::default()
                        .fg(color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    dtc.description.to_string(),
                    Style::default().fg(color),
                ),
            ]);
            lines.push(maybe_highlight(line, selected_item, i));
        }
    }

    let panel = Paragraph::new(lines).block(block);
    frame.render_widget(panel, area);
}

pub(crate) fn render_full_fuel_economy(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    block: Block,
    selected_item: Option<usize>,
) {
    let inner = block.inner(area);
    frame.render_widget(block, area);

    // Split horizontally: 45% gold | 55% advanced
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(inner);

    render_gold_side(frame, cols[0], state, selected_item);
    render_advanced_side(frame, cols[1], state, selected_item);
}

fn render_gold_side(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    selected_item: Option<usize>,
) {
    let fe = &state.domain.fuel_economy;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Data lines
            Constraint::Min(2),   // Sparkline
        ])
        .split(area);

    // Data lines
    let mut lines = Vec::new();

    let source_label = fe.gold.as_ref()
        .map(|g| g.source.label())
        .unwrap_or("Waiting...");
    let source_line = Line::from(vec![
        Span::styled(" Source:  ", Style::default().fg(Color::DarkGray)),
        Span::styled(source_label, Style::default().fg(Color::White)),
    ]);
    lines.push(maybe_highlight(source_line, selected_item, 0));

    let instant_mpg = fe.gold.as_ref().map(|g| g.instant_mpg).unwrap_or(0.0);
    let instant_color = mpg_color(instant_mpg);
    let instant_line = Line::from(vec![
        Span::styled(" Instant: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1} MPG", instant_mpg),
            Style::default().fg(instant_color).add_modifier(Modifier::BOLD),
        ),
    ]);
    lines.push(maybe_highlight(instant_line, selected_item, 1));

    let avg_mpg = fe.gold.as_ref().map(|g| g.avg_mpg).unwrap_or(0.0);
    let avg_line = Line::from(vec![
        Span::styled(" Average: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1} MPG", avg_mpg),
            Style::default().fg(mpg_color(avg_mpg)),
        ),
    ]);
    lines.push(maybe_highlight(avg_line, selected_item, 2));

    let rate = fe.gold.as_ref().map(|g| g.fuel_rate_lph).unwrap_or(0.0);
    let rate_line = Line::from(vec![
        Span::styled(" Rate:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.1} L/h", rate), Style::default().fg(Color::White)),
    ]);
    lines.push(maybe_highlight(rate_line, selected_item, 3));

    let gold_block = Block::default()
        .title(" ECU (Gold Standard) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let data_para = Paragraph::new(lines).block(gold_block);
    frame.render_widget(data_para, rows[0]);

    // Sparkline
    let hist: Vec<u64> = fe.gold_mpg_history.iter().copied().collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)))
        .data(&hist)
        .max(600) // 60 MPG × 10
        .style(Style::default().fg(Color::Yellow));
    frame.render_widget(sparkline, rows[1]);
}

fn render_advanced_side(
    frame: &mut Frame,
    area: Rect,
    state: &AppState,
    selected_item: Option<usize>,
) {
    let fe = &state.domain.fuel_economy;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Data lines
            Constraint::Min(2),   // Sparkline + corrections
        ])
        .split(area);

    // Data lines
    let mut lines = Vec::new();

    let method_line = Line::from(vec![
        Span::styled(" Method:  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Speed-Density", Style::default().fg(Color::White)),
    ]);
    lines.push(maybe_highlight(method_line, selected_item, 4));

    let instant_mpg = fe.advanced.as_ref().map(|a| a.instant_mpg).unwrap_or(0.0);
    let instant_color = mpg_color(instant_mpg);
    let instant_line = Line::from(vec![
        Span::styled(" Instant: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1} MPG", instant_mpg),
            Style::default().fg(instant_color).add_modifier(Modifier::BOLD),
        ),
    ]);
    lines.push(maybe_highlight(instant_line, selected_item, 5));

    let avg_mpg = fe.advanced.as_ref().map(|a| a.avg_mpg).unwrap_or(0.0);
    let avg_line = Line::from(vec![
        Span::styled(" Average: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{:.1} MPG", avg_mpg),
            Style::default().fg(mpg_color(avg_mpg)),
        ),
    ]);
    lines.push(maybe_highlight(avg_line, selected_item, 6));

    let rate = fe.advanced.as_ref().map(|a| a.corrected_fuel_rate_lph).unwrap_or(0.0);
    let base_rate = fe.advanced.as_ref().map(|a| a.base_fuel_rate_lph).unwrap_or(0.0);
    let rate_line = Line::from(vec![
        Span::styled(" Rate:    ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.1} L/h", rate), Style::default().fg(Color::White)),
        Span::styled(format!(" (base {:.1})", base_rate), Style::default().fg(Color::DarkGray)),
    ]);
    lines.push(maybe_highlight(rate_line, selected_item, 7));

    let adv_block = Block::default()
        .title(" Calculated (Advanced) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let data_para = Paragraph::new(lines).block(adv_block);
    frame.render_widget(data_para, rows[0]);

    // Bottom area: sparkline on left, corrections on right
    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[1]);

    // Sparkline
    let hist: Vec<u64> = fe.advanced_mpg_history.iter().copied().collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)))
        .data(&hist)
        .max(600)
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(sparkline, bottom[0]);

    // Correction factors
    let corrections = fe.advanced.as_ref().map(|a| &a.corrections);
    let mut corr_lines = Vec::new();

    let cold = corrections.map(|c| c.cold_engine).unwrap_or(1.0);
    let alt = corrections.map(|c| c.altitude).unwrap_or(1.0);
    corr_lines.push(Line::from(vec![
        correction_span("Cold", cold),
        Span::raw("  "),
        correction_span("Alt", alt),
    ]));

    let air = corrections.map(|c| c.air_density).unwrap_or(1.0);
    let trims = corrections.map(|c| c.fuel_trims).unwrap_or(1.0);
    corr_lines.push(Line::from(vec![
        correction_span("Air", air),
        Span::raw("  "),
        correction_span("Trm", trims),
    ]));

    let cat = corrections.map(|c| c.catalyst_warmup).unwrap_or(1.0);
    let trans = corrections.map(|c| c.throttle_transient).unwrap_or(1.0);
    corr_lines.push(Line::from(vec![
        correction_span("Cat", cat),
        Span::raw("  "),
        correction_span("Thr", trans),
    ]));

    let wot = corrections.map(|c| c.high_load_wot).unwrap_or(1.0);
    let total = corrections.map(|c| c.total()).unwrap_or(1.0);
    corr_lines.push(Line::from(vec![
        correction_span("WOT", wot),
        Span::raw("  "),
        Span::styled(
            format!("TOT {:.2}", total),
            Style::default().fg(correction_color(total)).add_modifier(Modifier::BOLD),
        ),
    ]));

    let corr_block = Block::default()
        .title(" Corrections ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let corr_para = Paragraph::new(corr_lines).block(corr_block);
    frame.render_widget(corr_para, bottom[1]);
}

fn correction_span<'a>(label: &str, value: f64) -> Span<'a> {
    let color = correction_color(value);
    Span::styled(
        format!("{} {:.2}", label, value),
        Style::default().fg(color),
    )
}

fn correction_color(value: f64) -> Color {
    let deviation = (value - 1.0).abs();
    if deviation < 0.01 {
        Color::Green
    } else if deviation < 0.05 {
        Color::Yellow
    } else {
        Color::Red
    }
}

fn mpg_color(mpg: f64) -> Color {
    if mpg < 0.1 {
        Color::DarkGray
    } else if mpg < 15.0 {
        Color::Red
    } else if mpg < 25.0 {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// Choose a color for a DTC based on its code pattern.
fn dtc_color(dtc: &obd2_core::Dtc) -> Color {
    match dtc.category {
        DtcCategory::Powertrain => {
            // P03xx misfire = red, P07xx transmission = red, others = yellow
            if dtc.code.starts_with("P03") || dtc.code.starts_with("P07") {
                Color::Red
            } else {
                Color::Yellow
            }
        }
        // Chassis / Body / Network codes are always red (less common, more serious)
        _ => Color::Red,
    }
}

// ─── Edit mode overlay ──────────────────────────────────────────────────────

fn render_edit_overlay(frame: &mut Frame, edit: &crate::widget::edit_mode::EditModeState) {
    use crate::widget::edit_mode::EditModeState;

    let area = centered_rect(60, 60, frame.area());
    frame.render_widget(Clear, area);

    match &edit.phase {
        EditPhase::Browse => {
            let block = Block::default()
                .title(" EDIT MODE - Browse ")
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Yellow));

            let mut lines = vec![
                Line::from(Span::styled(
                    format!(" Widget {} of {}", edit.cursor + 1, edit.working_config.widget_count()),
                    Style::default().fg(Color::White),
                )),
                Line::from(""),
            ];

            // Show current widget list with cursor
            let mut flat = 0usize;
            for (row_idx, row) in edit.working_config.rows.iter().enumerate() {
                lines.push(Line::from(Span::styled(
                    format!(" Row {}:", row_idx + 1),
                    Style::default().fg(Color::DarkGray),
                )));
                for slot in &row.widgets {
                    let marker = if flat == edit.cursor { "> " } else { "  " };
                    let style = if flat == edit.cursor {
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    lines.push(Line::from(Span::styled(
                        format!(" {}{:?} ({:?})", marker, slot.kind, slot.size),
                        style,
                    )));
                    flat += 1;
                }
            }

            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                " a:add  x:delete  s:save  Esc:cancel  \u{2191}\u{2193}:navigate",
                Style::default().fg(Color::DarkGray),
            )));

            let para = Paragraph::new(lines).block(block);
            frame.render_widget(para, area);
        }
        EditPhase::CategoryPicker { selected } => {
            let block = Block::default()
                .title(" SELECT CATEGORY ")
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Cyan));

            let items = EditModeState::category_items();
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .map(|(i, (title, desc))| {
                    let marker = if i == *selected { "> " } else { "  " };
                    let style = if i == *selected {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(vec![
                        Span::styled(format!(" {}{}", marker, title), style),
                        Span::styled(format!("  {}", desc), Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();

            let mut all_lines = lines;
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(Span::styled(
                " Enter:select  Esc:back",
                Style::default().fg(Color::DarkGray),
            )));

            let para = Paragraph::new(all_lines).block(block);
            frame.render_widget(para, area);
        }
        EditPhase::WidgetPicker { category, selected } => {
            let block = Block::default()
                .title(format!(" SELECT WIDGET - {} ", category.title()))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Cyan));

            let items = EditModeState::widget_items(*category);
            let lines: Vec<Line> = items
                .iter()
                .enumerate()
                .map(|(i, (title, desc))| {
                    let marker = if i == *selected { "> " } else { "  " };
                    let style = if i == *selected {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(vec![
                        Span::styled(format!(" {}{}", marker, title), style),
                        Span::styled(format!("  {}", desc), Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();

            let mut all_lines = lines;
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(Span::styled(
                " Enter:select  Esc:back",
                Style::default().fg(Color::DarkGray),
            )));

            let para = Paragraph::new(all_lines).block(block);
            frame.render_widget(para, area);
        }
        EditPhase::SizePicker { kind, selected } => {
            let block = Block::default()
                .title(format!(" SELECT SIZE - {:?} ", kind))
                .borders(Borders::ALL)
                .border_type(BorderType::Double)
                .border_style(Style::default().fg(Color::Cyan));

            let options = [("Half", "Takes roughly half the row width"), ("Full", "Takes the full row width")];
            let lines: Vec<Line> = options
                .iter()
                .enumerate()
                .map(|(i, (title, desc))| {
                    let marker = if i == *selected { "> " } else { "  " };
                    let style = if i == *selected {
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    Line::from(vec![
                        Span::styled(format!(" {}{}", marker, title), style),
                        Span::styled(format!("  {}", desc), Style::default().fg(Color::DarkGray)),
                    ])
                })
                .collect();

            let mut all_lines = lines;
            all_lines.push(Line::from(""));
            all_lines.push(Line::from(Span::styled(
                " Enter:select  Esc:back",
                Style::default().fg(Color::DarkGray),
            )));

            let para = Paragraph::new(all_lines).block(block);
            frame.render_widget(para, area);
        }
    }
}

// ─── Session picker overlay ─────────────────────────────────────────────────

fn render_session_picker(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(75, 60, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" SELECT RECORDING ")
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Magenta));

    // We need the storage manager to get sessions, but we can access it via state
    // For now, show a placeholder — the actual data comes from state.storage_manager
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            format!(" {:<24}{:<20}{:<10}", "Date/Time", "Vehicle", "Duration"),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(Span::styled(
        " ".to_string() + &"-".repeat(54),
        Style::default().fg(Color::DarkGray),
    )));

    // Sessions are loaded from storage manager in main.rs and passed to state
    // Here we just render what's available
    if let Some(ref storage) = state.domain.storage_manager {
        let sessions = storage.index.sessions_sorted();
        if sessions.is_empty() {
            lines.push(Line::from(Span::styled(
                " No recorded sessions",
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            for (i, session) in sessions.iter().enumerate() {
                let marker = if i == state.session_picker_selected {
                    ">"
                } else {
                    " "
                };
                let style = if i == state.session_picker_selected {
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                let vehicle = session
                    .vehicle_name
                    .as_deref()
                    .unwrap_or("Unknown");
                let time_str = session.start_time.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string();
                let dur = session.duration_display();
                lines.push(Line::from(Span::styled(
                    format!("{}{:<24}{:<20}{}", marker, time_str, vehicle, dur),
                    style,
                )));
            }
        }

        let stats = storage.storage_stats();
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                " Total: {} sessions, {}",
                stats.session_count,
                stats.total_display()
            ),
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            " Storage manager not initialized",
            Style::default().fg(Color::DarkGray),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Enter:play  d:delete  Esc:cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);
}

// ─── Device picker overlay ───────────────────────────────────────────────────

fn render_device_picker(frame: &mut Frame, state: &AppState) {
    let area = centered_rect(60, 50, frame.area());
    frame.render_widget(Clear, area);

    let title = match state.scan_mode {
        ScanMode::Scanning => " SCANNING... ",
        ScanMode::Picking => " SELECT DEVICE ",
        ScanMode::Idle => return,
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = Vec::new();

    // Column header
    lines.push(Line::from(vec![Span::styled(
        format!(" {:<30} {:<10}", "Device", "Type"),
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(Span::styled(
        " ".to_string() + &"-".repeat(42),
        Style::default().fg(Color::DarkGray),
    )));

    if state.scan_devices.is_empty() {
        let hint = if state.scan_mode == ScanMode::Scanning {
            "Searching..."
        } else {
            "No devices found"
        };
        lines.push(Line::from(Span::styled(
            format!(" {}", hint),
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, dev) in state.scan_devices.iter().enumerate() {
            let marker = if i == state.scan_selected { ">" } else { " " };
            let type_label = match &dev.kind {
                obd2_core::obd2::scanner::DeviceKind::Serial { .. } => "Serial",
                obd2_core::obd2::scanner::DeviceKind::Ble { .. } => "BLE",
            };
            let style = if i == state.scan_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("{}{:<30} {:<10}", marker, dev.display_name, type_label),
                style,
            )));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " Enter:connect  Esc:cancel",
        Style::default().fg(Color::DarkGray),
    )));

    let para = Paragraph::new(lines);
    frame.render_widget(para, inner);
}

// ─── Popup overlay ──────────────────────────────────────────────────────────

fn render_popup(frame: &mut Frame, popup: &PopupState) {
    let area = centered_rect(70, 75, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {} ", popup.title))
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow));

    let text: Vec<Line> = popup.body.iter()
        .map(|s| Line::from(s.as_str()))
        .collect();

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

// ─── Shared widget helpers ───────────────────────────────────────────────────

fn render_rpm(frame: &mut Frame, area: Rect, state: &AppState) {
    let rpm_val = state
        .domain.vehicle
        .rpm
        .as_ref()
        .map(|r| r.value)
        .unwrap_or(0.0);

    let color = threshold_color_for_pid(state, 0x0C, rpm_val, || rpm_color_default(rpm_val));

    let max_rpm = state
        .domain.thresholds_cache
        .get(&0x0C)
        .and_then(|t| t.high_critical)
        .map(|c| c * 1.15)
        .unwrap_or(8000.0);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(2)])
        .split(area);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Engine RPM ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(color))
        .label(format!("{:.0} rpm", rpm_val))
        .ratio((rpm_val / max_rpm).clamp(0.0, 1.0));

    frame.render_widget(gauge, chunks[0]);

    let history: Vec<u64> = state.domain.vehicle.rpm_history.readings.iter().copied().collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM))
        .data(&history)
        .max(max_rpm as u64)
        .style(Style::default().fg(color));

    frame.render_widget(sparkline, chunks[1]);
}

fn render_speed(frame: &mut Frame, area: Rect, state: &AppState) {
    let (speed_val, speed_unit) = state.domain.display_speed().unwrap_or((0.0, "km/h"));

    let color = threshold_color_for_pid(state, 0x0D, speed_val, || Color::Blue);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(2)])
        .split(area);

    let gauge = Gauge::default()
        .block(
            Block::default()
                .title(" Vehicle Speed ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(color))
        .label(format!("{:.0} {}", speed_val, speed_unit))
        .ratio((speed_val / 260.0).clamp(0.0, 1.0));

    frame.render_widget(gauge, chunks[0]);

    let history: Vec<u64> = state
        .domain.vehicle
        .speed_history
        .readings
        .iter()
        .copied()
        .collect();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM))
        .data(&history)
        .max(260)
        .style(Style::default().fg(color));

    frame.render_widget(sparkline, chunks[1]);
}

fn render_coolant(frame: &mut Frame, area: Rect, state: &AppState) {
    let (temp_val, temp_unit) = state.domain.display_temp().unwrap_or((0.0, "°C"));

    let raw_celsius = state
        .domain.vehicle
        .coolant_temp
        .as_ref()
        .map(|r| r.value)
        .unwrap_or(0.0);
    let color = threshold_color_for_pid(state, 0x05, raw_celsius, || {
        temp_color_default(raw_celsius)
    });

    let text = format!("{:.0}{}", temp_val, temp_unit);
    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        text,
        Style::default()
            .fg(color)
            .add_modifier(Modifier::BOLD),
    )]))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .title(" Coolant Temp ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    frame.render_widget(paragraph, area);
}

fn render_compact_driving(frame: &mut Frame, area: Rect, state: &AppState) {
    let block = Block::default()
        .title(" Driving Behavior ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let driving = &state.domain.driving;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Smoothness gauge
            Constraint::Min(2),   // Accel sparkline
            Constraint::Length(1), // Current accel value
            Constraint::Length(1), // Event counters
        ])
        .split(inner);

    // Smoothness gauge
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

    // Acceleration sparkline
    let hist = driving.accel_display_history();
    let sparkline = Sparkline::default()
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
        .data(&hist)
        .max(500)
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(sparkline, chunks[1]);

    // Current acceleration
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
            Style::default().fg(accel_color).add_modifier(Modifier::BOLD),
        ),
    ]));
    frame.render_widget(accel_line, chunks[2]);

    // Event counters
    let events_line = Paragraph::new(Line::from(vec![
        Span::styled(" Brakes: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", driving.hard_brake_count),
            Style::default().fg(if driving.hard_brake_count > 0 { Color::Red } else { Color::Green }),
        ),
        Span::styled("  Jackrabbits: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{}", driving.jackrabbit_count),
            Style::default().fg(if driving.jackrabbit_count > 0 { Color::Yellow } else { Color::Green }),
        ),
    ]));
    frame.render_widget(events_line, chunks[3]);
}

fn render_footer(frame: &mut Frame, area: Rect, state: &AppState) {
    let mut lines = Vec::new();

    // Replay progress bar
    if let RecordingState::Replaying(controller) = &state.domain.recording {
        let progress = controller.progress_text();
        let pause_label = if controller.paused { "Space:resume" } else { "Space:pause" };
        let replay_line = Line::from(vec![
            Span::styled(" ", Style::default()),
            Span::styled(
                format!("{} | [/]:seek | {}  | s:speed | Esc:stop", progress, pause_label),
                Style::default().fg(Color::Magenta),
            ),
        ]);
        lines.push(replay_line);
    }

    // Alert line (if any alerts are active)
    if !state.domain.active_alerts.is_empty() {
        let alert_spans: Vec<Span> = state
            .domain.active_alerts
            .iter()
            .enumerate()
            .flat_map(|(i, alert)| {
                let color = match alert.level {
                    AlertLevel::Critical => Color::Red,
                    AlertLevel::Warning => Color::Yellow,
                };
                let mut spans = vec![Span::styled(
                    alert.to_string(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )];
                if i < state.domain.active_alerts.len() - 1 {
                    spans.push(Span::raw("  "));
                }
                spans
            })
            .collect();
        lines.push(Line::from(alert_spans));
    }

    // Status line — varies by mode
    let error_text = state.domain.last_error.as_deref().unwrap_or("OK");
    let paused = if state.paused { " | PAUSED" } else { "" };

    let layout_toggle = match state.layout {
        DashboardLayout::Compact => "f: full",
        DashboardLayout::Full => "f: compact",
    };

    // Build help text based on current mode
    let help_text = if state.edit_mode.is_some() {
        "EDIT: a:add x:del s:save Esc:cancel \u{2190}\u{2191}\u{2192}\u{2193}:nav".to_string()
    } else if state.domain.recording.is_replaying() {
        format!("Poll: {}ms{} | {}", state.domain.poll_interval_ms, paused, layout_toggle)
    } else {
        let rec_hint = if state.domain.recording.is_recording() {
            " | r:stop rec"
        } else {
            " | r:rec R:replay"
        };
        let edit_hint = if state.layout == DashboardLayout::Full {
            " | e:edit"
        } else {
            ""
        };
        let scan_hint = match state.domain.connection {
            ConnectionState::Disconnected | ConnectionState::Error(_) => " | s:scan",
            _ => "",
        };
        format!(
            "Poll: {}ms{} | {} | q/p/u/d/+/-/Tab/l:log{}{}{}",
            state.domain.poll_interval_ms, paused, layout_toggle, edit_hint, rec_hint, scan_hint
        )
    };

    // Build left side: help text + DTC count
    let mut left_spans = vec![
        Span::styled(
            format!(" {}", help_text),
            Style::default().fg(Color::DarkGray),
        ),
    ];

    if !state.domain.stored_dtcs.is_empty() {
        let dtc_color = if state.domain.stored_dtcs.len() >= 3 {
            Color::Red
        } else {
            Color::Yellow
        };
        left_spans.push(Span::styled(
            format!(" | DTCs: {}", state.domain.stored_dtcs.len()),
            Style::default().fg(dtc_color).add_modifier(Modifier::BOLD),
        ));
    }

    // Build right side: Status indicator
    let status_label = "Status: ";
    let right_spans = vec![
        Span::styled(status_label, Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} ", error_text),
            if state.domain.last_error.is_some() {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::Green)
            },
        ),
    ];

    // Calculate padding to right-align status
    let inner_width = area.width.saturating_sub(2) as usize; // subtract border chars
    let left_len: usize = left_spans.iter().map(|s| s.content.len()).sum();
    let right_len: usize = right_spans.iter().map(|s| s.content.len()).sum();
    let pad = inner_width.saturating_sub(left_len + right_len);

    let mut status_spans = left_spans;
    status_spans.push(Span::raw(" ".repeat(pad)));
    status_spans.extend(right_spans);

    lines.push(Line::from(status_spans));

    let border_color = match state.domain.worst_alert_level() {
        Some(AlertLevel::Critical) => Color::Red,
        Some(AlertLevel::Warning) => Color::Yellow,
        None => Color::DarkGray,
    };

    let footer = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    );

    frame.render_widget(footer, area);
}

// ─── Line builders ───────────────────────────────────────────────────────────

fn make_value_line<'a>(label: &str, value: &str, color: Color) -> Line<'a> {
    Line::from(vec![
        Span::styled(
            format!(" {:<12}", label),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(value.to_string(), Style::default().fg(color)),
    ])
}

/// Build a value line for the engine panel that may be highlighted.
fn make_engine_line<'a>(
    label: &str,
    value: &str,
    color: Color,
    selected_item: Option<usize>,
    engine_item_start: usize,
    engine_offset: usize,
) -> Line<'a> {
    let line = make_value_line(label, value, color);
    maybe_highlight(line, selected_item, engine_item_start + engine_offset)
}

fn make_trim_line<'a>(label: &str, reading: &Option<PidReading>, pid_code: u8, state: &AppState) -> Line<'a> {
    if let Some(r) = reading {
        let color = threshold_color_for_pid(state, pid_code, r.value, || {
            if r.value.abs() > 10.0 { Color::Yellow } else { Color::Green }
        });
        let sign = if r.value >= 0.0 { "+" } else { "" };
        Line::from(vec![
            Span::styled(
                format!(" {:<12}", label),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{}{:.1}%", sign, r.value),
                Style::default().fg(color),
            ),
        ])
    } else {
        make_value_line(label, "--", Color::DarkGray)
    }
}

/// Apply REVERSED modifier to a line if its index matches the selected item.
fn maybe_highlight(line: Line<'_>, selected: Option<usize>, item_idx: usize) -> Line<'_> {
    if selected == Some(item_idx) {
        let highlighted_spans: Vec<Span> = line.spans.into_iter().map(|span| {
            Span::styled(span.content, span.style.add_modifier(Modifier::REVERSED))
        }).collect();
        Line::from(highlighted_spans)
    } else {
        line
    }
}

// ─── Color helpers ───────────────────────────────────────────────────────────

fn connection_style(conn: &ConnectionState) -> (&'static str, Color) {
    match conn {
        ConnectionState::Connected => ("Connected", Color::Green),
        ConnectionState::Connecting => ("Connecting...", Color::Yellow),
        ConnectionState::Disconnected => ("Disconnected (s:scan)", Color::DarkGray),
        ConnectionState::Error(_) => ("Error (s:scan)", Color::Red),
    }
}

/// Determine gauge color for a PID based on its resolved threshold.
pub(crate) fn threshold_color_for_pid(
    state: &AppState,
    pid_code: u8,
    value: f64,
    default_color: impl FnOnce() -> Color,
) -> Color {
    if let Some(threshold) = state.domain.thresholds_cache.get(&pid_code) {
        if let Some(hc) = threshold.high_critical {
            if value > hc {
                return Color::Red;
            }
        }
        if let Some(lc) = threshold.low_critical {
            if value < lc {
                return Color::Red;
            }
        }
        if let Some(hw) = threshold.high_warning {
            if value > hw {
                return Color::Yellow;
            }
        }
        if let Some(lw) = threshold.low_warning {
            if value < lw {
                return Color::Yellow;
            }
        }
        Color::Green
    } else {
        default_color()
    }
}

/// Default RPM color when no thresholds are loaded.
pub(crate) fn rpm_color_default(rpm: f64) -> Color {
    if rpm < 3000.0 {
        Color::Green
    } else if rpm < 5500.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Default temperature color when no thresholds are loaded.
pub(crate) fn temp_color_default(celsius: f64) -> Color {
    if celsius < 60.0 {
        Color::Blue
    } else if celsius <= 105.0 {
        Color::Green
    } else {
        Color::Red
    }
}
