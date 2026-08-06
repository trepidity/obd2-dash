# obd2-dash

A real-time OBD-II vehicle diagnostics TUI dashboard built with Rust. Connects to an ELM327 adapter over serial or Bluetooth LE, or runs in mock mode with realistic vehicle simulations. Features a customizable widget-based dashboard, data recording/replay, dual fuel economy calculations, driving behavior analysis, and DTC diagnostic analysis.

```
 ┌─ 2006 MINI Cooper S ─── VIN: WMWRE335... ── W11B16 ── Connected ── 12.8V ─┐
 └────────────────────────────────────────────────────────────────────────────┘
 ┌═══ GAUGES + ENGINE ══════════════════════┐┌═══ TEMPERATURES ═══════════════┐
 │ ▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░ 2847 rpm          ││ Coolant      92.1°C           │
 │ ▓▓▓▓▓▓▓▓░░░░░░░░░░░░ 59 km/h           ││ Oil          87.3°C           │
 │ Load   42.1%   Thrtl  35.2%             ││ Trans        78.5°C           │
 │ MAP    65 kPa  MAF    12.3 g/s          ││ Cat B1S1    412.0°C           │
 └──────────────────────────────────────────┘└───────────────────────────────┘
 ┌── FUEL SYSTEM ─────┐┌── SYSTEM / VEHICLE ──┐┌── DTCs (2) ────────────────┐
 │ Tank    74.2%      ││ Batt Voltage  12.8V  ││ P0420  Catalyst system ... │
 │ STFT B1  +2.3%    ││ VIN  WMWRE335...     ││ P0171  System too lean ... │
 │ LTFT B1  +1.1%    ││ Engine  W11B16 ...   ││                            │
 └────────────────────┘└──────────────────────┘└────────────────────────────┘
 ┌══ FUEL ECONOMY ═══════════════════════════════════════════════════════════┐
 │ ECU (Gold Standard)          │ Calculated (Advanced)                     │
 │  Source:  Direct Fuel Rate   │  Method:   Speed-Density                 │
 │  Instant: 28.3 MPG          │  Instant:  27.1 MPG                      │
 │  Average: 26.7 MPG          │  Average:  25.9 MPG                      │
 └═══════════════════════════════════════════════════════════════════════════┘
 ┌ Status: OK │ Poll: 250ms │ f: compact │ e:edit │ r:rec R:replay ────────┐
 └──────────────────────────────────────────────────────────────────────────┘
```

<img width="1396" height="769" alt="image" src="https://github.com/user-attachments/assets/87daa0bb-94b4-49be-9b30-0e32380eea56" />

<img width="1396" height="769" alt="image" src="https://github.com/user-attachments/assets/db7b8d58-7dbb-4fdb-91cc-8b6525a3ef09" />

<img width="1424" height="905" alt="image" src="https://github.com/user-attachments/assets/5700c4f3-b5d9-4587-8f96-03e42f8bc76b" />

## Features

