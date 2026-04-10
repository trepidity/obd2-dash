# Active Execution Board: Outstanding Items Resolution

**Date:** 2026-04-09

**Source plan:** [`2026-04-09-outstanding-items-implementation.md`](2026-04-09-outstanding-items-implementation.md)
**Design doc:** [`2026-04-09-outstanding-items-design.md`](2026-04-09-outstanding-items-design.md)

## Board Rules

- Status values: `READY`, `IN_PROGRESS`, `BLOCKED`, `DONE`, `DEFERRED`
- Only one task should be `IN_PROGRESS` at a time on the critical path.
- A task moves to `DONE` only after its listed verification step passes.
- If a task reveals new work, add it under `Discovered Work` before continuing.

## Current Objective

Resolve all outstanding items from `docs/OUTSTANDING.md`: diagnostics expansion (readiness monitors, clear DTCs, freeze-frame), raw capture baud_rate fix, and filesystem test coverage.

## Phase Overview

| Phase | Scope | Tasks | Risk |
|-------|-------|-------|------|
| **1 — Quick Wins** | Baud rate fix, tempfile dep, filesystem tests | T1–T5 | Low |
| **2 — Diagnostics** | Readiness, clear DTCs, freeze-frame | T6–T11 | Medium |
| **3 — Tests & Docs** | Coverage for Phase 2, doc updates | T12–T13 | Low |

## Critical Path

1. T1 baud rate fix
2. T2 tempfile dev-dependency
3. T3 SessionIndex tests
4. T4 StorageManager tests
5. T5 ConnectionPrefs tests
6. T6 readiness message types + domain state
7. T7 poll_readiness in session runner
8. T8 ReadinessPanel widget
9. T9 clear DTC command channel
10. T10 clear DTC UI (popup + two-key)
11. T11 freeze-frame in DTC popup
12. T12 diagnostics tests
13. T13 documentation updates

## Active Lane

| ID | Status | Task | Phase | Depends On | Exit Criteria |
|----|--------|------|-------|------------|---------------|
| T1 | DONE | Fix baud_rate passthrough in raw capture metadata | 1 | None | `CaptureMetadata.baud_rate` populated from serial baud, `cargo check` passes |
| T2 | DONE | Add tempfile dev-dependency | 1 | None | `tempfile = "3"` in `[dev-dependencies]`, `cargo check` passes |
| T3 | DONE | Add SessionIndex tests | 1 | T2 | 7 tests pass: load missing, save/load roundtrip, remove, total size, mark compressed, sorted, duration display |
| T4 | DONE | Add StorageManager tests | 1 | T2 | 6 tests pass: register, delete, stats, maintenance trim, raw capture bytes, reload |
| T5 | DONE | Add ConnectionPrefs tests | 1 | T2 | 4 tests pass: missing file, serial roundtrip, BLE roundtrip, invalid JSON |
| T6 | DONE | Add readiness message types and domain state | 2 | None | `ReadinessUpdate` message flows through app → domain, domain test passes |
| T7 | DONE | Add poll_readiness to session runner | 2 | T6 | `poll_readiness` called every 20th cycle, `cargo check` passes |
| T8 | DONE | Add ReadinessPanel widget | 2 | T6, T7 | Widget renders MIL/monitors, visible in `--mock` mode via edit mode |
| T9 | DONE | Add clear DTC command channel and session runner handling | 2 | T6 | `DiagnosticCommand` channel created, `ClearAll`/`ClearOnModule` handled in runner |
| T10 | DONE | Add clear DTC UI (popup + two-key) | 2 | T9 | `C` in DTC panel shows popup, Enter confirms, Esc cancels; per-module two-key works |
| T11 | DONE | Add freeze-frame to DTC detail popup | 2 | T9 | DTC popup shows freeze-frame section when data available, "Loading..." while pending |
| T12 | DONE | Add tests for diagnostics features | 3 | T6–T11 | All new domain transitions and runner operations have tests, `cargo test` passes |
| T13 | DONE | Update documentation | 3 | T8, T10, T11 | README, MANUAL, OUTSTANDING.md updated with new features and keybindings |

## Task Cards

### T1 — Fix baud_rate passthrough

**Status:** `READY`

**Scope:**
- Add `serial_baud_rate: Option<u32>` to `AppState`
- Set it when serial transport is created (all serial paths including emulator and scanner)
- Use it in `handle_toggle_raw_capture` instead of `None`

**Files:**
- `crates/obd2-dash/src/app.rs` — add field + init
- `crates/obd2-dash/src/main.rs:1508` — use `state.serial_baud_rate`
- `crates/obd2-dash/src/main.rs:430` (approx) — set after serial transport success

**Verification:**
- `cargo check -p obd2-dash`

**Commit:** `fix: pass serial baud_rate through to raw capture metadata`

---

### T2 — Add tempfile dev-dependency

**Status:** `READY`

**Scope:**
- Add `[dev-dependencies]` section with `tempfile = "3"` to obd2-dash Cargo.toml

**Files:**
- `crates/obd2-dash/Cargo.toml`

**Verification:**
- `cargo check -p obd2-dash`

