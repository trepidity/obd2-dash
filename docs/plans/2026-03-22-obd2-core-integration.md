# obd2-core Integration Plan

> Historical note (2026-04-09): this document reflects the original migration from the inline core implementation to the external `obd2-core` crate. The current active follow-on work is tracked in [`2026-04-09-obd2-core-api-alignment.md`](/Users/jared/Projects/HaulLogic/obd2-dash/docs/plans/2026-04-09-obd2-core-api-alignment.md) and its execution board.

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the inline `crates/obd2-core` with the standalone `~/Projects/obd2-core` library, making obd2-dash a pure UI shell.

**Architecture:** The inline `crates/obd2-core` (3400+ lines) contains both OBD2 protocol logic AND dash-specific analysis (fuel economy, driving behavior, AI). We split these: protocol logic comes from the external obd2-core library via path dependency; dash-specific modules move into the `obd2-dash` crate. The polling loop migrates from raw `Obd2Connection::query_pid()` to `Session::read_pid()`.

**Tech Stack:** Rust, tokio, ratatui, obd2-core (external), obd2-db (retained for now)

---

## Type Migration Reference

Every task below depends on this mapping. The OLD types are the inline `crates/obd2-core`. The NEW types are from `~/Projects/obd2-core`.

| Old (inline crate) | New (external obd2-core) | Notes |
|---|---|---|
| `Pid` (enum: `Pid::EngineRpm`) | `Pid` (newtype: `Pid::ENGINE_RPM`, `Pid(0x0C)`) | Match arms → if/else or lookup table |
| `PidReading { value: f64, raw_bytes }` | `Reading { value: Value, unit, timestamp, raw_bytes, source }` | Use `.value.as_f64()?` to extract |
| `Obd2Connection` trait | `Adapter` trait (+ `Session<A>` wrapper) | Session is the primary API |
| `Elm327` struct | `Elm327Adapter` struct | `Elm327Adapter::new(Box::new(transport))` |
| `MockObd2` | `MockAdapter` | `MockAdapter::new()` or `::with_vin()` |
| `Obd2Error` (in `obd2::types`) | `Obd2Error` (in `error`) | Similar variants, slightly different |
| `AdapterInfo` (in `obd2::types`) | `AdapterInfo` (in `adapter`) | Now has `Capabilities` struct |
| `Dtc { code, description, category }` | `Dtc { code, category, status, description, severity, ... }` | Richer type |
| `Transport` trait (`send`/`read_line`) | `Transport` trait (`write`/`read`/`reset`/`name`) | Different method signatures |
| `SerialTransport` | `SerialTransport` | `::new(port, baud)` or `::with_defaults(port)` |
| `BleTransport` | `BleTransport` (feature `ble`) | Similar API |
| `VehicleData` (struct with named fields) | No equivalent — dash must aggregate | `Reading` values stored in a HashMap |
| `Chipset` enum | `Chipset` enum | Same variants |
| `scanner::spawn_scan()` | No scanner in new core | Keep scanning logic in dash |
| `vin::decode_year/manufacturer` | Via `Session::identify_vehicle()` | Returns `VehicleProfile` |
| `dtc_description(code)` | `Dtc.description` field | Already populated |
| `DomainMessage`, `DomainState`, `ConnectionState` | No equivalent — dash-specific | Keep in dash |

### Import path changes

```rust
// OLD
use obd2_core::{Pid, PidReading, Obd2Connection, Obd2Error, AdapterInfo, Dtc, MockObd2, VehicleData};
use obd2_core::obd2::elm327::Elm327;
use obd2_core::obd2::transport::Transport;
use obd2_core::obd2::serial_transport::SerialTransport;

// NEW
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::enhanced::{Reading, Value};
use obd2_core::protocol::dtc::Dtc;
use obd2_core::adapter::{Adapter, AdapterInfo};
use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::adapter::mock::MockAdapter;
use obd2_core::transport::Transport;
use obd2_core::transport::serial::SerialTransport;
use obd2_core::session::Session;
use obd2_core::error::Obd2Error;
```

---

## Phase 1: Prepare — Move Dash-Specific Modules (Tasks 1–4)

These modules have zero OBD2 protocol logic and belong in the dash crate.

### Task 1: Move `fuel_economy.rs` into dash crate

**Files:**
- Copy: `crates/obd2-core/src/fuel_economy.rs` → `crates/obd2-dash/src/analysis/fuel_economy.rs`
- Create: `crates/obd2-dash/src/analysis/mod.rs`
- Modify: `crates/obd2-dash/src/main.rs` (update imports)
- Modify: `crates/obd2-dash/src/app.rs` (update imports if used)

