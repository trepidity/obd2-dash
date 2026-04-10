# obd2-dash User Manual

Complete guide to the OBD-II vehicle diagnostics TUI dashboard.

---

## Table of Contents

1. [Installation & Setup](#1-installation--setup)
2. [Connecting to a Vehicle](#2-connecting-to-a-vehicle)
3. [Dashboard Layouts](#3-dashboard-layouts)
4. [Navigating the Dashboard](#4-navigating-the-dashboard)
5. [Widget System & Customization](#5-widget-system--customization)
6. [Recording Data](#6-recording-data)
7. [Replaying Sessions](#7-replaying-sessions)
8. [Fuel Economy Display](#8-fuel-economy-display)
9. [DTC Diagnostics](#9-dtc-diagnostics)
10. [Threshold Alerts](#10-threshold-alerts)
11. [Vehicle Profiles & Database](#11-vehicle-profiles--database)
12. [Units & Display Options](#12-units--display-options)
13. [Headless Mode](#13-headless-mode)
14. [Driving Behavior](#14-driving-behavior)
15. [Debug Log Viewer](#15-debug-log-viewer)
16. [Raw Protocol Capture](#16-raw-protocol-capture)
17. [Configuration Files](#17-configuration-files)
18. [Keyboard Reference](#18-keyboard-reference)
19. [CLI Reference](#19-cli-reference)
20. [Troubleshooting](#20-troubleshooting)

---

## 1. Installation & Setup

### Prerequisites

- **Rust 1.70+** (edition 2021)
- **For real hardware**: an ELM327-compatible OBD-II adapter connected via USB serial or Bluetooth LE

### Building

```bash
cd obd2-dash
cargo build --release
```

The compiled binary will be at `target/release/obd2-dash`.

### First Run

On first launch, obd2-dash automatically creates:

- `obd2-dash.db` -- SQLite database seeded with vehicle profiles and PID thresholds
- `logs/` -- Directory for daily rolling log files
- `recordings/` -- Directory for recorded session data (created when first recording starts)

---

## 2. Connecting to a Vehicle

### Mock Mode (No Hardware)

Mock mode provides a full simulation of a running vehicle, including realistic warmup cycles, drive patterns, and sensor data. This is ideal for exploring the dashboard, testing recording/replay, and development.

```bash
# Generic vehicle profile
cargo run -- --mock

# 2006 MINI Cooper S (supercharged 1.6L I4)
cargo run -- --mock --mock-vehicle mini

# 2004 Chevy 2500HD Duramax (turbo 6.6L V8 diesel)
cargo run -- --mock --mock-vehicle chevy
```

Each vehicle profile has different engine characteristics:

| Profile | Engine | Max RPM | Idle RPM | Fuel Type |
|---------|--------|---------|----------|-----------|
| `generic` | 1.6L I4 | 6500 | 800 | Gasoline |
| `mini` | 1.6L S/C I4 (W11B16) | 6800 | 850 | Gasoline |
| `chevy` | 6.6L Turbo V8 (LLY) | 3200 | 650 | Diesel |

The mock simulator generates:
- Sinusoidal RPM patterns simulating acceleration/deceleration
- Speed proportional to RPM (varies by vehicle gearing)
- Gradual coolant warmup from cold start to operating temperature
- Realistic fuel trim oscillations
- Catalyst temperature ramp-up
- Battery voltage with small fluctuations

### Real ELM327 Adapter

#### USB Serial

Plug in your ELM327 OBD-II adapter via USB. The app will attempt to auto-detect the serial port:

```bash
# Auto-detect port
cargo run

# Specify port explicitly
cargo run -- --port /dev/cu.usbserial-0001

# Specify port and baud rate
cargo run -- --port /dev/ttyUSB0 --baud 115200
```

**Auto-detection priority:**
1. USB serial ports (preferred -- these are typically ELM327 adapters)
2. First available serial port (fallback)

If no port is found, the app exits with an error suggesting `--port`, `--ble`, or `--mock`.

#### Bluetooth LE

For wireless ELM327/STN adapters that support BLE (e.g., OBDLink MX+, Vgate iCar Pro):

```bash
# BLE with auto-discovery (scans for known adapter names)
cargo run -- --ble

# Filter by adapter name
cargo run -- --ble --ble-name "OBDLink"

# Adjust scan timeout (default 5 seconds)
cargo run -- --ble --ble-scan-secs 10
```

The BLE scan looks for devices matching known adapter name prefixes: OBDLink, OBD, ELM327, STN, OBDII, Vgate.

#### Device Scanner

Press `s` or `S` during normal operation to open the built-in device scanner. It discovers both serial ports and BLE adapters simultaneously and presents a picker to connect. The last-used device is saved to `connection.json` for quick reconnection.

### Polling Rate

The default polling interval is 250ms (4 Hz). You can adjust it at launch or during runtime:

```bash
# Start with 100ms polling (10 Hz)
cargo run -- --mock --poll-ms 100
```

During runtime, press `+` to poll faster (decrease interval by 50ms, minimum 50ms) or `-` to poll slower (increase by 50ms, maximum 2000ms). The current rate is shown in the footer.

---

## 3. Dashboard Layouts

Press `f` to toggle between the two layouts.

### Compact Layout

A streamlined 4-gauge view showing the most essential readings:

```
┌── Engine RPM ──────────────────┐┌── Vehicle Speed ──────────────┐
│ ▓▓▓▓▓▓▓▓▓░░░░░ 2847 rpm       ││ ▓▓▓▓░░░░░░░░░░ 59 km/h       │
│ ▁▂▃▅▆▇▆▅▃▂▁▂▃▅▆▇             ││ ▁▁▂▃▃▃▃▃▂▂▂▃▃▃▃              │
└────────────────────────────────┘└───────────────────────────────┘
┌── Coolant Temp ────────────────┐┌── Engine Load ────────────────┐
│            92°C                ││ ▓▓▓▓▓▓░░░░░░░░ 42.1%         │
└────────────────────────────────┘└───────────────────────────────┘
```

Each gauge includes a sparkline history showing the last ~30 seconds of data.

### Full Layout

A multi-row widget grid displaying all sensor data. The default arrangement is:

| Row | Widgets | Height |
|-----|---------|--------|
| 1 | Gauges + Engine (60%) \| Temperatures (40%) | Flexible (min 10 rows) |
| 2 | Fuel System (35%) \| System/Vehicle (35%) \| DTCs (30%) | Fixed 12 rows |
| 3 | Fuel Economy (100%) | Fixed 10 rows |

This layout is fully customizable -- see [Widget System & Customization](#5-widget-system--customization).

When a widget is focused (via Tab), it expands to take more horizontal space so you can see its contents more clearly. The expansion is 70% for 2-widget rows and 50% for 3-widget rows.

---

## 4. Navigating the Dashboard

### Panel Focus

In Full layout, press `Tab` to cycle focus forward through widgets, or `Shift+Tab` to cycle backward. The focused widget is indicated by a double-line cyan border.

### Item Selection

Once a widget is focused, use `Up` / `Down` arrows to highlight individual items within it (sensor readings, DTCs, fuel economy values, etc.). The selected item is shown with reversed colors.

### Detail Popups

Press `Enter` on a selected item to open a detail popup. The popup shows:

- **For PIDs**: Current value, PID hex code, unit, and configured thresholds (low/high warning/critical ranges)
- **For DTCs**: Trouble code, category, description, correlated sensor readings, other active DTCs, common causes, and suggested repair actions
- **For derived values**: Current value, source description
- **For vehicle fields**: Field name and value

Press `Esc` to dismiss a popup.

### Focus Hierarchy

`Esc` works in layers:
1. If a popup is open, close it
2. If an item is selected within a widget, deselect it
3. If a widget is focused, unfocus it
4. If nothing is focused, quit the application

---

## 5. Widget System & Customization

<img width="1441" height="905" alt="image" src="https://github.com/user-attachments/assets/206be02b-2ab4-449f-8e0f-ee41f3716d07" />

<img width="1441" height="905" alt="image" src="https://github.com/user-attachments/assets/3acfe64f-efe6-4f10-9e95-883ae166d9ad" />

### Available Widget Types

Widgets are organized into 8 categories:

**Engine & Airflow**
| Widget | Description |
|--------|-------------|
| Gauges + Engine | Full composite: RPM/Speed gauges, sparklines, engine data (load, throttle, MAP, MAF, pressures) |
| Engine RPM | RPM gauge with sparkline |
| Vehicle Speed | Speed gauge with sparkline |
| Engine Load | Load percentage gauge |
| Throttle Position | Throttle gauge with sparkline |
| Intake MAP | Manifold absolute pressure display |
| MAF | Mass air flow display |
| Fuel Pressure | Fuel rail pressure display |
| Boost Pressure | Derived boost (MAP - Barometric) |
| Oil Pressure | Engine oil pressure display |

**Fuel & Emissions**
| Widget | Description |
|--------|-------------|
| Fuel System | Composite: tank level, fuel rate, all fuel trims |
| Fuel Economy | Dual panel: ECU gold standard + speed-density calculated MPG |
| Fuel Tank Level | Tank fill percentage gauge |
| Engine Fuel Rate | Consumption rate (L/h) |
| Fuel Trim Bank 1 | STFT and LTFT for bank 1 |
| Fuel Trim Bank 2 | STFT and LTFT for bank 2 |

**Temperature**
| Widget | Description |
|--------|-------------|
| Temperatures | Composite: all 9 temperature readings |
| Coolant Temp | Engine coolant temperature |
| Oil Temp | Engine oil temperature |
| Trans Temp | Transmission fluid temperature |
| Intake Air Temp | Intake air temperature |
| Ambient Air Temp | Outside air temperature |
| Catalyst Temps | All 4 catalyst temperature sensors |

**Transmission & Chassis**
| Widget | Description |
|--------|-------------|
| Vehicle Speed | Speed gauge with sparkline |

**Error Codes & Diagnostics**
| Widget | Description |
|--------|-------------|
| DTCs | Stored diagnostic trouble codes with descriptions |

**System & Vehicle Info**
| Widget | Description |
|--------|-------------|
| System / Vehicle | Battery/module voltage, barometric pressure, VIN, engine specs |

**Recording & Playback**
| Widget | Description |
|--------|-------------|
| Recording Status | Shows current recording state, duration, or replay progress |

**Driving Behavior**
| Widget | Description |
|--------|-------------|
| Driving Behavior | Smoothness score, acceleration history, hard braking and jackrabbit start detection |

### Entering Edit Mode

Press `e` while in Full layout to enter edit mode. An overlay appears showing the current widget layout with a cursor.

### Edit Mode Workflow

```
Browse (cursor on widgets)
  ├── a ──> Category Picker ──> Widget Picker ──> Size Picker ──> Insert
  ├── x ──> Delete widget at cursor
  ├── s ──> Save config to JSON and exit
  └── Esc ── Discard changes and exit
```

**Browsing**: Use `Up` / `Down` to move the cursor between widgets. The current widget is highlighted with `>`.

**Adding a widget** (`a`):
1. **Category Picker** -- Choose from 8 categories. Each shows a description of the widgets it contains. Press `Enter` to pick, `Esc` to go back.
2. **Widget Picker** -- Choose a specific widget from the selected category. Press `Enter` to pick, `Esc` to go back.
3. **Size Picker** -- Choose `Half` (shares a row) or `Full` (takes the entire row). Press `Enter` to confirm.

The new widget is appended to the last row (if Half-size and room permits) or added as a new row. The cursor moves to the new widget.

**Deleting a widget** (`x`): Removes the widget at the cursor position. If a row becomes empty, the row is removed.

**Saving** (`s`): Writes the updated layout to `dashboard.json` (or the path specified by `--config`). The dashboard immediately renders the new layout.

**Canceling** (`Esc`): Discards all changes and returns to the previous layout.

### Widget Sizes

- **Half**: The widget shares its row with other widgets. Up to 3 Half-size widgets can fit in one row.
- **Full**: The widget takes the entire row width.

### Row Heights

Each row has a height type:
- **Fixed**: Exact number of terminal rows (e.g., 12)
- **Min**: Minimum height that grows to fill remaining space (e.g., min 10)
- **Proportional**: Relative weight for distributing space among proportional rows

The default layout uses Min(10) for the top row, Fixed(12) for the middle row, and Fixed(10) for the fuel economy row.

---

## 6. Recording Data

Recording captures all live OBD-II data to a binary file for later analysis or replay.

<img width="1441" height="905" alt="image" src="https://github.com/user-attachments/assets/e9f14f19-e8b7-4346-8ed8-701d8409def1" />


### Starting a Recording

Press `r` during normal operation (live data mode, not during replay). The header changes to show a red `REC` indicator with a running timer:

```
┌─ 2006 MINI Cooper S ─── VIN: WMWRE335... ── REC 00:05 ── 12.8V ───────┐
```

The footer also updates to show `r:stop rec` as a reminder.

### What Gets Recorded

Every message that flows through the app is captured:
- **PID readings**: All standard sensor values with millisecond-precision timestamps and optional raw hex bytes
- **Battery voltage**: Captured every poll cycle
- **Diagnostic trouble codes**: Captured every ~2.5 seconds (10 poll cycles)
- **Enhanced PIDs**: Manufacturer-specific readings captured every ~1.25 seconds (5 poll cycles)
- **O2 monitoring**: O2 sensor test results captured every ~5 seconds (20 poll cycles)

Recording works identically for both real and mock connections.

### Stopping a Recording

Press `r` again. The recording is finalized:
1. Buffered data is flushed to disk
2. A session entry is added to `recordings/sessions.json`
3. Storage maintenance runs (compression/trimming if needed)

The header returns to showing the connection status.

### Storage Management

Recording files are stored in the `recordings/` directory (configurable via `--recordings-dir`).

| Setting | Default | Description |
|---------|---------|-------------|
| Compress threshold | 50 MB | Raw files larger than this are gzip-compressed after recording stops |
| Max total storage | 500 MB | When total storage exceeds this, the oldest sessions are deleted |

Adjust the storage limit at launch:

```bash
cargo run -- --mock --max-storage-mb 1000
```

### File Format

Each session produces a `.obd2rec` file (or `.obd2rec.gz` when compressed):

```
[OBD2REC\x02]           8 bytes magic (v2 format)
[u32 header_length]     4 bytes
[JSON SessionHeader]    Variable (session ID, start time, VIN, vehicle name, poll interval)
[frame][frame][frame]   Variable-length frames:
                          PID:      14 bytes + optional raw hex
                          Voltage:  13 bytes
                          DTC:      14 bytes
                          Enhanced: variable (metadata + value)
                          O2:       variable (metadata + value)
```

At 25 PIDs polled at 4 Hz, raw data is approximately 5 MB/hour. The v1 format (14-byte fixed frames without raw bytes or extended types) is still readable for backward compatibility.

---

## 7. Replaying Sessions

### Opening the Session Picker

Press `R` (capital R) when the app is idle (not recording or already replaying). A popup appears listing all recorded sessions:

```
┌══ SELECT RECORDING ══════════════════════════════════════════┐
│  Date/Time              Vehicle          Duration            │
│> 2026-02-21 10:30:00   Mini Cooper S     1h 23m             │
│  2026-02-20 15:45:00   Mini Cooper S     0h 45m             │
│  Total: 3 sessions, 47.2 MB                                 │
│  Enter: play  |  d: delete  |  Esc: cancel                  │
└══════════════════════════════════════════════════════════════┘
```

Use `Up` / `Down` to browse sessions, `Enter` to start playback, `d` to delete a session, or `Esc` to cancel.

### During Replay

Once a session starts playing, the app behaves as if receiving live data. All gauges, sparklines, and panels update with the recorded values. The header shows the replay state:

```
┌─ 2006 MINI Cooper S ─── VIN: WMWRE335... ── REPLAY 1x ── 12.8V ───────┐
```

The footer shows a progress indicator and controls:

```
┌ 12:34 / 1:23:00 | [/]:seek | Space:pause | s:speed | Esc:stop ────────┐
```

### Playback Controls

| Key | Action |
|-----|--------|
| `Space` | Pause or resume playback |
| `[` | Seek backward 30 seconds |
| `]` | Seek forward 30 seconds |
| `s` | Cycle playback speed: 0.5x -> 1x -> 2x -> 4x -> 0.5x |
| `Esc` | Stop replay and return to live mode |

When playback reaches the end of the recording, it automatically returns to idle mode.

### How Replay Works

Replay feeds recorded frames back through the same message pipeline as live data. This means:
- All threshold alerts fire normally on replayed data
- Fuel economy calculations work on replayed data
- The dashboard renders identically to how it looked during the original session

The only difference is that the OBD-II polling task is suppressed during replay -- all data comes from the recording file.

---

## 8. Fuel Economy Display

The Fuel Economy panel (shown in Full layout) provides two independent MPG calculations side by side.

### ECU Gold Standard (Left Side)

This uses the most accurate data source available from the ECU:

1. **Direct Fuel Rate** (PID 0x5E) -- preferred, most accurate
2. **MAF-derived** (PID 0x10) -- fallback using: `MAF / (AFR x fuel_density) x 3.6 = L/h`

Displayed values:
- **Source**: Which data source is active
- **Instant**: Current fuel economy in MPG
- **Average**: Trip average MPG (accumulated since app start)
- **Rate**: Current fuel consumption in L/h

A sparkline below shows the recent history of instant MPG readings.

### Calculated Advanced (Right Side)

This uses the speed-density algorithm based on the ideal gas law, independent of ECU fuel rate reporting:

```
Base MAF = (MAP x VE x displacement x RPM) / (2 x 60 x R_air x IAT_K) x 1000
```

**Seven correction factors** adjust the base calculation in real time:

| Factor | Range | Description |
|--------|-------|-------------|
| Cold Engine | 1.0--1.2 | Enrichment during warmup (based on coolant temp) |
| Altitude | 0.7--1.3 | Barometric pressure vs. standard atmosphere |
| Air Density | 0.9--1.1 | Temperature-based air density variation |
| Fuel Trims | 0.8--1.2 | ECU's STFT/LTFT corrections averaged |
| Catalyst Warmup | 1.0--1.1 | Extra fuel during catalyst light-off |
| Throttle Transient | 1.0--1.15 | Acceleration enrichment penalty |
| High Load/WOT | 1.0--1.2 | Rich mixture at high load (>85%) |

The corrections panel shows each factor's current value. Green = near 1.0, yellow = moderate deviation, red = significant deviation. The "TOT" value is the multiplicative product of all factors.

### Fuel Types

The calculation adapts to the vehicle's fuel type:

| Fuel | Air-Fuel Ratio | Density (kg/L) |
|------|---------------|-----------------|
| Gasoline | 14.7:1 | 0.745 |
| Diesel | 14.5:1 | 0.832 |
| E85 | 9.765:1 | 0.785 |

Fuel type is determined from the vehicle database. If unknown, gasoline is assumed.

---

## 9. DTC Diagnostics

### Viewing Trouble Codes

The DTCs panel in Full layout shows all stored diagnostic trouble codes. When codes are present, the panel border turns yellow (1--2 codes) or red (3+ codes), and the count appears in the title.

In mock mode, press `d` to cycle through DTC scenarios:
- **Scenario 0**: No codes
- **Scenario 1**: 2 codes (P0420 catalyst, P0171 lean condition)
- **Scenario 2**: 5 codes (complex multi-system scenario)

### DTC Detail Popup

Focus the DTCs panel (Tab), select a code (Up/Down), and press Enter. The popup provides a complete diagnostic workup:

**1. Code Information**
```
Code: P0420
Category: Powertrain
Catalyst System Efficiency Below Threshold (Bank 1)
```

**2. Related Sensors**
Live readings from PIDs correlated to the specific code, with threshold status:
```
Related Sensors:
  Catalyst Temp B1S1: 412.0°C [OK]
  Catalyst Temp B1S2: 380.5°C [OK]
  Engine Load:         42.1%   [OK]
  Short Fuel Trim B1:  +2.3%  [OK]
```

**3. Other Active DTCs**
Any other stored codes that may indicate related failures:
```
Other Active DTCs:
  P0171 - System Too Lean (Bank 1)
```

**4. Common Causes**
The 5 most likely root causes for the specific code:
```
Common Causes:
  1. Failing catalytic converter
  2. Exhaust leak before catalyst
  3. Engine misfire damaging catalyst
  4. Contaminated catalyst (coolant/oil)
  5. Faulty O2 sensor (downstream)
```

**5. Suggested Actions**
Step-by-step diagnostic procedure:
```
Suggested Actions:
  1. Check for exhaust leaks
  2. Inspect O2 sensor waveforms
  3. Monitor catalyst temperature differential
  4. Check for engine misfires
  5. Inspect catalyst for physical damage
```

### Freeze-Frame Data

When you open a DTC detail popup (Enter on a selected code), the app automatically requests freeze-frame data (Mode $02) from the vehicle. If freeze-frame data is available, a "Freeze-Frame Snapshot" section appears at the bottom of the popup showing the sensor values that were captured when the DTC was set:

```
Freeze-Frame Snapshot:
  Engine RPM: 2847.0 rpm
  Vehicle Speed: 59.0 km/h
  Coolant Temp: 92.1 °C
  Engine Load: 42.1 %
```

Not all vehicles store freeze-frame data. If no data is available, this section is omitted.

### Clearing DTCs

Press `C` (capital C) while the DTC panel is focused to clear diagnostic trouble codes:

- **No DTC selected**: A confirmation popup appears: "Clear all DTCs? This resets readiness monitors." Press Enter to confirm or Esc to cancel.
- **DTC selected**: If the selected DTC has a source module, a two-key confirmation activates — press `C` again within 2 seconds to clear DTCs on that specific module only.

Clearing DTCs resets the vehicle's readiness monitors. After clearing, the readiness panel will show monitors as incomplete until the vehicle completes its drive cycles.

### Readiness Monitors

Add the "Readiness Monitors" widget via edit mode (under Diagnostics category) to see:

- **MIL status**: Malfunction Indicator Light on/off (red if on, green if off)
- **DTC count**: Number of stored codes as reported by the ECU
- **Ignition type**: Spark (gasoline) or Diesel
- **Monitor status**: Per-monitor supported/complete state with color coding (green = complete, yellow = incomplete)

Readiness data is polled every 5 seconds. Monitors reset when DTCs are cleared and complete progressively during normal driving.

### DTC Color Coding

| Color | Meaning |
|-------|---------|
| Yellow | Powertrain codes (most common) |
| Red | Misfire codes (P03xx), transmission codes (P07xx), chassis/body/network codes |

---

## 10. Threshold Alerts

### How Alerts Work

Each PID can have configured warning and critical thresholds. When a reading crosses a threshold, the gauge color changes and an alert appears in the footer bar.

| Level | Gauge Color | Border Color | Description |
|-------|-------------|--------------|-------------|
| Normal | Green | Gray | Within all thresholds |
| Warning | Yellow | Yellow | Exceeded warning threshold |
| Critical | Red | Red | Exceeded critical threshold |

Alerts are shown as a row in the footer:
```
┌ COOLANT TEMP HIGH: 118.2°C (CRITICAL)  ENGINE RPM HIGH: 5800 (WARNING) ─┐
```

### Threshold Resolution

Thresholds come from the SQLite database and follow a priority chain:

1. **VIN-specific overrides** (highest priority) -- thresholds tied to a specific vehicle
2. **Engine family overrides** -- thresholds for the engine family (e.g., W11B16 has a different RPM redline than LLY)
3. **Default thresholds** (lowest priority) -- universal defaults for all vehicles

For example, the RPM thresholds differ by engine:

| Engine | Warning | Critical |
|--------|---------|----------|
| Default | 5500 | 6500 |
| W11B16 (Mini) | 6000 | 6800 |
| LLY (Duramax) | 2800 | 3200 |

### Default Thresholds

Some key defaults out of the box:

| PID | Low Warn | Low Crit | High Warn | High Crit |
|-----|----------|----------|-----------|-----------|
| Coolant Temp | -- | -- | 105°C | 115°C |
| Engine RPM | -- | -- | 5500 | 6500 |
| Engine Load | -- | -- | 85% | 95% |
| Oil Temp | -- | -- | 120°C | 140°C |
| Fuel Trim B1 | -15% | -25% | +15% | +25% |

---

## 11. Vehicle Profiles & Database

### Pre-Configured Vehicles

**2006 MINI Cooper S**
- VIN: `WMWRE33546T000001`
- Engine: W11B16 -- 1.6L Supercharged I4
- Transmission: Manual 6-speed
- Drive: FWD
- Fuel: Gasoline
- Power: 125 kW / 220 Nm
- Compression: 8.3:1

**2004 Chevy Silverado 2500HD**
- VIN: `1GCHK23164F000001`
- Engine: LLY -- 6.6L Turbocharged V8 (Duramax)
- Transmission: Allison 1000 5-speed auto
- Drive: 4WD
- Fuel: Diesel
- Power: 224 kW / 890 Nm
- Compression: 17.5:1

### Database Location

The SQLite database is created at `obd2-dash.db` by default. Change the path with `--db-path`:

```bash
cargo run -- --mock --db-path /path/to/my-vehicles.db
```

The database is automatically seeded with reference data on first creation. Subsequent launches reuse the existing database.

---

## 12. Units & Display Options

Press `u` to toggle between metric and imperial units. This affects:

| Metric | Imperial |
|--------|----------|
| km/h | mph |
| °C | °F |

All other units (RPM, kPa, %, g/s, L/h, V) remain unchanged regardless of the unit setting.

MPG in the Fuel Economy panel is always displayed in US miles per gallon.

---

## 13. Headless Mode

Headless mode outputs sensor data to stdout in a simple text format, without the TUI. It is activated automatically when stdout is not a terminal (e.g., when piped), or manually with `--headless`:

```bash
# Explicit headless mode
cargo run -- --mock --headless

# Auto-headless when piped
cargo run -- --mock | tee vehicle_log.txt
```

Output format (printed every 500ms):
```
obd2-dash headless mode — 2006 MINI Cooper S
-------------------------------------------
[   1] RPM:   850  Speed:     0 km/h  Coolant:  45.2°C  Load:  22.1%  Batt: 12.8V  [connected]
[   2] RPM:  1200  Speed:    15 km/h  Coolant:  48.7°C  Load:  35.6%  Batt: 12.7V  [connected]
```

Headless mode does not support widget customization, recording, or replay.

---

## 14. Driving Behavior

The Driving Behavior widget tracks your driving style in real time by analyzing speed changes and throttle input.

### Metrics

| Metric | Description |
|--------|-------------|
| Smoothness Score | 0--100 score based on acceleration variance over the last ~7.5 seconds. 100 = perfectly smooth |
| Current Acceleration | Instantaneous acceleration/deceleration in m/s², derived from consecutive speed readings |
| Hard Brake Count | Number of deceleration events exceeding 2.8 m/s² (~0.29g) |
| Jackrabbit Starts | Number of acceleration events exceeding 2.8 m/s² while throttle >65% and speed <50 km/h |

### Acceleration History

The widget shows a rolling sparkline of recent acceleration values, giving a visual sense of driving smoothness.

Add this widget via edit mode under the "Driving Behavior" category.

---

## 15. Debug Log Viewer

Press `l` to open the in-app debug log viewer. This displays the last 2000 log lines from the `tracing` system in a scrollable overlay, useful for diagnosing connection issues or inspecting OBD-II command exchanges without leaving the dashboard.

- `Up` / `Down` to scroll
- `Home` to jump to the oldest entry
- `End` to jump to the newest (auto-scroll)
- `l` or `Esc` to close

The log viewer shows the same data written to the `logs/` directory, but accessible in real time without switching terminals.

---

## 16. Raw Protocol Capture

Raw protocol capture records the hex-level ELM327/adapter traffic to a `.obd2raw` sidecar file, useful for offline protocol analysis, debugging adapter behavior, or contributing to protocol reverse-engineering efforts.

### Toggling Capture

Press `c` during normal operation (when connected) to start raw capture. The status bar shows a `RAW` indicator while capture is active. Press `c` again to stop.

### What Gets Captured

The `.obd2raw` file contains the raw hex bytes exchanged between the adapter and the vehicle's OBD-II bus, including:
- ELM327 AT command responses
- Raw OBD-II request/response frames
- Protocol negotiation traffic
- Timestamps for each exchange

### File Location

Raw capture files are stored alongside recordings in the `recordings/` directory with the naming pattern `{session_id}.obd2raw`. They are tracked by the storage manager and counted toward the total storage quota.

### Storage Management

Raw capture files are included in the storage manager's quota calculations. When total storage (recordings + raw captures) exceeds the `--max-storage-mb` limit, the oldest sessions and their associated `.obd2raw` sidecars are trimmed together.

---

## 17. Configuration Files

### Dashboard Layout (`dashboard.json`)

Stores the widget grid configuration as JSON. Created when you save from edit mode, or you can create it manually. Example:

```json
{
  "version": 1,
  "rows": [
    {
      "widgets": [
        { "kind": "GaugesAndEngine", "size": "Half" },
        { "kind": "TemperaturesPanel", "size": "Half" }
      ],
      "height": { "Min": 10 }
    },
    {
      "widgets": [
        { "kind": "FuelSystemPanel", "size": "Half" },
        { "kind": "SystemInfoPanel", "size": "Half" },
        { "kind": "DtcPanel", "size": "Half" }
      ],
      "height": { "Fixed": 12 }
    },
    {
      "widgets": [
        { "kind": "FuelEconomyPanel", "size": "Full" }
      ],
      "height": { "Fixed": 10 }
    }
  ]
}
```

**WidgetKind values**: `GaugesAndEngine`, `TemperaturesPanel`, `FuelSystemPanel`, `SystemInfoPanel`, `DtcPanel`, `FuelEconomyPanel`, `EngineRpmGauge`, `VehicleSpeedGauge`, `EngineLoadGauge`, `ThrottleGauge`, `IntakeMapDisplay`, `MafDisplay`, `FuelPressureDisplay`, `BoostPressureDisplay`, `OilPressureDisplay`, `FuelTankLevel`, `EngineFuelRate`, `FuelTrimBank1`, `FuelTrimBank2`, `CoolantTemp`, `OilTemp`, `TransmissionTemp`, `IntakeAirTemp`, `AmbientAirTemp`, `CatalystTemps`, `RecordingStatus`, `DrivingBehavior`

**WidgetSize values**: `Half`, `Full`

**RowHeight values**: `{"Fixed": N}`, `{"Min": N}`, `{"Proportional": N}`

If the config file is missing or invalid, the default 6-panel layout is used.

### Session Index (`recordings/sessions.json`)

Automatically maintained by the recording system. Each entry tracks:

```json
{
  "session_id": "a1b2c3d4-...",
  "start_time": "2026-02-21T10:30:00Z",
  "vin": "WMWRE33546T000001",
  "vehicle_name": "2006 MINI Cooper S",
  "duration_secs": 4980,
  "frame_count": 124500,
  "file_path": "recordings/a1b2c3d4-....obd2rec",
  "file_size_bytes": 1743000,
  "compressed": false
}
```

### Connection Preferences (`connection.json`)

Automatically saved when you connect to a device via the scanner. Stores the last-used device (serial port path + baud, or BLE adapter name) so the scanner can highlight it on next launch.

### Log Files (`logs/`)

Daily rolling log files named `obd2-dash.log.YYYY-MM-DD`. Control the log level with the `RUST_LOG` environment variable:

```bash
# Debug-level logging
RUST_LOG=debug cargo run -- --mock

# Only warnings and errors
RUST_LOG=warn cargo run -- --mock
```

---

## 18. Keyboard Reference

### Normal Mode (Live Data)

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Esc` | Dismiss popup / deselect / unfocus / quit |
| `Ctrl+C` | Quit |
| `f` | Toggle Compact / Full layout |
| `p` | Pause / resume data updates |
| `u` | Toggle metric / imperial units |
| `d` | Cycle DTC scenarios (mock mode only) |
| `+` / `=` | Increase poll rate (faster, -50ms) |
| `-` | Decrease poll rate (slower, +50ms) |
| `Tab` | Focus next widget |
| `Shift+Tab` | Focus previous widget |
| `Up` | Select previous item in focused widget |
| `Down` | Select next item in focused widget |
| `Enter` | Open detail popup for selected item |
| `e` | Enter edit mode (Full layout only) |
| `r` | Toggle recording on/off |
| `c` | Toggle raw protocol capture on/off |
| `C` | Clear DTCs (popup confirmation or two-key per-module) |
| `R` | Open session picker for replay |
| `s` / `S` | Open device scanner/picker |
| `l` | Open debug log viewer |

### Debug Log Viewer

| Key | Action |
|-----|--------|
| `Up` / `Down` | Scroll log lines |
| `Home` | Scroll to top |
| `End` | Scroll to bottom (auto-scroll) |
| `l` / `Esc` | Close log viewer |
| `q` | Quit application |

### Device Scanner

| Key | Action |
|-----|--------|
| `Up` / `Down` | Browse discovered devices |
| `Enter` | Connect to selected device |
| `Esc` | Close scanner |

### Edit Mode

| Key | Action |
|-----|--------|
| `Up` / `Down` | Move cursor / navigate picker |
| `a` | Start add-widget flow |
| `x` | Delete widget at cursor |
| `s` | Save config and exit edit mode |
| `Enter` | Confirm selection in picker |
| `Esc` | Go back one step / cancel edit mode |

### Replay Mode

| Key | Action |
|-----|--------|
| `Space` | Pause / resume playback |
| `[` | Seek backward 30 seconds |
| `]` | Seek forward 30 seconds |
| `s` | Cycle speed (0.5x -> 1x -> 2x -> 4x) |
| `Esc` | Stop replay, return to live |
| `q` | Quit application |

### Session Picker

| Key | Action |
|-----|--------|
| `Up` / `Down` | Browse sessions |
| `Enter` | Start replay of selected session |
| `d` | Delete selected session |
| `Esc` | Close picker |

---

## 19. CLI Reference

```
obd2-dash — OBD2 vehicle diagnostics TUI dashboard

USAGE:
    obd2-dash [OPTIONS]

OPTIONS:
    -p, --port <PORT>
            Serial port path (e.g. /dev/ttyUSB0, /dev/cu.usbserial-*)

    -b, --baud <BAUD>
            Baud rate for serial connection [default: 115200]

        --ble
            Connect via Bluetooth LE instead of serial

        --ble-name <NAME>
            BLE adapter name filter (e.g. "OBDLink")

        --ble-scan-secs <SECS>
            BLE scan timeout in seconds [default: 5]

        --mock
            Use mock data instead of a real OBD2 adapter

        --mock-vehicle <PROFILE>
            Mock vehicle profile: mini, chevy, or generic [default: generic]

        --poll-ms <MS>
            Polling interval in milliseconds [default: 250]

        --db-path <PATH>
            SQLite database path [default: obd2-dash.db]

        --headless
            Run in headless mode (no TUI, prints to stdout).
            Auto-enabled when stdout is not a TTY.

        --config <PATH>
            Dashboard config JSON path [default: dashboard.json]

        --recordings-dir <PATH>
            Recordings directory path [default: recordings]

        --max-storage-mb <MB>
            Max recording storage in MB [default: 500]

    -h, --help
            Print help information
```

---

## 20. Troubleshooting

### "No serial ports found"

No USB serial devices were detected. Check that:
- Your ELM327 adapter is plugged in
- The USB cable is working
- On macOS, check `ls /dev/cu.usbserial-*`
- On Linux, check `ls /dev/ttyUSB*` and ensure your user is in the `dialout` group

Use `--mock` to run without hardware.

### "Failed to open serial port"

The port exists but can't be opened. Common causes:
- Another application has the port open (close it first)
- Permissions: on Linux, add your user to the `dialout` group (`sudo usermod -aG dialout $USER`, then log out/in)
- Wrong baud rate: try `--baud 9600` or `--baud 115200`

### BLE adapter not found

If `--ble` doesn't discover your adapter:
- Ensure Bluetooth is enabled on your system
- On macOS, grant Bluetooth permission to the terminal app (System Settings > Privacy & Security > Bluetooth)
- Try `--ble-scan-secs 15` for a longer scan window
- Use `--ble-name` with part of your adapter's name (e.g., `--ble-name "OBDLink"`)
- Ensure the adapter is powered on and not connected to another device

### Dashboard shows "--" for all values

- If using a real adapter: the vehicle ignition may be off, or the adapter failed to initialize. Check the connection status in the header (should say "Connected").
- If using mock mode: values should appear within 1-2 seconds. Check the log file for errors.

### Gauges not updating / "PAUSED" shown

You may have pressed `p` to pause. Press `p` again to resume.

### Recording files not appearing

Ensure the recordings directory is writable. By default it's `recordings/` relative to where you run the binary. Use `--recordings-dir` to specify an absolute path.

### Replay shows no data

- The recording file may be corrupted or empty (very short session)
- Check the session duration in the picker -- 0-second recordings have no data
- Check the log file for "Failed to read recording" errors

### Dashboard layout reset after restart

The layout is only saved when you press `s` in edit mode. If you press `Esc` to exit edit mode, changes are discarded. Make sure to press `s` to persist.

### High CPU usage

Reduce the poll rate with `-` (or `--poll-ms 500` at launch). The default 250ms (4 Hz) is reasonable for most use cases.

### Log file location

Logs are written to `logs/obd2-dash.log` in the working directory. For more verbose output:

```bash
RUST_LOG=debug cargo run -- --mock
```