**Commit:** `chore: add tempfile dev-dependency for filesystem tests`

---

### T3 — SessionIndex tests

**Status:** `READY`

**Scope:**
- 7 tests in `recording/index.rs`: load missing file, save/load roundtrip, remove session, total size, mark compressed, sorted order, duration display
- All use `tempfile::tempdir()` for isolation

**Files:**
- `crates/obd2-dash/src/recording/index.rs` — add `#[cfg(test)] mod tests`

**Verification:**
- `cargo test -p obd2-dash recording::index::tests`

**Commit:** `test: add SessionIndex filesystem roundtrip tests`

---

### T4 — StorageManager tests

**Status:** `READY`

**Scope:**
- 6 tests in `recording/storage.rs`: register + persist, delete + cleanup, stats, maintenance FIFO trim, raw capture bytes counting, reload index
- Tests create real files in tempdir to exercise actual compression/deletion paths

**Files:**
- `crates/obd2-dash/src/recording/storage.rs` — add `#[cfg(test)] mod tests`

**Verification:**
- `cargo test -p obd2-dash recording::storage::tests`

**Commit:** `test: add StorageManager filesystem tests`

---

### T5 — ConnectionPrefs tests

**Status:** `READY`

**Scope:**
- 4 tests in `connection_prefs.rs`: missing file default, serial roundtrip, BLE roundtrip, invalid JSON graceful default
- Tests exercise JSON serialization of `DeviceKind` enum variants

**Files:**
- `crates/obd2-dash/src/connection_prefs.rs` — add `#[cfg(test)] mod tests`

**Verification:**
- `cargo test -p obd2-dash connection_prefs::tests`

**Commit:** `test: add ConnectionPrefs load/save roundtrip tests`

---

### T6 — Readiness message types and domain state

**Status:** `READY`

**Scope:**
- Import `obd2_core::protocol::service::ReadinessStatus` in domain and app
- Add `DomainMessage::ReadinessUpdate(ReadinessStatus)`
- Add `Message::ReadinessUpdate(ReadinessStatus)`
- Add `DomainState.readiness: Option<ReadinessStatus>`
- Handle in `DomainState::update()` — store on update, clear on disconnect
- Forward in `AppState::update()`
- Add domain test: readiness stored and cleared on disconnect

**Files:**
- `crates/obd2-dash/src/domain.rs`
- `crates/obd2-dash/src/app.rs`

**Verification:**
- `cargo test -p obd2-dash domain::tests`

**Commit:** `feat: add readiness monitor message types and domain state`

---

### T7 — poll_readiness in session runner

**Status:** `READY`

**Scope:**
- Add `poll_readiness()` async fn calling `session.read_readiness()`
- Wire into `cycle % 20` block alongside `poll_o2_monitoring`

**Files:**
- `crates/obd2-dash/src/session_runner.rs`

**Verification:**
- `cargo check -p obd2-dash`

**Commit:** `feat: poll readiness monitors every 20th cycle`

---

### T8 — ReadinessPanel widget

**Status:** `READY`

**Scope:**
- Add `WidgetKind::ReadinessPanel` to enum and registry with `WidgetCategory::Diagnostics`
- Add render function: MIL (red/green), DTC count, ignition type, per-monitor rows (OK/-- with color)
- Wire into `render_widget` dispatcher

**Files:**
- `crates/obd2-dash/src/widget/mod.rs`
- `crates/obd2-dash/src/widget/renderers.rs`

**Verification:**
- `cargo check -p obd2-dash`
- `cargo run -p obd2-dash -- --mock` — add widget via edit mode, verify rendering

**Commit:** `feat: add ReadinessPanel widget with MIL and monitor status`

---

### T9 — Clear DTC command channel

**Status:** `READY`

**Scope:**
- Add `DiagnosticCommand` enum: `ClearAll`, `ClearOnModule(ModuleId)`, `FetchFreezeFrame { dtc_code, pids }`
- Add `Message::DiagnosticReady`, `ClearDtcsComplete`, `ClearDtcsError(String)`
- Add `diagnostic_tx: Option<mpsc::UnboundedSender<DiagnosticCommand>>` to `AppState`
- Create channel in `main.rs`, send sender via `DiagnosticReady`, pass receiver to runner
- Handle `ClearAll` and `ClearOnModule` in session runner loop (drain like `capture_rx`)

**Files:**
- `crates/obd2-dash/src/app.rs`
- `crates/obd2-dash/src/domain.rs`
- `crates/obd2-dash/src/session_runner.rs`
- `crates/obd2-dash/src/main.rs`

**Verification:**
- `cargo check -p obd2-dash`

**Commit:** `feat: add clear DTC command channel and session runner handling`

---

### T10 — Clear DTC UI

**Status:** `READY`

**Scope:**
- Add `ClearDtcConfirm` enum to `app.rs`: `BroadcastPopup`, `ModulePending { module_id, expires }`
- Handle `C` keypress: no selection → broadcast popup, selected DTC → two-key with 2s expiry
- Render broadcast confirmation popup in `tui/ui.rs`
- Show footer flash for module-pending state
- Enter confirms → send `DiagnosticCommand::ClearAll`, Esc cancels