**Step 1: Create the analysis module directory**

```bash
mkdir -p crates/obd2-dash/src/analysis
```

**Step 2: Copy fuel_economy.rs**

Copy `crates/obd2-core/src/fuel_economy.rs` to `crates/obd2-dash/src/analysis/fuel_economy.rs`.

The file currently imports `crate::obd2::types::VehicleData`. Since `VehicleData` is a simple struct with named float fields, create a local type alias or adapt the import. The new obd2-core doesn't have `VehicleData` — it uses `Reading` per-PID. So `fuel_economy.rs` already takes `SensorSnapshot` as input (not `VehicleData` directly). Check if `SensorSnapshot` is self-contained. If it only uses primitive fields (f64, Option<f64>), no adaptation needed.

**Step 3: Create analysis/mod.rs**

```rust
pub mod fuel_economy;
```

**Step 4: Add `mod analysis;` to main.rs or lib equivalent**

In `crates/obd2-dash/src/main.rs`, add:
```rust
mod analysis;
```

Update any imports from `obd2_core::FuelEconomyState` to `crate::analysis::fuel_economy::FuelEconomyState`.

**Step 5: Verify it compiles**

```bash
cargo check -p obd2-dash 2>&1 | head -30
```

**Step 6: Commit**

```
feat: move fuel_economy module from obd2-core to obd2-dash
```

---

### Task 2: Move `driving.rs` into dash crate

**Files:**
- Copy: `crates/obd2-core/src/driving.rs` → `crates/obd2-dash/src/analysis/driving.rs`
- Modify: `crates/obd2-dash/src/analysis/mod.rs`

**Step 1: Copy driving.rs**

This file has NO imports from obd2-core (only std). Direct copy.

**Step 2: Add to analysis/mod.rs**

```rust
pub mod fuel_economy;
pub mod driving;
```

**Step 3: Update imports throughout dash crate**

Replace `obd2_core::DrivingBehavior` with `crate::analysis::driving::DrivingBehavior`.

**Step 4: Verify compilation**

```bash
cargo check -p obd2-dash 2>&1 | head -30
```

**Step 5: Commit**

```
feat: move driving behavior module from obd2-core to obd2-dash
```

---

### Task 3: Move `ai/` into dash crate

**Files:**
- Copy: `crates/obd2-core/src/ai/` → `crates/obd2-dash/src/ai/`
- Modify: `crates/obd2-dash/src/main.rs`
- Modify: `crates/obd2-dash/Cargo.toml` (add `reqwest` dependency for HTTP)

**Step 1: Copy the entire ai/ directory**

The AI module depends on `obd2_db::models` (not on obd2-core protocol types). It needs `reqwest` for HTTP calls to LLM APIs.

**Step 2: Add reqwest to obd2-dash Cargo.toml**

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

**Step 3: Update imports**

Replace `obd2_core::ai::*` with `crate::ai::*` throughout the dash crate.

**Step 4: Remove the `ai` feature flag usage**

The old `obd2-core` Cargo.toml had `features = ["ai"]`. After moving the AI module out, this feature flag is no longer needed on the core dependency.

**Step 5: Verify compilation**

```bash
cargo check -p obd2-dash 2>&1 | head -30
```

**Step 6: Commit**

```
feat: move AI analysis module from obd2-core to obd2-dash
```

---

### Task 4: Move `mock_profile.rs` into dash crate

**Files:**
- Copy: `crates/obd2-core/src/mock_profile.rs` → `crates/obd2-dash/src/mock_profile.rs`

**Step 1: Copy mock_profile.rs**

No external dependencies. Direct copy.

**Step 2: Update imports**

Replace `obd2_core::MockVehicleProfile` with `crate::mock_profile::MockVehicleProfile`.

**Step 3: Verify compilation, commit**

```
feat: move mock vehicle profiles from obd2-core to obd2-dash
```

---

## Phase 2: Swap the Core Dependency (Tasks 5–7)

### Task 5: Add external obd2-core dependency, remove inline crate

**Files:**
- Modify: `Cargo.toml` (workspace root) — remove `crates/obd2-core` from members
- Modify: `crates/obd2-dash/Cargo.toml` — change obd2-core path to external

**Step 1: Update workspace Cargo.toml**

Remove `"crates/obd2-core"` from workspace members:

```toml
[workspace]
resolver = "2"
members = [
    "crates/obd2-db",
    "crates/obd2-dash",
]
```

**Step 2: Update obd2-dash Cargo.toml**

Change the obd2-core dependency from the inline path to the external library:

```toml
# OLD
# obd2-core = { path = "../obd2-core", features = ["ai"] }

# NEW
obd2-core = { path = "/Users/jared/Projects/obd2-core/crates/obd2-core", features = ["serial", "embedded-specs"] }
```

Note: Add `"ble"` feature when BLE support is needed:
```toml
obd2-core = { path = "/Users/jared/Projects/obd2-core/crates/obd2-core", features = ["serial", "ble", "embedded-specs"] }
```

**Step 3: Do NOT delete `crates/obd2-core/` yet**

We'll reference it during migration. Delete after all tasks are complete.

**Step 4: Attempt cargo check — expect errors**

```bash
cargo check -p obd2-dash 2>&1 | head -50
```

This will fail with many import errors. That's expected — the remaining tasks fix them.

**Step 5: Commit**

```
refactor: swap inline obd2-core for external library dependency
```

---

### Task 6: Create a compatibility layer for VehicleData

The old inline core had `VehicleData` — a flat struct with named fields (`rpm: f64`, `speed: f64`, `coolant_temp: f64`, etc.). The new core doesn't have this. The dash's `DomainState`, UI renderers, fuel economy, and recording all reference `VehicleData`.

**Files:**
- Create: `crates/obd2-dash/src/vehicle_data.rs`
- Modify: `crates/obd2-dash/src/main.rs`

**Step 1: Create vehicle_data.rs with the VehicleData struct**

Copy the `VehicleData` struct definition from the OLD `crates/obd2-core/src/obd2/types.rs`. This is a simple data holder — no protocol logic. It just stores the latest value for each PID.

