# Active Execution Board: obd2-core API Alignment

**Date:** 2026-04-09

**Source plan:** [`2026-04-09-obd2-core-api-alignment.md`](/Users/jared/Projects/HaulLogic/obd2-dash/docs/plans/2026-04-09-obd2-core-api-alignment.md)

## Board Rules

- Status values: `READY`, `IN_PROGRESS`, `BLOCKED`, `DONE`, `DEFERRED`
- Only one task should be `IN_PROGRESS` at a time on the critical path.
- A task moves to `DONE` only after its listed verification step passes.
- If a task reveals new work, add it under `Discovered Work` before continuing.

## Current Objective

Bring `obd2-dash` into conformance with the current `obd2-core` session-first API without doing a destabilizing one-shot rewrite.

## Critical Path

1. T1 message and state types
2. T2 rich domain state
3. T3 common session runner
4. T4 core poller integration
5. T5 enhanced planning from discovery
6. T6 richer DTC integration
7. T7 UI surfacing
8. T8 bootstrap cleanup
9. T9 tests and docs cleanup

## Active Lane

| ID | Status | Task | Depends On | Exit Criteria |
|---|---|---|---|---|
| T1 | DONE | Add session-facing app/domain message types | None | App/domain can carry core connection and discovery data without lossy conversion |
| T2 | DONE | Extend domain state for rich connection/discovery state | T1 | Domain can represent protocol negotiation, ignition off, unsupported protocol, and discovery summary |
| T3 | DONE | Introduce common `session_runner` boundary | T1, T2 | Serial, BLE, emulator, and mock share one session lifecycle path |
| T4 | DONE | Replace standard PID loop with `session::poller` | T3 | Standard PID polling no longer hand-rolls `read_pid` per loop |
| T5 | DONE | Move enhanced planning onto `module_pids()` and discovery | T3 | No polling selection depends on `session.spec().enhanced_pids` |
| T6 | DONE | Upgrade DTC polling to current core API | T3 | Live DTC updates come from `read_all_dtcs()` or explicit aggregation |
| T7 | DONE | Surface discovery and session state in the UI | T2, T3 | Operator can see real session/discovery state |
| T8 | DONE | Remove duplicated bootstrap from `main.rs` | T3, T4, T5, T6 | `main.rs` is reduced to wiring and device setup |
| T9 | DONE | Add alignment tests and update docs | T4, T5, T6, T7, T8 | Translation seams are tested and docs point to the new flow |

## Task Cards

### T1

**Status:** `DONE`

**Scope:**
- add app messages for core session state and discovery
- add matching domain messages
- define whether dash stores core types directly or wraps them

**Files:**
- `crates/obd2-dash/src/app.rs`
- `crates/obd2-dash/src/domain.rs`
- optionally `crates/obd2-dash/src/session_runner.rs`

**Implementation checklist:**
- add message variants for session connection state
- add message variants for discovery snapshot
- add message variants for visible ECU and protocol metadata
- wire `AppState::update()` to forward them into domain state

**Verification:**
- `cargo check -p obd2-dash`

**Suggested commit:**
- `refactor: add session discovery and connection message types`

### T2

**Status:** `DONE`

**Scope:**
- replace or wrap the weak local connection enum
- store discovery summary in domain state
- preserve separation between session state and last error

**Files:**
- `crates/obd2-dash/src/domain.rs`
- `crates/obd2-dash/src/app.rs`
- `crates/obd2-dash/src/tui/ui.rs`

**Implementation checklist:**
- add fields for selected protocol
- add fields for protocol choice source
- add fields for visible ECUs or ECU count
- clear stale discovery data on disconnect or fatal connect error

**Verification:**
- `cargo check -p obd2-dash`
- add a small domain-state test if practical

**Suggested commit:**
- `refactor: preserve rich session and discovery state in domain`

### T3

**Status:** `DONE`

**Scope:**
- create a common runner module for all session lifecycle work
- move bootstrap and message emission out of `main.rs`

**Files:**
- `crates/obd2-dash/src/session_runner.rs`
- `crates/obd2-dash/src/main.rs`

**Implementation checklist:**
- create runner config and entry point
- move initialize logic
- move identify/VIN fallback logic
- move adapter info and discovery emission
- move raw capture command handling
- move standard loop start into the runner

**Verification:**
- `cargo check -p obd2-dash`
- `cargo run -p obd2-dash -- --mock`

**Suggested commit:**
- `refactor: introduce common session runner`

### T4

**Status:** `DONE`

**Scope:**
- adopt `session::poller::PollConfig`
- translate `PollEvent` into app messages

**Files:**
- `crates/obd2-dash/src/session_runner.rs`
- `crates/obd2-dash/src/main.rs`
- optionally `crates/obd2-dash/src/app.rs`

**Implementation checklist:**
- build `PollConfig` from `pollable_pids()`
- replace manual standard PID loop with `execute_poll_cycle()`
- map `Reading` to `PidUpdate`
- map `Voltage` to `VoltageUpdate`
- map `Error` to operator-visible error flow
- decide how to treat `Alert` without double-thresholding

**Verification:**
- `cargo check -p obd2-dash`
- `cargo run -p obd2-dash -- --mock`

**Suggested commit:**
- `refactor: switch standard PID polling to obd2-core poller`