- **25+ OBD-II PIDs**: RPM, speed, coolant temp, engine load, fuel trims (all 4 banks), MAF, MAP, throttle, fuel pressure, catalyst temps (4 sensors), oil temp/pressure, transmission temp, fuel level/rate, voltages, torque, EGR/EVAP, and more
- **Enhanced (manufacturer-specific) PIDs**: Discovery-driven module resolution reads manufacturer-specific DIDs from identified ECU modules
- **O2 sensor monitoring**: Periodic O2 sensor test result collection from all available sensors
- **Customizable widget dashboard**: 27 widget types across 8 categories, configurable grid layout with JSON persistence, real-time edit mode to add/remove/resize widgets
- **Two layouts**: Compact (4-gauge view) and Full (configurable widget grid)
- **Data recording**: Binary recording format captures all PID, voltage, DTC, enhanced PID, and O2 monitoring data to disk with automatic gzip compression and storage management
- **Raw protocol capture**: Toggle hex-level protocol capture (`c` key) to `.obd2raw` sidecar files for offline protocol analysis
- **Session replay**: Play back recorded sessions with adjustable speed (0.5x--4x), seek, and pause controls through a session picker UI
- **Dual fuel economy**: ECU gold-standard MPG alongside speed-density calculated MPG with 7 real-time correction factors
- **Threshold alerts**: Vehicle-specific and engine-family-specific warning/critical thresholds with color-coded gauges, loaded from a SQLite database
- **DTC diagnostics**: Reads stored, pending, and permanent trouble codes via `read_all_dtcs()` with contextual analysis -- correlated sensor snapshots, other active DTCs, common causes, suggested repair actions, and on-demand freeze-frame data
- **Clear DTCs**: Clear all DTCs (`C` key) with popup confirmation, or clear per-module with two-key confirmation
- **Readiness monitors**: MIL status, DTC count, and per-monitor supported/complete state polled every 5 seconds
- **Sparkline histories**: Rolling 30-second trend graphs for RPM, speed, throttle, and load
- **Interactive panels**: Tab between widgets, arrow-key select items, Enter for detail popups
- **Mock mode**: Built-in vehicle simulator with realistic drive patterns, warmup cycles, and DTC scenarios for demo/development without hardware
- **Vehicle profiles**: Pre-configured profiles for a 2006 MINI Cooper S (W11B16) and 2004 Chevy 2500HD Duramax (LLY) with engine-family-specific thresholds
- **BLE connectivity**: Connect to ELM327/STN adapters over Bluetooth Low Energy in addition to USB serial, with a built-in device scanner/picker
- **Driving behavior**: Real-time smoothness scoring, hard brake detection, and jackrabbit start tracking
- **Debug log viewer**: In-app scrollable log overlay (`l` key) backed by a 2000-line ring buffer
- **Headless mode**: Non-interactive stdout output when piped or with `--headless`
- **File logging**: Structured logs via `tracing` written to `logs/` (stdout is reserved for the TUI)

## Requirements

- Rust 1.70+ (edition 2021)
- For real hardware: an ELM327-compatible OBD-II adapter connected via USB serial or Bluetooth LE

## Building

```bash
cd obd2-dash
cargo build --release
```

## Quick Start

```bash
# Mock mode -- no hardware needed
cargo run -- --mock

# Mock with a specific vehicle profile
cargo run -- --mock --mock-vehicle mini
cargo run -- --mock --mock-vehicle chevy

# Real ELM327 adapter (auto-detect port)
cargo run

# Specify serial port
cargo run -- --port /dev/cu.usbserial-0001 --baud 115200

# Connect via Bluetooth LE
cargo run -- --ble
```

See [MANUAL.md](MANUAL.md) for the full user guide.

## CLI Options

```
Options:
  -p, --port <PORT>                Serial port path (e.g. /dev/ttyUSB0)
  -b, --baud <BAUD>               Baud rate [default: 115200]
      --ble                        Connect via Bluetooth LE instead of serial
      --ble-name <NAME>            BLE adapter name filter
      --ble-scan-secs <SECS>       BLE scan timeout in seconds [default: 5]
      --mock                       Use mock data instead of a real adapter
      --mock-vehicle <PROFILE>     Mock vehicle: mini, chevy, or generic [default: generic]
      --poll-ms <MS>               Polling interval in milliseconds [default: 250]
      --db-path <PATH>             SQLite database path [default: obd2-dash.db]
      --headless                   Non-interactive stdout mode
      --config <PATH>              Dashboard layout config JSON [default: dashboard.json]
      --recordings-dir <PATH>      Directory for recorded sessions [default: recordings]
      --max-storage-mb <MB>        Max recording storage in MB [default: 500]
```

For protocol development, run a headless raw capture until you press Ctrl-C:

```bash
cargo run -p obd2-dash -- --port /dev/ttyUSB0 record raw
cargo run -p obd2-dash -- --ble --ble-name "OBDLink CX" record raw --output raw-captures/my-vehicle.obd2raw
```

The capture contains the adapter request/response stream needed to develop new vehicle profiles and decoders.

## Keyboard Controls