```rust
use std::collections::HashMap;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::enhanced::Reading;

/// Aggregated vehicle sensor readings for UI display.
/// Updated from Session::read_pid() results.
#[derive(Debug, Default)]
pub struct VehicleData {
    pub rpm: f64,
    pub speed: f64,
    pub coolant_temp: f64,
    pub intake_air_temp: f64,
    pub engine_load: f64,
    pub throttle_position: f64,
    pub maf: f64,
    pub intake_map: f64,
    pub timing_advance: f64,
    pub fuel_pressure: f64,
    pub short_fuel_trim_b1: f64,
    pub long_fuel_trim_b1: f64,
    pub short_fuel_trim_b2: f64,
    pub long_fuel_trim_b2: f64,
    pub ambient_air_temp: f64,
    pub barometric_pressure: f64,
    pub catalyst_temp_b1s1: f64,
    pub catalyst_temp_b2s1: f64,
    pub catalyst_temp_b1s2: f64,
    pub catalyst_temp_b2s2: f64,
    pub control_module_voltage: f64,
    pub engine_oil_temp: f64,
    pub engine_fuel_rate: f64,
    pub fuel_rail_gauge_pressure: f64,
    pub fuel_rail_abs_pressure: f64,
    pub absolute_load: f64,
    pub commanded_equiv_ratio: f64,
    pub commanded_egr: f64,
    pub commanded_evap_purge: f64,
    pub relative_throttle_pos: f64,
    pub abs_throttle_pos_b: f64,
    pub accel_pedal_pos_d: f64,
    pub accel_pedal_pos_e: f64,
    pub demanded_torque: f64,
    pub actual_torque: f64,
    pub reference_torque: f64,
    pub run_time: f64,
    pub distance_with_mil: f64,
    pub distance_since_dtc_clear: f64,
    // Custom PIDs (from old inline core)
    pub oil_pressure: f64,
    pub transmission_temp: f64,
}

impl VehicleData {
    /// Update a field from a new PID reading.
    pub fn apply_reading(&mut self, pid: Pid, reading: &Reading) {
        let val = match reading.value.as_f64() {
            Ok(v) => v,
            Err(_) => return, // skip non-scalar values
        };
        // Map Pid constants to struct fields
        if pid == Pid::ENGINE_RPM { self.rpm = val; }
        else if pid == Pid::VEHICLE_SPEED { self.speed = val; }
        else if pid == Pid::COOLANT_TEMP { self.coolant_temp = val; }
        else if pid == Pid::INTAKE_AIR_TEMP { self.intake_air_temp = val; }
        else if pid == Pid::ENGINE_LOAD { self.engine_load = val; }
        else if pid == Pid::THROTTLE_POSITION { self.throttle_position = val; }
        else if pid == Pid::MAF { self.maf = val; }
        else if pid == Pid::INTAKE_MAP { self.intake_map = val; }
        else if pid == Pid::TIMING_ADVANCE { self.timing_advance = val; }
        else if pid == Pid::FUEL_PRESSURE { self.fuel_pressure = val; }
        else if pid == Pid::SHORT_FUEL_TRIM_B1 { self.short_fuel_trim_b1 = val; }
        else if pid == Pid::LONG_FUEL_TRIM_B1 { self.long_fuel_trim_b1 = val; }
        else if pid == Pid::SHORT_FUEL_TRIM_B2 { self.short_fuel_trim_b2 = val; }
        else if pid == Pid::LONG_FUEL_TRIM_B2 { self.long_fuel_trim_b2 = val; }
        else if pid == Pid::AMBIENT_AIR_TEMP { self.ambient_air_temp = val; }
        else if pid == Pid::BAROMETRIC_PRESSURE { self.barometric_pressure = val; }
        else if pid == Pid::CATALYST_TEMP_B1S1 { self.catalyst_temp_b1s1 = val; }
        else if pid == Pid::CATALYST_TEMP_B2S1 { self.catalyst_temp_b2s1 = val; }
        else if pid == Pid::CATALYST_TEMP_B1S2 { self.catalyst_temp_b1s2 = val; }
        else if pid == Pid::CATALYST_TEMP_B2S2 { self.catalyst_temp_b2s2 = val; }
        else if pid == Pid::CONTROL_MODULE_VOLTAGE { self.control_module_voltage = val; }
        else if pid == Pid::ENGINE_OIL_TEMP { self.engine_oil_temp = val; }
        else if pid == Pid::ENGINE_FUEL_RATE { self.engine_fuel_rate = val; }
        else if pid == Pid::FUEL_RAIL_GAUGE_PRESSURE { self.fuel_rail_gauge_pressure = val; }
        else if pid == Pid::FUEL_RAIL_ABS_PRESSURE { self.fuel_rail_abs_pressure = val; }
        else if pid == Pid::ABSOLUTE_LOAD { self.absolute_load = val; }
        else if pid == Pid::COMMANDED_EQUIV_RATIO { self.commanded_equiv_ratio = val; }
        else if pid == Pid::COMMANDED_EGR { self.commanded_egr = val; }
        else if pid == Pid::COMMANDED_EVAP_PURGE { self.commanded_evap_purge = val; }
        else if pid == Pid::RELATIVE_THROTTLE_POS { self.relative_throttle_pos = val; }
        else if pid == Pid::ABS_THROTTLE_POS_B { self.abs_throttle_pos_b = val; }
        else if pid == Pid::ACCEL_PEDAL_POS_D { self.accel_pedal_pos_d = val; }
        else if pid == Pid::ACCEL_PEDAL_POS_E { self.accel_pedal_pos_e = val; }
        else if pid == Pid::DEMANDED_TORQUE { self.demanded_torque = val; }
        else if pid == Pid::ACTUAL_TORQUE { self.actual_torque = val; }
        else if pid == Pid::REFERENCE_TORQUE { self.reference_torque = val; }
        else if pid == Pid::RUN_TIME { self.run_time = val; }
        else if pid == Pid::DISTANCE_WITH_MIL { self.distance_with_mil = val; }
        else if pid == Pid::DISTANCE_SINCE_CLEAR { self.distance_since_dtc_clear = val; }
    }
}
```

**Step 2: Verify the Pid constants exist**

Cross-reference against the new obd2-core's `Pid` constants. Some old PIDs may have different constant names. Check `~/Projects/obd2-core/crates/obd2-core/src/protocol/pid.rs` for exact names. Adjust any that don't match.

**Step 3: Commit**

```
feat: add VehicleData compatibility layer for new obd2-core types
```

---

### Task 7: Rewrite DomainState for new types

**Files:**
- Create: `crates/obd2-dash/src/domain.rs` (replaces the old `obd2_core::state`)
- Modify: `crates/obd2-dash/src/app.rs`

**Step 1: Create domain.rs**

Move `DomainState`, `DomainMessage`, `ConnectionState`, `TemperatureUnit`, `SpeedUnit` from the old `crates/obd2-core/src/state.rs` into `crates/obd2-dash/src/domain.rs`. These are dash-specific state management types.