### T5

**Status:** `DONE`

**Scope:**
- build enhanced polling work from discovery-resolved modules
- stop using raw spec internals for selection

**Files:**
- `crates/obd2-dash/src/session_runner.rs`
- optionally `crates/obd2-dash/src/domain.rs`
- optionally `crates/obd2-dash/src/tui/panel.rs`

**Implementation checklist:**
- inspect `session.discovery()` after identify
- enumerate target modules
- call `session.module_pids(module_id)`
- replace `session.spec().enhanced_pids.clone()`
- keep slower enhanced cadence until core poller expands

**Verification:**
- `cargo check -p obd2-dash`
- run against mock or a known-spec path if available

**Suggested commit:**
- `refactor: build enhanced polling from discovery modules`

### T6

**Status:** `DONE`

**Scope:**
- use richer DTC API with minimal UI churn

**Files:**
- `crates/obd2-dash/src/session_runner.rs`
- `crates/obd2-dash/src/app.rs`
- `crates/obd2-dash/src/domain.rs`
- optionally `crates/obd2-dash/src/tui/ui.rs`
- optionally `crates/obd2-dash/src/tui/panel.rs`

**Implementation checklist:**
- replace `read_dtcs()` with `read_all_dtcs()`
- avoid duplicate code presentation if aggregate results overlap
- preserve enrichment behavior if needed by the current panel and popup code
- capture follow-on work for clear/readiness/freeze-frame APIs

**Verification:**
- `cargo check -p obd2-dash`
- add at least one aggregation test if practical

**Suggested commit:**
- `feat: use read_all_dtcs for live diagnostics`

### T7

**Status:** `DONE`

**Scope:**
- surface actual session/discovery state to operators

**Files:**
- `crates/obd2-dash/src/tui/ui.rs`
- `crates/obd2-dash/src/widget/renderers.rs`
- optionally `crates/obd2-dash/src/widget/mod.rs`
- optionally `crates/obd2-dash/src/tui/panel.rs`

**Implementation checklist:**
- update footer state labels
- display protocol selection
- display visible ECU count
- distinguish disconnected, negotiating, ignition off, and unsupported protocol

**Verification:**
- `cargo check -p obd2-dash`
- manual smoke in `--mock` and one real-device path if available

**Suggested commit:**
- `feat: surface session discovery and protocol state in UI`

### T8

**Status:** `DONE`

**Scope:**
- delete duplicated bootstrap logic after runner adoption

**Files:**
- `crates/obd2-dash/src/main.rs`
- `crates/obd2-dash/src/session_runner.rs`

**Implementation checklist:**
- reduce connect paths to transport construction and runner handoff
- keep device-specific retry only where transport creation differs
- remove dead legacy loop code

**Verification:**
- `cargo check -p obd2-dash`
- `cargo run -p obd2-dash -- --mock`

**Suggested commit:**
- `refactor: remove duplicated session bootstrap from main`

### T9

**Status:** `DONE`

**Scope:**
- lock in the new integration shape with tests and doc cleanup

**Files:**
- `docs/plans/2026-03-22-obd2-core-integration.md`
- `docs/plans/2026-04-09-obd2-core-api-alignment.md`
- `README.md` if architecture text changes
- test locations in `crates/obd2-dash/src/`

**Implementation checklist:**
- mark March plan historical
- point ongoing work to the new alignment artifacts
- add tests for connection-state propagation
- add tests for discovery propagation
- add tests for poller event translation
- add tests for enhanced planning and DTC aggregation

**Verification:**
- `cargo test -p obd2-dash`

**Suggested commit:**
- `test: cover session runner translation and discovery state`

## Ready Queue

Run tasks in this order unless a blocking issue forces reordering:

1. T1
2. T2
3. T3
4. T4
5. T5
6. T6
7. T7
8. T8
9. T9

## Known Risks

- `session::poller` currently covers standard PID reads and voltage, not the full enhanced/DTC/O2 cadence.
- The dash has its own DB-backed threshold system, so core poller alerts cannot be naively merged without double-reporting.
- UI status rendering currently assumes a weak connection model and will need careful updates to avoid layout regressions.
- `main.rs` already has in-flight local changes, so refactor work needs careful file reads before editing.

## Blockers

- None yet.

## Discovered Work

- None.

## Update Log

- `2026-04-09`: Board created from the API alignment plan. No implementation tasks started yet.
- `2026-04-09`: T1-T8 implemented. `session_runner` added, standard PID polling moved onto `session::poller`, enhanced polling now comes from discovery/module resolution, DTC polling now uses `read_all_dtcs()`, and discovery/session state is surfaced in the UI.
- `2026-04-09`: T9 started. Added domain tests for connection-state mapping and disconnect cleanup, and marked the March integration plan as historical. Remaining work is dedicated `session_runner` translation coverage.
- `2026-04-09`: Review findings resolved. Serial startup now preserves retry semantics and only saves `connection.json` after successful session preparation, enhanced targets rebuild when discovery changes, and `session_runner` now has direct tests for poll-event translation, discovery emission dedupe, and discovery-driven enhanced planning. Verified with `cargo check -p obd2-dash`, `cargo test -p obd2-dash`, and `cargo run -p obd2-dash -- --mock --headless`.
