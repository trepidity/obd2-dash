# Outstanding Items

**Last updated:** 2026-04-09

---

## Resolved (this session)

The following items from the original outstanding list have been implemented:

- **`clear_dtcs()`** -- Implemented with popup confirmation (broadcast) and two-key confirmation (per-module). `C` keybinding.
- **`clear_dtcs_on_module(module_id)`** -- Implemented via two-key confirmation when a specific DTC is selected.
- **Readiness monitors** -- `ReadinessPanel` widget with MIL status, DTC count, per-monitor supported/complete. Polled every 20th cycle (5s).
- **Freeze-frame data** -- On-demand Mode $02 fetch in DTC detail popup. Shows sensor snapshot when available.
- **`baud_rate` passthrough** -- Serial baud rate now flows through `CaptureReady` message to `CaptureMetadata`.
- **StorageManager tests** -- 6 tests covering register, delete, stats, maintenance trim, raw capture bytes, reload.
- **SessionIndex tests** -- 7 tests covering load/save roundtrip, remove, total size, mark compressed, sorted, duration display.
- **ConnectionPrefs tests** -- 4 tests covering missing file, serial/BLE roundtrip, invalid JSON.

## Remaining

### Not yet implemented

- **Mode $06 test results** -- `obd2-core` does not yet expose a Mode $06 API. Blocked upstream.
- **Readiness/freeze-frame recording** -- Readiness status and freeze-frame data are not captured to `.obd2rec` files. Would require a new frame type in the recording format.

### Not tested (by design -- UI/IO-heavy)

- `tui/ui.rs`, `tui/event.rs`, `tui/panel.rs` -- Terminal rendering and input handling
- `widget/renderers.rs` -- Widget rendering dispatchers
- `scanner.rs` -- Hardware device discovery (serial/BLE)
- `main.rs` -- CLI and runtime wiring

### Could benefit from tests

- `ai/client.rs`, `ai/summary.rs` -- LLM pipeline (requires HTTP mocking)

### Documentation

- **API documentation (`cargo doc`)** -- No `#![doc]` crate-level documentation.
- **NHTSA integration** -- Automatic VIN lookup not mentioned in MANUAL.md (transparent to user).

## Historical Plans

| Document | Status |
|----------|--------|
| `docs/plans/2026-03-22-obd2-core-design.md` | Historical -- original core design |
| `docs/plans/2026-03-22-obd2-core-implementation.md` | Historical -- original implementation |
| `docs/plans/2026-03-22-obd2-core-integration.md` | Historical -- superseded by April alignment |
| `docs/plans/2026-03-24-raw-protocol-capture-design.md` | Completed -- raw capture implemented |
| `docs/plans/2026-03-24-raw-protocol-capture-integration.md` | Completed -- all 8 tasks done |
| `docs/plans/2026-04-09-obd2-core-api-alignment.md` | Completed -- all 9 tasks done |
| `docs/plans/2026-04-09-obd2-core-api-alignment-board.md` | Completed -- execution board closed |
| `docs/plans/2026-04-09-outstanding-items-design.md` | Completed -- diagnostics design |
| `docs/plans/2026-04-09-outstanding-items-implementation.md` | Completed -- 13 tasks done |
| `docs/plans/2026-04-09-outstanding-items-board.md` | Completed -- execution board closed |