Key changes:
- Replace `use crate::obd2::{...}` with `use obd2_core::protocol::pid::Pid;` etc.
- Replace `PidReading` with `Reading` from `obd2_core::protocol::enhanced`
- Replace `VehicleData` with `crate::vehicle_data::VehicleData`
- Replace `AdapterInfo` with `obd2_core::adapter::AdapterInfo`
- Replace `Dtc` with `obd2_core::protocol::dtc::Dtc`

The `DomainMessage` enum becomes:

```rust
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::enhanced::Reading;
use obd2_core::protocol::dtc::Dtc;
use obd2_core::adapter::AdapterInfo;

#[derive(Debug)]
pub enum DomainMessage {
    PidUpdate(Pid, Reading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    ConnectionStatus(ConnectionState),
    AdapterDetected(AdapterInfo),
    Error(String),
}
```

The `DomainState::update()` method needs to call `self.vehicle.apply_reading(pid, &reading)` instead of the old match-based field assignment.

**Step 2: Update app.rs imports**

Replace `use obd2_core::{ConnectionState, DomainMessage, DomainState, ...}` with `use crate::domain::*`.

**Step 3: Verify compilation**

```bash
cargo check -p obd2-dash 2>&1 | head -50
```

**Step 4: Commit**

```
refactor: move DomainState to dash crate, adapt to new obd2-core types
```

---

## Phase 3: Rewrite Connection & Polling (Tasks 8–10)

### Task 8: Rewrite device scanning (keep in dash)

The new obd2-core does NOT have a scanner module. The old scanner combined serial port enumeration (`serialport::available_ports()`) with BLE scanning (`btleplug`). Move scanning into the dash crate.

**Files:**
- Create: `crates/obd2-dash/src/scanner.rs`

**Step 1: Copy scanner logic from old crate**

Copy `crates/obd2-core/src/obd2/scanner.rs` to `crates/obd2-dash/src/scanner.rs`.

Adapt imports:
- Remove any `crate::obd2::` prefixed imports
- The scanner uses `serialport` (for serial enumeration) and `btleplug` (for BLE) — both are already in dash's Cargo.toml or need to be added.

Keep the existing types: `DeviceKind`, `DiscoveredDevice`, `ScanEvent`, `spawn_scan()`.

**Step 2: Add missing deps to obd2-dash Cargo.toml if needed**

```toml
serialport = "4.7"  # already present
# btleplug = { version = "0.11", optional = true }  # add if BLE scanning needed
```

**Step 3: Update imports throughout dash crate**

Replace `obd2_core::{DeviceKind, DiscoveredDevice, ScanEvent}` with `crate::scanner::*`.

**Step 4: Commit**

```
feat: move device scanner from obd2-core to obd2-dash
```

---

### Task 9: Rewrite the OBD2 polling task

This is the core integration point. The old `main.rs` creates an `Elm327` or `MockObd2`, calls `conn.initialize()`, then loops calling `conn.query_pid()`. The new pattern uses `Session<A>`.

**Files:**
- Modify: `crates/obd2-dash/src/main.rs`

**Step 1: Rewrite the connection creation**

OLD pattern:
```rust
let mut conn = Elm327::new(SerialTransport::new(&port, baud).await?);
conn.initialize().await?;
```

NEW pattern:
```rust
use obd2_core::transport::serial::SerialTransport;
use obd2_core::adapter::elm327::Elm327Adapter;
use obd2_core::session::Session;

let transport = SerialTransport::new(&port, baud)?;
let adapter = Elm327Adapter::new(Box::new(transport));
let mut session = Session::new(adapter);
// Session handles initialization internally on first use
```

For mock mode:
```rust
use obd2_core::adapter::mock::MockAdapter;

let adapter = MockAdapter::new();
let mut session = Session::new(adapter);
```

**Step 2: Rewrite the polling loop**

OLD:
```rust
for pid in supported_pids.iter() {
    match conn.query_pid(*pid).await {
        Ok(reading) => tx.send(Message::PidUpdate(*pid, reading)),
        Err(e) => { /* handle */ }
    }
}
```

NEW:
```rust
// Get supported PIDs once
let supported = session.supported_pids().await?;

// Polling loop
for &pid in &supported_list {
    match session.read_pid(pid).await {
        Ok(reading) => {
            let _ = tx.send(Message::PidUpdate(pid, reading)).await;
        }
        Err(obd2_core::error::Obd2Error::NoData) => continue,
        Err(e) => {
            let _ = tx.send(Message::Error(e.to_string())).await;
        }
    }
}
```

**Step 3: Rewrite VIN reading**

