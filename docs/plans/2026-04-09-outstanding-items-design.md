# Outstanding Items Resolution — Design

**Date:** 2026-04-09

**Scope:** Resolve all items from `docs/OUTSTANDING.md` in a single plan: diagnostics expansion, raw capture metadata fix, and test coverage gaps.

---

## Background

The API alignment work (T1–T9) brought `obd2-dash` onto the `obd2-core` session-first API. Several features were identified as follow-up work. All required APIs already exist in `obd2-core` — the work is wiring, UI, and testing.

### obd2-core API surface available

**DTC operations (all on `Session<A>`):**
- `read_dtcs()` → `Result<Vec<Dtc>>` (Mode 03, stored)
- `read_all_dtcs()` → `Result<Vec<Dtc>>` (aggregates stored + pending + permanent)
- `read_pending_dtcs()` → `Result<Vec<Dtc>>` (Mode 07)
- `read_permanent_dtcs()` → `Result<Vec<Dtc>>` (Mode 0A)
- `clear_dtcs()` → `Result<()>` (broadcast Mode 04, resets readiness monitors)
- `clear_dtcs_on_module(ModuleId)` → `Result<()>` (targeted Mode 04)

**Readiness monitors:**
- `read_readiness()` → `Result<ReadinessStatus>`
- `ReadinessStatus { mil_on: bool, dtc_count: u8, compression_ignition: bool, monitors: Vec<MonitorStatus> }`
- `MonitorStatus { name: String, supported: bool, complete: bool }`

**Freeze-frame:**
- `read_freeze_frame(Pid, frame: u8)` → `Result<Reading>`
- `Reading.source` is `ReadingSource::FreezeFrame`

**Raw capture metadata:**
- `CaptureMetadata { transport_type: String, port_or_device: String, baud_rate: Option<u32> }`
- `baud_rate` field already exists — just not populated from serial setup

---

## Three Phases

### Phase 1: Quick Wins

**1a. Baud rate passthrough**

In `main.rs`, where the serial transport is constructed and `CaptureMetadata` is built, pass the CLI `--baud` value (or detected baud rate) into `CaptureMetadata.baud_rate` instead of `None`.

Files: `crates/obd2-dash/src/main.rs`

**1b. Filesystem test coverage**

Add `tempfile` as a dev-dependency. Write tests for:

- **StorageManager**: register session, compress session, run maintenance (compression threshold, FIFO trimming), delete session, storage stats, raw capture bytes calculation, reload index.
- **SessionIndex**: load/save roundtrip, add/remove sessions, mark compressed, total size calculation, missing-file graceful default, sessions sorted order.
- **ConnectionPrefs**: load/save roundtrip, missing file returns default, invalid JSON returns default.

All tests use `tempfile::TempDir` for isolated temp directories with automatic cleanup.

Files: `crates/obd2-dash/Cargo.toml` (dev-dep), `crates/obd2-dash/src/recording/storage.rs`, `crates/obd2-dash/src/recording/index.rs`, `crates/obd2-dash/src/connection_prefs.rs`

### Phase 2: Diagnostics Expansion

**2a. Clear DTCs**

New message flow:
- `Message::ClearDtcsRequested` (broadcast) and `Message::ClearDtcsOnModuleRequested(ModuleId)` (targeted)
- Session runner calls `session.clear_dtcs()` or `session.clear_dtcs_on_module(module_id)`
- `Message::ClearDtcsComplete` or `Message::ClearDtcsError(String)` sent back
- Domain state clears `stored_dtcs` on success

UI confirmation:
- **Broadcast clear**: User presses `C` (capital) in DTC panel → popup: "Clear all DTCs? This resets readiness monitors. Enter to confirm, Esc to cancel." On Enter, sends `ClearDtcsRequested`.
- **Module-targeted clear**: User selects a specific DTC, presses `C` → first `C` shows footer flash "Press C again to clear DTCs on [module]", second `C` within 2 seconds sends `ClearDtcsOnModuleRequested(module_id)`.

