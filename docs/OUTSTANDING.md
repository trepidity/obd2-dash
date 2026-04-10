# Outstanding Items

**Last updated:** 2026-04-09

Items discovered during the API alignment work and full documentation review. These are follow-up tasks, not blockers.

---

## Diagnostics API Expansion

The alignment work upgraded from `read_dtcs()` to `read_all_dtcs()`, which aggregates stored, pending, and permanent codes. The following DTC APIs are available in `obd2-core` but not yet wired into the dash:

- **`clear_dtcs()`** -- Clear all DTCs (requires user confirmation UI)
- **`clear_dtcs_on_module(module_id)`** -- Clear DTCs for a specific ECU module
- **Readiness / test results** -- `obd2-core` exposes readiness monitors and mode $06 test results; no UI exists yet
- **Freeze-frame data** -- Mode $02 freeze-frame snapshot viewing

**Priority:** Medium. `clear_dtcs` is the most requested; readiness and freeze-frame are informational.

## Raw Capture Metadata

The raw protocol capture integration plan (2026-03-24) noted:

- **`baud_rate: None`** in `CaptureMetadata` -- The baud rate used during connection should be stored at connection time and passed as metadata when raw capture starts. Currently hardcoded to `None`.

**Priority:** Low. Only affects offline analysis tool interpretation.

## Test Coverage Gaps

While the alignment work and this review brought the test suite to comprehensive coverage of the core domain, session runner, and recording format, the following areas have limited or no tests:

### Not Tested (by design -- UI/IO-heavy)
- `tui/ui.rs`, `tui/event.rs`, `tui/panel.rs` -- Terminal rendering and input handling
- `widget/renderers.rs` -- Widget rendering dispatchers
- `scanner.rs` -- Hardware device discovery (serial/BLE)
- `main.rs` -- CLI and runtime wiring

### Could Benefit From Tests
- `recording/storage.rs` -- StorageManager compression and trimming (requires temp directory fixture)
- `recording/index.rs` -- SessionIndex load/save/remove
- `ai/client.rs`, `ai/summary.rs` -- LLM pipeline (requires HTTP mocking)
- `connection_prefs.rs` -- ConnectionPrefs load/save (requires temp file fixture)

**Priority:** Low-medium. The most valuable additions would be StorageManager and SessionIndex tests.

## Documentation

- **API documentation (`cargo doc`)** -- The crate has doc comments on all key public types but no `#![doc]` crate-level documentation. Consider adding `//!` at the top of `main.rs` or `lib.rs` if the crate is ever split.
- **NHTSA integration** -- The NHTSA VIN lookup is documented in code but not mentioned in MANUAL.md. It's currently automatic and transparent, so this is informational only.

## Historical Plans

The following plan documents are now historical references:

| Document | Status |
|----------|--------|
| `docs/plans/2026-03-22-obd2-core-design.md` | Historical -- original core design |
| `docs/plans/2026-03-22-obd2-core-implementation.md` | Historical -- original implementation |
| `docs/plans/2026-03-22-obd2-core-integration.md` | Historical -- superseded by April alignment |
| `docs/plans/2026-03-24-raw-protocol-capture-design.md` | Completed -- raw capture implemented |
| `docs/plans/2026-03-24-raw-protocol-capture-integration.md` | Completed -- all 8 tasks done |
| `docs/plans/2026-04-09-obd2-core-api-alignment.md` | Completed -- all 9 tasks done |
| `docs/plans/2026-04-09-obd2-core-api-alignment-board.md` | Completed -- execution board closed |