OLD: `conn.read_vin()`
NEW: `session.read_vin()` or `session.identify_vehicle()`

**Step 4: Rewrite DTC reading**

OLD: `conn.read_dtcs()`
NEW: `session.read_dtcs()`

**Step 5: Rewrite voltage reading**

OLD: `conn.read_voltage()`
NEW: `session.battery_voltage()`

**Step 6: Handle the Session ownership**

`Session` takes `&mut self` for all reads, so it must be owned by the polling task. The polling task runs in a separate tokio::spawn, communicating via mpsc channel to the TUI thread. This pattern stays the same — only the internal API calls change.

**Step 7: Verify the polling compiles and types align**

```bash
cargo check -p obd2-dash 2>&1 | head -50
```

**Step 8: Commit**

```
refactor: rewrite OBD2 polling to use Session API from external obd2-core
```

---

### Task 10: Adapt connection preferences

**Files:**
- Copy: `crates/obd2-core/src/obd2/connection_prefs.rs` → `crates/obd2-dash/src/connection_prefs.rs`

This is a small file (~49 lines) that persists the last-used device. It has no protocol dependencies. Direct copy and import update.

**Step 1: Copy, update imports, commit**

```
feat: move connection preferences to obd2-dash
```

---

## Phase 4: Adapt Recording System (Tasks 11–12)

### Task 11: Adapt recording to new types

The recording system captures raw PID data to binary `.obd2rec` files. It currently uses the old `Pid` enum's `.code()` method and `PidReading.value` (f64). The new `Pid` is `Pid(pub u8)` — access the code via `pid.0`. The new `Reading.value` is `Value` — extract via `.as_f64()`.

**Files:**
- Move: `crates/obd2-core/src/recording/` → `crates/obd2-dash/src/recording/`

**Step 1: Copy the entire recording/ directory into dash**