Files: `app.rs`, `domain.rs`, `session_runner.rs`, `main.rs`, `tui/ui.rs`, `tui/panel.rs`

**2b. Readiness monitors**

New state:
- `DomainState.readiness: Option<ReadinessStatus>` (using the obd2-core type directly or a local snapshot)
- `DomainMessage::ReadinessUpdate(ReadinessStatus)`
- `Message::ReadinessUpdate(ReadinessStatus)`

Polling: Every 20th cycle (5s at default 250ms poll), alongside O2 monitoring. New `poll_readiness()` function in `session_runner.rs`.

UI: New `ReadinessPanel` widget type in `widget/mod.rs` registry.
- Shows: MIL status (on/off with color), DTC count, ignition type
- Per-monitor rows: name, supported (yes/no), complete (checkmark/dash)
- Color: green if all supported monitors complete, yellow if incomplete, red if MIL on

Files: `app.rs`, `domain.rs`, `session_runner.rs`, `widget/mod.rs`, `widget/renderers.rs`

**2c. Freeze-frame in DTC popup**

On-demand fetch: When the user opens a DTC detail popup (Enter on a selected DTC), if the DTC has `status == Stored`, send a request to the session runner to fetch freeze-frame data for a set of correlated PIDs.

New messages:
- `Message::FreezeFrameRequest { dtc_code: String, pids: Vec<Pid> }`
- `Message::FreezeFrameResult { dtc_code: String, readings: Vec<(Pid, Reading)> }` or `Message::FreezeFrameError(String)`

The session runner receives the request via a channel (similar to `CaptureCommand`), calls `session.read_freeze_frame(pid, 0)` for each requested PID, and sends the result back.

UI: The DTC detail popup gains a new section "Freeze-Frame Snapshot" below "Related Sensors", showing the frozen sensor values at the time the DTC was set. If no freeze-frame data is available (vehicle doesn't support it or frame is empty), the section is omitted.

Files: `app.rs`, `session_runner.rs`, `main.rs`, `tui/ui.rs`, `tui/panel.rs`

### Phase 3: Tests and Documentation

- Tests for all new session runner operations: `poll_readiness`, clear DTC command handling, freeze-frame request/response
- Tests for new domain state transitions: readiness update, clear DTCs success/error, freeze-frame popup state
- Update `README.md`: add readiness panel to widget list, `C` keybinding, freeze-frame mention in DTC section
- Update `MANUAL.md`: new readiness section, updated DTC section with clear and freeze-frame, updated keyboard reference
- Update `docs/OUTSTANDING.md`: mark completed items, remove resolved entries

---

## Polling Cadence (after all phases)

| Cycle modulus | Operation | Interval at 250ms |
|---------------|-----------|-------------------|
| Every cycle | Standard PIDs + voltage | 250ms |
| Every 5th | Enhanced PIDs | 1.25s |
| Every 10th | DTCs (`read_all_dtcs`) | 2.5s |
| Every 20th | O2 monitoring + readiness | 5s |
| On-demand | Freeze-frame, clear DTCs | User-triggered |

## New Message Types Summary

```
Message::ClearDtcsRequested
Message::ClearDtcsOnModuleRequested(ModuleId)
Message::ClearDtcsComplete
Message::ClearDtcsError(String)
Message::ReadinessUpdate(ReadinessStatus)
Message::FreezeFrameRequest { dtc_code, pids }
Message::FreezeFrameResult { dtc_code, readings }
Message::FreezeFrameError(String)
```

## New Domain State Fields

```
DomainState.readiness: Option<ReadinessStatus>
DomainState.freeze_frame_pending: bool
DomainState.freeze_frame_data: Option<Vec<(Pid, Reading)>>
```

## New Widget Types

```
WidgetKind::ReadinessPanel
```

## Not In Scope

- Mode $06 test results (no obd2-core API)
- Recording format changes for readiness/freeze-frame data
- AI enrichment changes for DTC analysis
- `clear_dtcs` during replay mode (blocked by design — no live session)
