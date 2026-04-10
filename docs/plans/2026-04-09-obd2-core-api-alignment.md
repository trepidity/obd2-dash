# obd2-core API Alignment Review and Implementation Plan

**Date:** 2026-04-09

**Primary reference:** `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md`

**Active execution board:** [`2026-04-09-obd2-core-api-alignment-board.md`](/Users/jared/Projects/HaulLogic/obd2-dash/docs/plans/2026-04-09-obd2-core-api-alignment-board.md)

**Goal:** Align `obd2-dash` with the current pre-`1.0` `obd2-core` integration surface. The March migration plan got the project onto external `obd2-core`, but the current library contract is stricter: the app should talk to `Session`, and `Session` should own lifecycle, discovery, routing, diagnostics, and polling.

## Review Summary

The good news: the large type migration described in the older March plan is already mostly done. `obd2-dash` now uses `Session`, `Elm327Adapter`, `MockAdapter`, `Pid`, `Reading`, and the external `obd2-core` path dependency.

The remaining work is not a wholesale migration. It is an API-alignment cleanup. The current dash still runs a custom session loop that duplicates connection bootstrap, ignores the richer `Session` discovery/connection surfaces, and reaches into session/spec internals where the current API expects the session boundary to stay in charge.

## Findings

### 1. Polling and lifecycle are still app-owned, not session-owned