**Step 2: Update type imports throughout recording/**

- `Pid::code()` → `pid.0` (the newtype's inner u8)
- `PidReading { value, raw_bytes }` → `Reading { value: Value, raw_bytes, ... }`
- For `writer.write_pid()`: extract `reading.value.as_f64().unwrap_or(0.0)` and `&reading.raw_bytes`
- For `reader`: construct values back as `f64` (recording format stores raw floats)

**Step 3: Adapt replay**

The replay controller produces `(Pid, PidReading)` pairs. Change to produce `(Pid, Reading)` pairs. Construct `Reading` from recorded data:

```rust
use obd2_core::protocol::enhanced::{Reading, Value, ReadingSource};
use std::time::Instant;

Reading {
    value: Value::Scalar(recorded_value),
    unit: "", // or look up from Pid
    timestamp: Instant::now(),
    raw_bytes: recorded_raw.to_vec(),
    source: ReadingSource::Replay,
}
```

**Step 4: Verify compilation, commit**

```
refactor: adapt recording system to new obd2-core types
```

---

### Task 12: Adapt recording state in DomainState

The `DomainState::update()` method intercepts `PidUpdate` messages and writes to the recording. Update the interception to work with `Reading` instead of `PidReading`.

**Files:**
- Modify: `crates/obd2-dash/src/domain.rs`

**Step 1: Update the recording interception**

```rust
DomainMessage::PidUpdate(pid, ref reading) => {
    // Recording interception
    if let RecordingState::Recording { ref mut writer, ref start_instant, .. } = self.recording {
        let offset_ms = start_instant.elapsed().as_millis() as u32;
        let val = reading.value.as_f64().unwrap_or(0.0);
        let _ = writer.write_pid(offset_ms, pid.0, val, &reading.raw_bytes);
    }
    // Update aggregated vehicle data
    self.vehicle.apply_reading(pid, reading);
}
```

**Step 2: Commit**

```
fix: adapt recording interception for new Reading type
```

---

## Phase 5: Adapt UI Layer (Tasks 13–16)

### Task 13: Adapt Message enum in app.rs

**Files:**
- Modify: `crates/obd2-dash/src/app.rs`

**Step 1: Update the Message enum**

Replace `PidUpdate(Pid, PidReading)` with `PidUpdate(Pid, Reading)`:

```rust
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::enhanced::Reading;
use obd2_core::protocol::dtc::Dtc;
use obd2_core::adapter::AdapterInfo;

pub enum Message {
    PidUpdate(Pid, Reading),
    VoltageUpdate(f64),
    DtcUpdate(Vec<Dtc>),
    // ... rest stays the same
}
```

**Step 2: NHTSA integration**

The old `NhtsaVehicle` type lived in the inline obd2-core. If the new obd2-core has NHTSA support (behind `nhtsa` feature), use `session.identify_vehicle()` which does NHTSA internally. If not, move the NHTSA client code into the dash crate (small HTTP-based VIN decoder).

Check: does the external obd2-core Cargo.toml have a `nhtsa` feature? If yes, `Session::identify_vehicle()` handles it. Remove the manual NHTSA lookup from `main.rs`.

**Step 3: Commit**

```
refactor: adapt app Message enum for new obd2-core types
```

---

### Task 14: Adapt widget renderers for Pid newtype

The widget renderers in `crates/obd2-dash/src/widget/renderers.rs` match on `Pid` enum variants to decide what to display. With the new `Pid` newtype, these become equality comparisons.

**Files:**
- Modify: `crates/obd2-dash/src/widget/renderers.rs`

**Step 1: Replace match arms**

OLD:
```rust
match pid {
    Pid::EngineRpm => format!("{:.0} RPM", value),
    Pid::CoolantTemp => format!("{:.1}°C", value),
    // ...
}
```

NEW:
```rust
if pid == Pid::ENGINE_RPM { format!("{:.0} RPM", value) }
else if pid == Pid::COOLANT_TEMP { format!("{:.1}°C", value) }
// ...
else { format!("{:.1} {}", value, pid.unit()) }
```

Or use `pid.name()` and `pid.unit()` from the new core for generic display, which may simplify the renderer significantly.

**Step 2: Update VehicleData field access**

If renderers read `state.domain.vehicle.rpm` directly, those field names haven't changed (we preserved them in the compatibility VehicleData struct).

**Step 3: Verify compilation, commit**

```
refactor: adapt widget renderers for Pid newtype constants
```

---

### Task 15: Adapt TUI panel rendering

**Files:**
- Modify: `crates/obd2-dash/src/tui/ui.rs`
- Modify: `crates/obd2-dash/src/tui/panel.rs`

**Step 1: Update any Pid references**

Search for `Pid::` throughout the TUI files. Replace enum variant references with constant comparisons.

**Step 2: Update Dtc rendering**

The new `Dtc` has `description: Option<String>` instead of `description: &'static str`. Update display logic:

```rust
// OLD
dtc.description
// NEW
dtc.description.as_deref().unwrap_or("Unknown")
```

**Step 3: Update AdapterInfo rendering**

The new `AdapterInfo` has `capabilities: Capabilities` struct instead of individual bool fields. Access: `info.capabilities.can_clear_dtcs`, `info.capabilities.battery_voltage`, etc.

**Step 4: Commit**

```
refactor: adapt TUI rendering for new obd2-core types
```

---

### Task 16: Adapt key event handling

**Files:**
- Modify: `crates/obd2-dash/src/tui/event.rs`

**Step 1: Check for any Pid references in event handling**

The event handler maps keyboard inputs to `Message` variants. It likely doesn't reference `Pid` directly, but verify.

**Step 2: Update any DTC-related key handlers**

If there are handlers for clearing DTCs, the new API uses `session.clear_dtcs()` (called via a message to the polling task).

**Step 3: Commit**

```
refactor: adapt event handling for new obd2-core types
```

---

## Phase 6: Adapt Remaining Modules (Tasks 17–19)

### Task 17: Adapt fuel economy for new VehicleData

**Files:**
- Modify: `crates/obd2-dash/src/analysis/fuel_economy.rs`

**Step 1: Check SensorSnapshot construction**

`FuelEconomyState::recalculate()` takes a `SensorSnapshot`. Verify how `SensorSnapshot` is populated from `VehicleData` fields. Since we preserved the same field names in our compatibility `VehicleData`, this should work without changes.

**Step 2: Verify, commit**

```
fix: verify fuel economy works with new VehicleData compatibility layer
```

---

### Task 18: Adapt diagnostics module

**Files:**
- Copy: `crates/obd2-core/src/diagnostics/` → `crates/obd2-dash/src/diagnostics/`

The diagnostics module depends on `Pid`, `Dtc`, and `VehicleData`. It provides DTC-to-PID correlation tables.

**Step 1: Copy diagnostics/ directory**

**Step 2: Update Pid references**

Replace `Pid::EngineRpm` style references with `Pid::ENGINE_RPM` throughout the correlation tables.

**Step 3: Update Dtc references**

Adapt for the new `Dtc` struct fields.

**Step 4: Commit**

```
refactor: move diagnostics module to dash, adapt for new types
```

---

### Task 19: Evaluate and adapt obd2-db integration

**Files:**
- Review: `crates/obd2-db/src/lib.rs`, `crates/obd2-db/src/models.rs`

The new obd2-core has its own `VehicleSpec` + `ThresholdSet` via the `SpecRegistry`. Evaluate whether obd2-db's threshold resolution can be replaced by the core's spec system.

**Step 1: Compare threshold systems**

Old obd2-db: `ResolvedThreshold` with `min/max/warning_high/warning_low/critical_high/critical_low`
New obd2-core: `Threshold` with same fields, evaluated via `Session::evaluate_threshold()`

If they're compatible, remove the threshold resolution from obd2-db and use the Session API.

**Step 2: Keep obd2-db for vehicle info storage**

obd2-db stores VehicleInfo (VIN history, engine families). Keep this functionality for now. The new core's `VehicleStore` trait could replace it eventually, but that's a separate effort.

**Step 3: Update DomainState to use Session threshold evaluation**

```rust
// Instead of looking up thresholds from obd2-db:
if let Some(result) = session.evaluate_threshold(pid, value) {
    match result.level {
        AlertLevel::Warning => { /* ... */ }
        AlertLevel::Critical => { /* ... */ }
        _ => {}
    }
}
```

**Step 4: Commit**

```
refactor: migrate threshold evaluation to obd2-core SpecRegistry
```

---

## Phase 7: Cleanup & Verification (Tasks 20–23)

### Task 20: Remove inline crates/obd2-core directory

**Files:**
- Delete: `crates/obd2-core/` (entire directory)

**Step 1: Verify the project compiles without it**

```bash
cargo check -p obd2-dash 2>&1 | head -30
```

**Step 2: Delete**

```bash
rm -rf crates/obd2-core
```

**Step 3: Verify clean build**

```bash
cargo build -p obd2-dash 2>&1 | tail -5
```

**Step 4: Commit**

```
chore: remove inline obd2-core crate (replaced by external library)
```

---

### Task 21: Run tests

**Files:**
- Any test files in `crates/obd2-dash/`

**Step 1: Run all tests**

```bash
cargo test -p obd2-dash 2>&1
```

**Step 2: Fix any test failures**

Tests may reference old type names. Apply the same migrations (Pid enum → Pid constants, PidReading → Reading, etc.).

**Step 3: Also run obd2-db tests**

```bash
cargo test -p obd2-db 2>&1
```

**Step 4: Commit fixes**

```
fix: update tests for new obd2-core type system
```

---

### Task 22: Verify with mock mode

**Step 1: Run the app in mock mode**

```bash
cargo run -p obd2-dash -- --mock --mock-vehicle chevy
```

**Step 2: Verify**

- Dashboard renders without panics
- PID values update on screen
- DTCs display correctly
- Voltage displays
- Fuel economy calculations work
- Recording start/stop works

**Step 3: Fix any runtime issues**

---

### Task 23: Clean up unused dependencies

**Files:**
- Modify: `crates/obd2-dash/Cargo.toml`

**Step 1: Remove dependencies that were only needed for the inline core**

Check if `tokio-serial` and `serialport` are still needed directly in obd2-dash. After migration, serial transport lives in the external obd2-core — the dash shouldn't need these directly (except for the scanner, which uses `serialport` for port enumeration).

```bash
cargo check -p obd2-dash 2>&1 | head -30
```

**Step 2: Commit**

```
chore: remove unused dependencies after obd2-core migration
```

---

## Dependency Graph (After Migration)

```
obd2-dash (TUI app)
├── obd2-core (external, ~/Projects/obd2-core)
│   ├── protocol/ (Pid, Dtc, Reading, Value)
│   ├── adapter/ (Elm327Adapter, MockAdapter)
│   ├── transport/ (SerialTransport, BleTransport)
│   ├── session/ (Session<A> — primary API)
│   └── vehicle/ (SpecRegistry, VehicleSpec, VehicleProfile)
├── obd2-db (vehicle info storage, engine families)
├── analysis/ (fuel_economy, driving — dash-specific)
├── ai/ (LLM-based diagnostics — dash-specific)
├── recording/ (data recording/replay — dash-specific binary format)
├── diagnostics/ (DTC-PID correlation — dash-specific)
├── scanner.rs (device discovery — dash-specific)
├── vehicle_data.rs (VehicleData compatibility struct)
├── domain.rs (DomainState, DomainMessage — dash-specific)
└── widget/ + tui/ (UI rendering — unchanged)
```