**Files:**
- `crates/obd2-dash/src/app.rs`
- `crates/obd2-dash/src/main.rs` (key handler)
- `crates/obd2-dash/src/tui/ui.rs` (popup rendering)

**Verification:**
- `cargo run -p obd2-dash -- --mock` — press `d` for DTCs, focus DTC panel, press `C`, verify popup, Enter/Esc

**Commit:** `feat: add clear DTC UI with popup and two-key confirmation`

---

### T11 — Freeze-frame in DTC detail popup

**Status:** `READY`

**Scope:**
- Add `FreezeFrameSnapshot` struct to `domain.rs` with `dtc_code` and `readings: Vec<(Pid, f64, &'static str)>`
- Add `freeze_frame_pending: bool` and `freeze_frame_data: Option<FreezeFrameSnapshot>` to `DomainState`
- Add `Message::FreezeFrameResult(FreezeFrameSnapshot)` and `Message::FreezeFrameError(String)`
- Handle `FetchFreezeFrame` command in session runner: call `session.read_freeze_frame(pid, 0)` per PID
- On DTC popup open (Enter in main.rs), send `FetchFreezeFrame` with correlated PIDs
- In `tui/panel.rs` `build_popup`, append freeze-frame section if data matches current DTC
- Clear freeze-frame data on disconnect

**Files:**
- `crates/obd2-dash/src/domain.rs`
- `crates/obd2-dash/src/app.rs`
- `crates/obd2-dash/src/session_runner.rs`
- `crates/obd2-dash/src/tui/panel.rs`
- `crates/obd2-dash/src/main.rs`

**Verification:**
- `cargo check -p obd2-dash`
- `cargo run -p obd2-dash -- --mock` — select DTC, Enter, verify popup has freeze-frame section

**Commit:** `feat: add freeze-frame data to DTC detail popup (on-demand)`

---

### T12 — Diagnostics tests

**Status:** `READY`

**Scope:**
- Domain test: readiness update + clear on disconnect (if not already in T6)
- Domain test: freeze-frame stored and cleared on disconnect
- Domain test: clear DTCs clears stored_dtcs
- Session runner: verify `poll_readiness` sends message (if practical with MockAdapter)

**Files:**
- `crates/obd2-dash/src/domain.rs` (test module)
- `crates/obd2-dash/src/session_runner.rs` (test module)

**Verification:**
- `cargo test -p obd2-dash`

**Commit:** `test: add diagnostics expansion tests`

---

### T13 — Documentation updates

**Status:** `READY`

**Scope:**
- README: add ReadinessPanel to widget list, `C` keybinding, freeze-frame mention, update test count
- MANUAL: update Section 9 (DTC) with clear and freeze-frame, add readiness description, `C` in keyboard reference
- OUTSTANDING.md: mark diagnostics, baud_rate, and test items as completed; keep Mode $06 as remaining

**Files:**
- `README.md`
- `MANUAL.md`
- `docs/OUTSTANDING.md`

**Verification:**
- Manual review of rendered markdown

**Commit:** `docs: update for diagnostics expansion (readiness, clear DTCs, freeze-frame)`

---

## Ready Queue

Execute tasks in this order:

**Phase 1 (can be parallelized: T1 independent of T2–T5):**
1. T1 + T2 (parallel — no dependencies between them)
2. T3, T4, T5 (sequential — all depend on T2 for tempfile)

**Phase 2 (sequential critical path):**
3. T6 (message types — foundation for T7–T11)
4. T7 (poll_readiness — depends on T6)
5. T8 (widget — depends on T6, T7)
6. T9 (clear DTC commands — depends on T6)
7. T10 (clear DTC UI — depends on T9)
8. T11 (freeze-frame — depends on T9)

**Phase 3 (after all Phase 2):**
9. T12 (tests)
10. T13 (docs)

## Known Risks

- `ReadinessStatus` is used directly from `obd2-core`. If the struct changes upstream, the dash will need updating. Consider a local snapshot type if this becomes an issue.
- Mock adapter may not implement `read_readiness()` or `read_freeze_frame()` — tests may need to handle `Err` gracefully. Verify MockAdapter behavior before writing runner tests.
- The `DiagnosticCommand` channel adds a second command channel alongside `CaptureCommand`. If more command types are needed later, consider unifying into a single `SessionCommand` enum.
- Clear DTCs resets readiness monitors on real vehicles — the confirmation UI must be clear about this consequence.
- Freeze-frame data may be empty on many vehicles (not all ECUs store it). The UI must handle the no-data case gracefully.

## Blockers

- None.

## Discovered Work

- None yet.

## Update Log

- `2026-04-09`: Board created from the outstanding items implementation plan. All 13 tasks in READY state.
- `2026-04-09`: All 13 tasks implemented and verified. Phase 1: baud_rate fix, tempfile dep, 17 filesystem tests. Phase 2: readiness polling + widget, clear DTC command channel + UI, freeze-frame popup. Phase 3: diagnostics tests, full doc update. 98 total tests passing.