The current integration guide is explicit that the application should talk to `Session`, and that `Session` owns discovery, routing, diagnostics, polling, and lifecycle.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:7`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:8`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:11`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:38`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:44`
- [`main.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs#L356)
- [`main.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs#L568)

`obd2-dash` still owns the polling schedule, cadence, bootstrap handshake, and retry logic in [`main.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs#L356). That is workable, but it means the app is still the orchestration layer instead of `Session`. The core crate now exposes a poller API, but the dash is not using it.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/poller.rs:11`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/poller.rs:46`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/poller.rs:113`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/poller.rs:136`

### 2. The dash collapses rich core connection state into a much weaker local enum

`obd2-core` now exposes a richer connection state model including `AdapterPresent`, `AdapterInitialized`, `ProtocolNegotiating`, `IgnitionOff`, and `UnsupportedProtocol`.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:134`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:146`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/mod.rs:64`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/mod.rs:151`

The dash reduces that to `Disconnected | Connecting | Connected | Error(String)` in [`domain.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/domain.rs#L49). That loses real information the UI and operator should care about, especially ignition-off reporting and discovery progress.

### 3. Discovery data exists in core, but the dash does not surface or use it

The integration guide now treats discovery as a first-class session output, including selected protocol, choice source, and visible ECUs.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:136`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:139`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/discovery.rs:8`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/discovery.rs:35`

The dash currently sends adapter info and VIN, but not discovery metadata. It therefore cannot show the operator which protocol was selected, whether the choice was auto-detected or forced, what ECUs were seen, or which logical modules were resolved.

### 4. Enhanced polling still reaches into session/spec internals instead of using the session boundary

The guide says module-targeted operations must go through `Session` because logical module names are resolved there.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:248`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:255`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:259`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/mod.rs:146`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/mod.rs:460`

The dash currently pulls `session.spec().enhanced_pids.clone()` and drives a hardcoded every-5th-loop schedule from the app side in [`main.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs#L417). That works, but it couples the app to spec layout instead of using `module_pids()` and discovery-resolved modules as the session-first surface expects.

### 5. Diagnostic integration is behind the current API surface

The current guide exposes stored, pending, permanent, and combined DTC reads.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:208`
- `/Users/jared/Projects/HaulLogic/obd2-core/docs/INTEGRATION.md:214`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/mod.rs:275`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/mod.rs:299`

The dash only polls `read_dtcs()` in [`main.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs#L517). That means the UI is still bound to the narrower legacy view of DTCs even though the library now has a richer diagnostic model.

### 6. The March integration plan is partially stale and should not drive new work

The old plan focused on the migration from inline core types to external `Session` usage. That work is largely already reflected in the codebase.

References:
- [`2026-03-22-obd2-core-integration.md`](/Users/jared/Projects/HaulLogic/obd2-dash/docs/plans/2026-03-22-obd2-core-integration.md#L5)
- [`2026-03-22-obd2-core-integration.md`](/Users/jared/Projects/HaulLogic/obd2-dash/docs/plans/2026-03-22-obd2-core-integration.md#L21)
- [`main.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs#L37)

The mismatch now is not “replace old inline core with new library.” The mismatch is “finish conforming to the current session-first API contract.”

## Implementation Plan

### Phase 1: Establish a Session-facing app boundary

Create a small session integration layer inside `obd2-dash` whose job is to translate `obd2-core` session state into app messages.

Deliverables:
- Add a session-facing module, likely `crates/obd2-dash/src/session_runner.rs`
- Move bootstrap, identify, discovery snapshot emission, and poll orchestration into that module
- Define app/domain messages for:
  - core connection state
  - discovery snapshot
  - visible ECUs
  - protocol choice source

Notes:
- Do not expose adapters or transports above this runner boundary.
- Keep device selection in the dash, but once a `Session` exists, all operational behavior should flow through the runner.

### Phase 2: Preserve the richer core connection model in domain state

Replace the current simplified `ConnectionState` in the dash with either:
- a direct mirror of `obd2_core::session::ConnectionState`, or
- a lossless wrapper enum that preserves all core states

Deliverables:
- Extend [`domain.rs`](/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/domain.rs) to store:
  - session connection state
  - discovery summary
  - visible ECU count or list
- Update the footer / status widgets to render:
  - protocol negotiation
  - ignition off
  - unsupported protocol
  - resolved protocol
  - visible ECU count

Rationale:
- This is not cosmetic. It makes the dash match the actual transport/session reality the library now reports.

### Phase 3: Replace the custom standard-PID loop with core poller primitives

Adopt `session::poller::PollConfig` and `execute_poll_cycle()` for the standard PID path.

Deliverables:
- Build `PollConfig` from the dash’s selected PID set
- Map `PollEvent::Reading`, `PollEvent::Voltage`, and `PollEvent::Error` into existing app messages
- Use the core poller for the standard PID cadence

Important constraint:
- `PollEvent` already has an `EnhancedReading` variant, but `execute_poll_cycle()` currently only emits standard PID reads and voltage.

References:
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/poller.rs:18`
- `/Users/jared/Projects/HaulLogic/obd2-core/crates/obd2-core/src/session/poller.rs:135`

Inference from source:
- We should not block the refactor on full enhanced-poller support. Use the core poller now for the standard path, and keep a controlled secondary schedule for enhanced/DTC/O2 work until the core surface covers them cleanly.

### Phase 4: Stop peeking into `session.spec().enhanced_pids`

Move enhanced PID planning to the session-owned APIs.

Deliverables:
- After `identify_vehicle()`, inspect `session.discovery()` and resolved modules
- Build module-targeted polling work from `session.module_pids(ModuleId)`
- Use `read_enhanced()` only through module IDs resolved by the current discovery state

Why this matters:
- It keeps module routing and spec interpretation inside `obd2-core`
- It avoids stale assumptions if routing rules or spec loading change

### Phase 5: Upgrade diagnostics to the richer session API

Bring the DTC and diagnostic paths in line with the current session surface.

Deliverables:
- Decide whether the primary DTC panel should show:
  - `read_all_dtcs()`, or
  - separate stored / pending / permanent buckets in the UI
- Add support paths for:
  - `read_pending_dtcs()`
  - `read_permanent_dtcs()`
  - `clear_dtcs()`
  - `clear_dtcs_on_module()`
- Evaluate whether detail popups should incorporate:
  - `read_vehicle_info()`
  - `read_readiness()`
  - `read_test_results()`
  - freeze-frame data

Recommendation:
- Use `read_all_dtcs()` as the operational default first. It closes the biggest gap without forcing a UI redesign in the same patch.

### Phase 6: Collapse duplicate bootstrap paths into one runner

Today the serial, BLE, emulator, and mock paths each partially duplicate setup and message emission.

Deliverables:
- Introduce one common runner entry that accepts a constructed adapter and returns a task handle
- Keep only transport construction device-specific
- Move:
  - initialize
  - adapter info emission
  - identify/discovery emission
  - capture wiring
  - poll loop start
  into the common runner

Benefits:
- one lifecycle path
- one place to map core API changes
- less divergence between serial, BLE, emulator, and mock behavior

### Phase 7: Update documentation and add alignment tests

The repo now needs docs and tests for the current API shape, not the March migration state.

Deliverables:
- Mark [`2026-03-22-obd2-core-integration.md`](/Users/jared/Projects/HaulLogic/obd2-dash/docs/plans/2026-03-22-obd2-core-integration.md) as historical / partially completed
- Point future work to this plan
- Add tests for:
- session discovery snapshot propagation
- ignition-off state propagation
- poller event translation
- enhanced PID planning from `module_pids()`
- DTC aggregation via `read_all_dtcs()`

## Detailed Task List

### Task 1: Add session-facing app/domain message types

**Goal:** Give the app a lossless way to receive `obd2-core` lifecycle and discovery data.

**Files:**
- Modify: `crates/obd2-dash/src/app.rs`
- Modify: `crates/obd2-dash/src/domain.rs`
- Possibly create: `crates/obd2-dash/src/session_runner.rs`

**Implementation steps:**
1. Add app messages for:
   - core session connection state
   - discovery snapshot
   - visible ECU updates
   - protocol/discovery errors that should be operator-visible
2. Add matching domain messages.
3. Decide whether to store the core types directly or wrap them in dash-local structs.
4. Keep the old simplified `ConnectionStatus` path only long enough to preserve incremental compilation.

**Design constraints:**
- Do not throw away `IgnitionOff` or `UnsupportedProtocol`.
- Do not store only strings if the UI may need structured access later.

**Acceptance criteria:**
- `AppState::update()` can ingest new session/discovery messages.
- `DomainState` can store current session status and discovery details without lossy conversion.

**Verification:**
- `cargo check -p obd2-dash`

### Task 2: Extend domain state for rich connection/discovery state

**Goal:** Replace the current weak connection model with something that can represent the real session state machine.

**Files:**
- Modify: `crates/obd2-dash/src/domain.rs`
- Modify: `crates/obd2-dash/src/app.rs`
- Modify: `crates/obd2-dash/src/tui/ui.rs`

**Implementation steps:**
1. Replace or wrap `domain::ConnectionState`.
2. Add fields for:
   - current core connection state
   - selected protocol
   - protocol choice source
   - visible ECUs
   - resolved module count
3. Update domain transitions to clear stale discovery state on disconnect/error.
4. Ensure the last-known session state is preserved for debugging even after non-fatal read errors.

**Design constraints:**
- Distinguish transport/session state from last read error.
- Do not regress existing footer behavior for disconnected scanning.

**Acceptance criteria:**
- Domain state can represent `ProtocolNegotiating`, `IgnitionOff`, and `UnsupportedProtocol`.
- Discovery metadata survives after identification and is visible to the UI.

**Verification:**
- `cargo check -p obd2-dash`
- Add a unit test for domain transitions if practical.

### Task 3: Introduce a common `session_runner` boundary

**Goal:** Centralize all post-adapter behavior behind one runner so `main.rs` stops duplicating lifecycle logic.

**Files:**
- Create: `crates/obd2-dash/src/session_runner.rs`
- Modify: `crates/obd2-dash/src/main.rs`

**Implementation steps:**
1. Create a runner entry point such as:
   - `run_session_task(session, config, tx, capture_rx, capture_handle)`
2. Move into the runner:
   - initialization
   - adapter info emission
   - identify/read VIN fallback
   - discovery snapshot emission
   - raw capture start/stop handling
   - standard polling loop start
3. Keep transport construction in `main.rs`, but hand off immediately once the adapter/session exists.
4. Reuse the same runner for serial, BLE, emulator, and mock.

**Design constraints:**
- Do not expose adapters/transports back into UI code.
- Keep mock and real transport behavior as close as possible.

**Acceptance criteria:**
- Serial, BLE, emulator, and mock all pass through one session runner path.
- The runner emits the same or richer app messages than the current ad hoc loop.

**Verification:**
- `cargo check -p obd2-dash`
- Manual smoke: `cargo run -p obd2-dash -- --mock`

### Task 4: Replace the standard PID loop with `session::poller`

**Goal:** Move the standard Mode 01 polling path onto the core poller primitives.

**Files:**
- Modify: `crates/obd2-dash/src/session_runner.rs`
- Modify: `crates/obd2-dash/src/main.rs`
- Possibly modify: `crates/obd2-dash/src/app.rs`

**Implementation steps:**
1. Build `PollConfig` from `pollable_pids()` and the current poll interval.
2. Call `execute_poll_cycle()` instead of hand-rolling the standard PID read loop.
3. Translate `PollEvent` into `Message` values:
   - `Reading` -> `PidUpdate`
   - `Voltage` -> `VoltageUpdate`
   - `Error` -> `Error`
   - `Alert` -> either new alert message or keep domain-side thresholding for now
4. Preserve current tick cadence until interval adjustment is fully runner-owned.

**Design constraints:**
- Avoid double-thresholding. If the dash still uses its own DB threshold system, do not also surface core `Alert` as if it were the same thing without a clear merge strategy.
- Keep non-fatal PID failures non-fatal.

**Acceptance criteria:**
- Standard PID reads are no longer manually looped with `session.read_pid(pid)` in the runner.
- Poll results still reach domain state correctly.

**Verification:**
- `cargo check -p obd2-dash`
- Mock run confirms live values still update.

### Task 5: Move enhanced PID planning onto `module_pids()` and discovery

**Goal:** Stop coupling the dash to raw spec layout for enhanced reads.

**Files:**
- Modify: `crates/obd2-dash/src/session_runner.rs`
- Possibly modify: `crates/obd2-dash/src/domain.rs`
- Possibly modify: `crates/obd2-dash/src/tui/panel.rs`

**Implementation steps:**
1. After `identify_vehicle()`, inspect `session.discovery()`.
2. For each resolved module of interest, call `session.module_pids(module_id)`.
3. Build the enhanced poll schedule from those returned definitions.
4. Replace `session.spec().enhanced_pids.clone()` usage.
5. Keep the current slower cadence for enhanced reads unless/until core poller support is expanded.

**Design constraints:**
- Skip unresolved modules cleanly.
- Do not assume `ecm` exists on every vehicle/spec.

**Acceptance criteria:**
- No app-side dependency remains on `session.spec().enhanced_pids` for polling selection.
- Enhanced reads only target modules resolved by the active discovery/profile state.

**Verification:**
- `cargo check -p obd2-dash`
- Mock or known-spec run confirms enhanced values still populate.

### Task 6: Upgrade DTC polling to the current core API

**Goal:** Use the richer diagnostic surface without forcing an immediate full UI redesign.

**Files:**
- Modify: `crates/obd2-dash/src/session_runner.rs`
- Modify: `crates/obd2-dash/src/app.rs`
- Modify: `crates/obd2-dash/src/domain.rs`
- Possibly modify: `crates/obd2-dash/src/tui/ui.rs`
- Possibly modify: `crates/obd2-dash/src/tui/panel.rs`

**Implementation steps:**
1. Replace periodic `read_dtcs()` with `read_all_dtcs()` in the runner.
2. If needed, add domain fields for stored/pending/permanent buckets later, but keep the first step simple.
3. Preserve DTC enrichment behavior if the current UI depends on descriptions/severity.
4. Add follow-up TODOs or backlog tasks for:
   - `clear_dtcs()`
   - `clear_dtcs_on_module()`
   - readiness/test result/freeze-frame UI

**Design constraints:**
- Avoid regressing current DTC panel rendering.
- Do not silently duplicate DTCs if `read_all_dtcs()` returns overlapping categories.

**Acceptance criteria:**
- Live DTC updates come from `read_all_dtcs()` or an explicitly documented aggregation path.
- Existing DTC panel and detail popup still compile and render.

**Verification:**
- `cargo check -p obd2-dash`
- Add at least one test for DTC aggregation behavior if practical.

### Task 7: Surface discovery and session state in the UI

**Goal:** Make the richer session/discovery information visible to the operator.

**Files:**
- Modify: `crates/obd2-dash/src/tui/ui.rs`
- Modify: `crates/obd2-dash/src/widget/renderers.rs`
- Possibly modify: `crates/obd2-dash/src/widget/mod.rs`
- Possibly modify: `crates/obd2-dash/src/tui/panel.rs`

**Implementation steps:**
1. Update footer status rendering to include richer states:
   - negotiating
   - ignition off
   - unsupported protocol
2. Add protocol/discovery summary somewhere low-noise:
   - footer
   - system info widget
   - new discovery widget if necessary
3. Show visible ECU count and selected protocol once available.
4. Keep scan instructions visible only when appropriate.

**Design constraints:**
- Do not overload the footer with multi-line noise if the same information fits better in a widget.
- Preserve compact layout readability.

**Acceptance criteria:**
- UI can visibly distinguish disconnected vs ignition-off vs protocol-negotiating.
- Protocol and ECU visibility are available somewhere operator-facing.

**Verification:**
- `cargo check -p obd2-dash`
- Manual mock/real-device smoke test.

### Task 8: Remove duplicated bootstrap and retry logic from `main.rs`

**Goal:** Make `main.rs` responsible only for argument parsing, device selection, and task wiring.

**Files:**
- Modify: `crates/obd2-dash/src/main.rs`
- Modify: `crates/obd2-dash/src/session_runner.rs`

**Implementation steps:**
1. Reduce each connect path to:
   - construct transport
   - wrap transport if needed
   - construct adapter/session
   - call common runner
2. Consolidate connection success/failure message emission inside the runner where possible.
3. Keep device-specific retry behavior only where transport creation actually differs.
4. Delete dead code paths left behind by the old custom loop.

**Design constraints:**
- Do not widen the runner until it starts owning device scanning; scanning remains dash-owned.
- Avoid merging unrelated cleanup into this task.

**Acceptance criteria:**
- `main.rs` no longer contains multiple partially duplicated session lifecycle flows.
- Mock, serial, BLE, and emulator still build and dispatch through one operational path.

**Verification:**
- `cargo check -p obd2-dash`
- Manual smoke for at least `--mock`

### Task 9: Add alignment tests and update documentation

**Goal:** Lock in the new integration shape and prevent drift back to app-owned session semantics.

**Files:**
- Modify: `docs/plans/2026-03-22-obd2-core-integration.md`
- Modify: `README.md` if integration architecture text needs correction
- Add tests in:
  - `crates/obd2-dash/src/session_runner.rs`
  - `crates/obd2-dash/src/domain.rs`
  - dedicated integration tests if added later

**Implementation steps:**
1. Mark the March plan as historical/partially completed.
2. Point future work to this alignment plan.
3. Add tests for:
   - connection-state propagation
   - discovery snapshot propagation
   - poller event translation
   - enhanced module planning
   - DTC aggregation path
4. If adding tests inside `main.rs` is awkward, keep translation logic in testable helper functions inside `session_runner.rs`.

**Design constraints:**
- Prefer small deterministic tests over broad async integration tests first.
- Do not leave the new runner untested; that is the refactor seam most likely to drift.

**Acceptance criteria:**
- The repo has a current plan and historical note, not two competing “active” integration plans.
- At least the translation seams have direct tests.

**Verification:**
- `cargo test -p obd2-dash`

## Recommended Landing Sequence

Land the work in this order:

1. Task 1
2. Task 2
3. Task 3
4. Task 4
5. Task 5
6. Task 6
7. Task 7
8. Task 8
9. Task 9

This order keeps the refactor on stable seams:
- message/domain shape first
- common runner second
- polling changes after the runner exists
- UI after state is stable
- test/doc cleanup last

## Suggested Commit Breakdown

If this work is split across multiple commits, use boundaries like:

1. `refactor: add session discovery and connection message types`
2. `refactor: introduce common session runner`
3. `refactor: switch standard PID polling to obd2-core poller`
4. `refactor: build enhanced polling from discovery modules`
5. `feat: use read_all_dtcs for live diagnostics`
6. `feat: surface session discovery and protocol state in UI`
7. `docs: mark old obd2-core integration plan historical`
8. `test: cover session runner translation and discovery state`

## Proposed Execution Order

1. Add the session runner and richer app/domain messages.
2. Switch standard PID polling to `session::poller`.
3. Surface discovery and connection-state details in the UI.
4. Move enhanced PID planning off `session.spec().enhanced_pids`.
5. Upgrade diagnostics from `read_dtcs()` to `read_all_dtcs()` plus follow-on surfaces.
6. Clean up stale docs and add alignment tests.

## Scope Boundaries

This plan does **not** require:
- rewriting the scanner into `obd2-core`
- removing dash-owned UI state
- replacing dash-specific recording/replay
- adopting every optional session API in a single patch

This plan **does** require:
- treating `Session` as the operational boundary
- preserving core discovery and lifecycle semantics in the dash
- reducing direct app-side knowledge of spec and routing internals