### Normal Mode

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit (or dismiss popup / deselect / unfocus) |
| `Ctrl+C` | Quit |
| `f` | Toggle Compact / Full layout |
| `p` | Pause / resume data display |
| `u` | Toggle units (metric / imperial) |
| `d` | Cycle DTC scenarios (mock mode) |
| `+` / `-` | Increase / decrease poll rate (50ms steps) |
| `Tab` / `Shift+Tab` | Focus next / previous widget (Full layout) |
| `Up` / `Down` | Select items within focused widget |
| `Enter` | Open detail popup for selected item |
| `e` | Enter edit mode (Full layout) |
| `r` | Toggle recording on/off |
| `c` | Toggle raw protocol capture on/off |
| `C` | Clear DTCs (popup or two-key confirmation) |
| `R` | Open session picker for replay |
| `s` / `S` | Open device scanner/picker |
| `l` | Open debug log viewer |

### Edit Mode

| Key | Action |
|-----|--------|
| `a` | Add a new widget (opens category picker) |
| `x` | Delete widget at cursor |
| `s` | Save layout to JSON and exit edit mode |
| `Up` / `Down` | Navigate widgets or picker items |
| `Enter` | Confirm selection in pickers |
| `Esc` | Go back one step, or cancel and exit edit mode |

### Replay Mode

| Key | Action |
|-----|--------|
| `Space` | Pause / resume playback |
| `[` | Seek backward 30 seconds |
| `]` | Seek forward 30 seconds |
| `s` | Cycle playback speed (0.5x / 1x / 2x / 4x) |
| `Esc` | Stop replay, return to live |

## Architecture

```
crates/obd2-dash/src/
├── main.rs              # CLI, runtime wiring, transport setup, TUI event loop, replay loop
├── session_runner.rs    # Shared obd2-core session bootstrap and live polling orchestration
├── app.rs               # AppState, Message enum, update() integration boundary
├── domain.rs            # Vehicle state, connection/discovery state, reducer logic
├── scanner.rs           # Device discovery for serial ports and BLE adapters
├── connection_prefs.rs  # Persisted last-used device preference
├── debug_log.rs         # LogBuffer ring buffer, tracing layer for in-app log viewer
├── mock_profile.rs      # Mock VIN/profile helpers for the obd2-core mock adapter
├── vehicle_data.rs      # Canonical vehicle telemetry model used by widgets and analysis
├── analysis/
│   ├── mod.rs           # Analysis exports
│   ├── driving.rs       # DrivingBehavior — smoothness score, hard brake/jackrabbit detection
│   └── fuel_economy.rs  # Dual fuel economy: ECU + calculated methods
├── tui/
│   ├── mod.rs           # Terminal setup/teardown
│   ├── event.rs         # Async key/tick/render event handler
│   ├── ui.rs            # All rendering (compact, full, overlays, footer)
│   └── panel.rs         # Panel grid, item models, popup builder
├── widget/
│   ├── mod.rs           # WidgetKind, WidgetCategory, WidgetMeta, registry
│   ├── config.rs        # DashboardConfig — rows, slots, JSON load/save
│   ├── renderers.rs     # render_widget() dispatcher + individual renderers
│   └── edit_mode.rs     # Edit mode state machine (Browse/Category/Widget/Size)
├── recording/
│   ├── mod.rs           # RecordingState enum (Idle/Recording/Replaying)
│   ├── format.rs        # Binary frame format and frame codecs
│   ├── writer.rs        # RecordingWriter — append-only binary file
│   ├── reader.rs        # Frame reader (raw + gzip)
│   ├── index.rs         # Session index and metadata
│   ├── storage.rs       # StorageManager — compress, trim, quota enforcement
│   └── replay.rs        # ReplayController — speed, seek, pause
├── ai/
│   ├── mod.rs           # AI module exports
│   ├── client.rs        # LLM client wiring
│   ├── config.rs        # AI configuration
│   ├── insights.rs      # Text insight parsing/helpers
│   ├── prompt.rs        # Prompt construction
│   └── summary.rs       # Recording summarization pipeline
└── nhtsa.rs             # NHTSA vehicle/threshold enrichment helpers
```

### Design patterns

- **TEA-style state flow**: `AppState` is the single source of truth, `Message` carries live/replay/session events, and `update()` folds them into domain state
- **Session-first integration**: `obd2-dash` talks to `obd2_core::session::Session`; `session_runner` owns adapter/session bootstrap, discovery emission, standard polling, enhanced polling cadence, DTC aggregation, and raw capture control
- **Transport setup at the edge**: `main.rs` is responsible for selecting serial, BLE, emulator, or mock transports and then handing an initialized `Session` boundary to `session_runner`
- **Widget-config-driven rendering**: `DashboardConfig` (JSON) drives the full layout dynamically -- the rendering loop iterates config rows/slots rather than hardcoded panels
- **Recording interception**: Data capture sits at the top of `update()`, passively recording every PID/Voltage/DTC/enhanced/O2 message that flows through the app
- **Replay injection**: Playback feeds frames back into the same `Message` pipeline via a `tokio::select!` arm, so replayed data follows the exact same code path as live data
- **Rich connection/discovery state**: The domain preserves `obd2-core` connection states such as protocol negotiation, ignition-off, and unsupported protocol, plus discovery metadata for the current vehicle/modules
- **Layered threshold resolution**: Default thresholds -> engine family overrides -> VIN-specific overrides, resolved at startup from SQLite

## Database

The app creates a SQLite database (`obd2-dash.db` by default) on first run, seeded with:

- 2 engine families (W11B16, LLY) with operating characteristics
- 2 vehicle records with VIN, year/make/model, engine linkage
- 25 default PID thresholds with warning/critical ranges
- Engine-family-specific threshold overrides (RPM redline, coolant ranges)

Threshold resolution follows a priority chain: VIN-specific > engine family > default.

## Recording Format

Recorded sessions use a compact binary format (v2):

- **Magic**: `OBD2REC\x02` (8 bytes)
- **Header**: JSON `SessionHeader` (session ID, start time, VIN, vehicle name, poll interval)
- **Frames**: Variable length per type:
  - **PID** (type 0x00): 14 bytes fixed + optional raw hex bytes (v2)
  - **Voltage** (type 0x01): 13 bytes
  - **DTC** (type 0x02): 14 bytes (5-char code packed into `u8 + f64`)
  - **Enhanced** (type 0x03): variable (module/name/unit metadata + f64 value)
  - **O2** (type 0x04): variable (test_name/sensor/unit metadata + f64 value)

At 25 PIDs polled at 4 Hz, this produces approximately 5 MB/hour of raw data. Files larger than 50 MB are automatically gzip-compressed on session end. The v1 format (without raw bytes or extended frame types) is still readable for backward compatibility.

## Testing

```bash
cargo test
```

98 tests covering domain state transitions (connection, discovery, readiness, freeze-frame), poll-event translation, discovery-driven enhanced planning, recording format roundtrips, replay controller seek/pause/speed, vehicle data model, fuel economy calculations (gold standard, speed-density, correction factors), driving behavior analysis (hard braking, jackrabbit detection, smoothness scoring), NHTSA parsing, database threshold resolution, SessionIndex/StorageManager filesystem operations, and ConnectionPrefs serialization.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime |
| `ratatui` / `crossterm` | Terminal UI framework |
| `obd2-core` | OBD-II session, poller, discovery, and protocol layer |
| `tokio-serial` / `serialport` | Serial port communication |
| `btleplug` | Bluetooth Low Energy communication |
| `clap` | CLI argument parsing |
| `rusqlite` | SQLite database (bundled) |
| `serde` / `serde_json` | Serialization (config, recording headers, session index) |
| `chrono` | Wall-clock timestamps for recording sessions |
| `flate2` | Gzip compression for recorded data |
| `uuid` | Session ID generation (v4) |
| `async-trait` | Async trait support |
| `tracing` | Structured logging |
| `rand` | Mock data simulation |
| `anyhow` / `thiserror` | Error handling |
