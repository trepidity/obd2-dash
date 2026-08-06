# Scan Modes Implementation Plan

> Execute tasks in order. Each task is a self-contained TDD cycle. Do not start
> GUI deletion until the shared runner has equivalent snapshot and diagnostic
> coverage.

**Goal:** Replace GUI-owned serial sweeps with one reconnecting background
runner that performs capability-pruned telemetry, foreground diagnostics, and
atomic per-vehicle capability persistence.

**Design:** `docs/superpowers/specs/2026-07-24-scan-modes-design.md`

**Traceability:**

**Slice 5 (`f5be016`, audited + fixed in `787d4be`):** ordered diagnostic
request plan: DTC services, Mode-02 freeze frames, readiness, conditional
Mode-05, module refresh; Mode-06 absent, Mode-05 fuel/protocol gated. Audit
fix: readiness now skips when its cached service row is `unsupported`
(spec §11; unverified is still attempted) — gate inputs are a named
`ServiceGates` struct. Wire-execution notes for the next slice: the plan is
service-level only — the DTC phase must fan out broadcast-then-per-module
including the selected-profile DTC path and GM Class-2 backoff; the
freeze-frame phase expands to one substep per code found (sub-total known
only after the DTC phase); readiness/module-refresh sharing `service: 0x01`
means the execution contract needs a richer request model than a bare
service byte.

**Slice 6 (`b261340`, audited + fixed in `a232da0`):** requests carry
target scope (Broadcast / DiscoveredModules) and expansion (Static /
PerDtc). Audit fix: the DTC phase collapsed the scan matrix — stored (03)
was broadcast-only and pending/permanent (07/0A) module-only, silently
dropping per-module stored codes and broadcast pending/permanent. The plan
now emits the full 3×2 matrix (broadcast S/P/P, then per-module S/P/P),
matching spec §11 and the TUI's `scan_standard_dtcs`, pinned by an
exact-sequence test. Wire execution must still add the selected-profile
DTC path (ProfileRuntime, not a raw service byte) with GM Class-2 backoff,
and iterate DiscoveredModules groups module-major to match legacy capture
ordering.

**Slice 7 (`a9de1c8`, audited + fixed in `fa6928e`):** DTC wire-order
expansion (broadcast trio, then module-major trios). Audit fix: the
expansion discarded the module binding — N modules produced N
indistinguishable `DiscoveredModules` triples an executor could count but
never address — and ordering was whatever the caller passed. Targets now
carry `Module(index)` into the caller's slice, emitted in id-sorted order
(matching `dtc_scan_modules`), and `expand_dtc_requests` documents that it
REPLACES `request_plan`'s six DTC summary rows so broadcast is never
double-scanned. Selected-profile DTC routing and GM Class-2 backoff remain
wire-execution obligations.

**Slice 8 (`b2e471b`, audited + fixed in `fc91f40`):** request-boundary
diagnostic executor over `DiagnosticTransport`; cancellation observed only
between completed requests; no in-flight future dropped. Audit fix: the
executor aborted the whole bundle on ANY transport error — spec §11 records
non-transport step failures (NO DATA, negative responses) and continues,
aborting only on transport loss. The contract now returns `StepResult`
(`Data` | `StepError`) with `Err` reserved for transport loss, and an abort
carries partial progress (`DiagnosticAborted`) so the interrupted result is
reportable per §13. Session binding must map `Obd2Error` accordingly:
transport-class loss → `Err`; everything else → `StepError`.

**Slice 9 (`a14867b`, audited + fixed in the follow-up commit):**
`map_obd2_result` binds core errors to the executor contract. Audit fix:
only `Obd2Error::Transport` counted as link loss — `Obd2Error::Io` (how a
yanked USB adapter actually surfaces through mio-serial) was recorded as a
step error, grinding the bundle through remaining requests at full timeout
instead of aborting to reconnect. `Io` now aborts alongside `Transport`;
`Timeout` deliberately remains a step error per the §8.2 error taxonomy.
Remaining: wire requests to Session/ProfileRuntime operations (including
GM Class-2 backoff) and persist diagnostic outcomes.

**Slice 10 (`bbf6960`, audited + fixed in the follow-up commit):**
`capability_outcome` maps step results to persisted capability state with
separated NO DATA confirmation. Audit fix: the classifier matched error
DISPLAY strings and every non-Data arm was dead code against real core
errors (`NoData` renders "no data (vehicle did not respond)",
`UnsupportedPid` renders "not supported", negative responses render
Debug-style with no spaces) — its tests fed invented strings. All step
errors resolved Unverified, so no service could ever be pruned.
`StepResult::StepError` now carries a typed `StepErrorKind` assigned from
the typed `Obd2Error` in `map_obd2_result`; the outcome test routes real
core errors through the mapper end to end.

**Slice 11 (`3052c65`, audited + fixed in the follow-up commit):** real
foreground execution — diagnostics through `execute_session_request` (gate
enforced at the wire), profile DTC via `ProfileRuntime`, typed outcome
persistence with separated NO DATA, staged rescan honoring §9.1/§9.5
semantics, shutdown flush after session drop, narrow raw-request allowlist
extension. Audit fixes: persisted diagnostic module keys used
session-local indices (`module-0`) instead of canonical ids — reordering
between sessions would attach cached outcomes to the wrong module; and
`is_lly_profile` used a substring match. Prominent remaining gaps:
**decoded DTC payloads are discarded — a diagnostic scan currently
persists service support but surfaces no codes to the operator** (freeze
frames therefore structurally skip, and `ProfileResponse::Dtcs` maps to
empty `Data`); profile DTC evidence goes to `NullEvidenceSink` (the legacy
GUI records evidence — GUI-0001 must wire a real sink before deleting it);
profile DTC services do not consult cached-unsupported service rows;
Mode-05 runs as a bare `0x05` probe rather than `read_all_o2_monitoring`
enumeration. Executor's own honest list also open: bounded async command
channel with oneshot acks, concurrent in-flight cancellation control,
`RequestActiveTest` routing.

**Slice 12 (TASK-DASH-0004 closure):** retained typed standard and profile
DTC results in the runner snapshot, with decoded-code-correlated Mode-02
freeze-frame work; Mode-05 now delegates its TID/sensor matrix to
`Session::read_all_o2_monitoring`. The bounded capacity-8 control plane uses
oneshot replies and a watch-backed view value, observes cancellation/shutdown
only at request boundaries, and treats channel close as orderly shutdown.
Locked active tests route through that control plane, write evidence in
`spawn_blocking`, and remain structurally unable to issue a Session request.
The runner's `run_once` is the single-session execution entry point for the
control receiver. Verified with the full `obd2-dash` test suite, strict
clippy, fmt, and architectural import gates.

**Closure audit (2026-08-02): TASK-DASH-0004 CONFIRMED CLOSED.** All
behaviors verified in code; the executor's channel-level tests were
supplemented with three runner-level contracts the closure lacked:
`channel_cancel_is_observed_at_the_next_request_boundary` (gated in-flight
request; cancel takes effect after it completes, nothing starts after),
`shutdown_command_acks_after_session_release`, and
`closed_control_channel_shuts_down_instead_of_reconnecting` (connector
never re-invoked). `CancelForeground` was verified to set the boundary
flag rather than flipping the mode under an in-flight bundle (the slice-1
behavior would have left the wire gate denying mid-bundle requests).
Carried forward to TASK-GUI-0001 as explicit obligations: profile DTC
evidence still uses `NullEvidenceSink` (the legacy GUI records evidence —
wire a real sink before deleting `LiveBackend`); Mode-05 O2 values and
readiness results are executed but not yet retained in the snapshot
(legacy publishes both); GUI recording port per the earlier note.

**TASK-GUI-0001 (`06387e2`): audit-confirmed CLOSED.** `LiveBackend` and
all GUI serial I/O deleted (−3,814 lines); Session/adapter/transport types
exist only in `serial_connector.rs`, pinned by the architecture test (OWL
invariant 9). Tauri commands are snapshot reads + bounded-control acks.
Carried obligations resolved: profile DTC evidence flows through a
collecting sink into the snapshot and the recording worker persists it;
recording runs on a dedicated OS thread fed per published runner snapshot
via a non-blocking bridge (drop-on-saturation); frontend polls with
completion-scheduled 500 ms timeouts, gated off in replay. Audit notes:
(1) the executor's verification omitted Playwright — the suite requires a
running vite dev server and passes 4/4 with one; EV-0001 must script that
prerequisite; (2) a mid-recording write failure still surfaces only at
Stop (error ack), not live — GUI-0002 owns showing recording state
truthfully; (3) O2/readiness were never GUI-displayed (TUI-only), so no
GUI parity loss — runner retention stays a Phase-2 TUI item; (4) the
`LiveBackend`-name scan test is a deletion candidate at EV-0001 close per
the test-selection standard (the import scan already guards the harm).

**TASK-GUI-0002 (`760e995`): audit-confirmed CLOSED.** Completion-scheduled
500 ms polling with a StrictMode-safe zero-delay start (the dev
double-effect cleans up before IPC fires, preserving single in-flight);
duplicate foreground commands guarded through the ack-to-snapshot latency
window via an in-flight ref plus `foregroundPending`, cleared on mode
transitions and rejections; conditional Cancel; Diagnostics as an
always-available tab; replay freezes live polling (exact call-count
assertion) and resumes on exit. Seven Playwright tests pass against the
mocked Tauri IPC boundary — cadence window, delayed-response non-overlap,
one-command dedup, cancel emission, latest-view, replay pause/resume —
all seam-level, spec-named, and mutation-falsifiable. No Rust changes.
Remaining: TASK-EV-0001 only.

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`, `CAP-OBD-RECON`, `CAP-DIAG-DTC`,
  `CAP-DIAG-UI`
- **COMP:** `COMP-OBD-SESS`, `COMP-DASH-PROF`
- **Evidence:** core unit tests, dash runner integration tests, DB migration
  tests, GUI Playwright/Tauri tests, emulator request log, LLY hardware matrix

**Repositories:**

- `/Users/jared/Projects/HaulLogic/obd2-core`
- `/Users/jared/Projects/HaulLogic/obd2-dash`
- Workspace L0 matrix under `/Users/jared/Projects/HaulLogic`

**Tech stack:** Rust 2021, Tokio, async-trait, rusqlite, Tauri 2, React 18,
TypeScript, Playwright.

## Execution status

- `TASK-PROG-0001`: registered in the workspace L0 matrix.
- `TASK-CORE-0001`: committed in `obd2-core` as `0fe6e667`; full workspace
  tests and clippy pass.
- `TASK-DASH-0001`: dependency pin committed in `obd2-dash` as `52c9191`.
  Core `codex/scan-modes` pushed to origin (`0fe6e667`); network-only resolve
  then failed because the committed lock paired the rev with version `0.2.0`
  while the rev declares `0.3.0-dev` — corrected via `cargo update -p
  obd2-core` in `f01adeb`. Workspace check + tests (299 passed) verified
  against the git source; `cargo tree -d` shows one core identity.
- `TASK-DASH-0002`: pure capability, scheduler, and snapshot slice committed
  as `9c911e2`; diagnostics type migration and full runner contracts remain.
- `TASK-DB-0001`: versioned capability schema/models and transactional APIs
  committed as `97e6683`; async store wrapping remains in `TASK-DASH-0003`.
- `TASK-DASH-0003`: async `spawn_blocking` SQLite store boundary committed as
  `68f1d69`; connector/lifecycle/discovery work remains.
- `TASK-DASH-0003` (audit 2026-08-01): executor closed it; audit reopened it
  as PARTIAL — verifier/persistence/harness/reconnect are sound, but
  discovery staging, fallback paths, fingerprint/profile context, the
  telemetry cycle executor, and reconnect policy diverge from spec or are
  absent. Debug-format protocol tokens fixed in the audit (`protocol_token`).
  See the task section for the itemized remainder.
- Out-of-plan feature (`1fb8ee4`): GUI `.obd2rec` recording implemented inside
  the legacy `LiveBackend` (per-snapshot cadence, not per serial poll), plus a
  TUI `record raw` subcommand. The recording module moved into the lib
  (single-compile; binary re-exports it). `TASK-GUI-0001` must port
  start/stop/record into the runner when `LiveBackend` is deleted, and should
  fix the mid-recording write-failure path, which currently drops the writer
  silently while the frontend still shows recording.

## Global constraints

- Work on dedicated branches in both repositories. Do not implement cross-repo
  changes directly on `master`.
- Land `obd2-core` first, then pin the exact resulting SHA in `obd2-dash`.
- Add no new runtime dependency unless a task proves the existing stack cannot
  satisfy the contract. `tokio`, `async-trait`, `serde`, `chrono`, and
  `rusqlite` are already available where needed.
- No `unsafe`.
- No raw supported-PID bitmap parsing in `obd2-dash`.
- No SQLite work on a Tokio worker thread.
- No serial request future may be cancelled by dropping it.
- No change to recording formats or profile definitions.
- Existing TUI behavior remains operational. Phase 2 moves it to the runner.
- Preserve the current GUI JSON capability shape unless the spec explicitly
  adds a field.
- Captures and fixtures must contain no new unscrubbed VIN.

## OWL invariants

The implementation is incomplete unless tests prove all of these:

1. Rescan touches the wire even after `supported_pids()` populated its cache.
2. Only `CapabilityOutcome::Unsupported` is pruned.
3. A failed/cancelled rescan cannot delete the old SQLite set.
4. Reconnect constructs a second adapter/session instance.
5. Duplicate foreground commands do not queue.
6. View changes retain only the latest value.
7. `03/07/0A` cannot occur before an accepted diagnostic command.
8. Cancelling waits for a request boundary.
9. Tauri command code cannot reach live session request APIs.
10. GUI presentation age is measured separately from runner sample age.

## Verified current facts

- `acquire_identity` calls `Session::identify_vehicle`
  (`crates/obd2-dash/src/profiles/selection.rs`).
- `Session::identify_vehicle` currently calls
  `supported_pids().await.unwrap_or_default()`.
- `Session::supported_pids` uses a session cache and the private
  `query_supported_pids`; it does not call `Elm327Adapter::supported_pids`.
- `Elm327Adapter::supported_pids` already has a multi-payload decoder seam but
  blindly iterates four mask pages and hides errors.
- `obd2-db::Database` owns a synchronous `rusqlite::Connection` and has no
  versioned migration mechanism.
- GUI `LiveBackend` owns `Session<Elm327Adapter>` and performs I/O inside
  `diagnostic_snapshot`.
- The frontend polls `diagnostic_snapshot` every 2.5 seconds without overlap
  protection.
- `obd2-dash` already depends on `tokio`, `async-trait`, `serde`, `chrono`, and
  `obd2-db`.
- `obd2-core` is pinned in the root `Cargo.toml` and `Cargo.lock` to
  `94cc6817b3dea85928ad440e8e76fad700ceeea2`.

## Ordered merge graph

```text
TASK-PROG-0001 traceability
    |
TASK-CORE-0001 identity/refresh APIs
    |
TASK-DASH-0001 pin exact core SHA
    |
    +--> TASK-DB-0001 schema/model/transactions
    |
    +--> TASK-DASH-0002 pure capability/scheduler/snapshot contracts
             |
             +--> TASK-DASH-0003 runner lifecycle/discovery/reconnect
                       |
                       +--> TASK-DASH-0004 diagnostics/commands/cancel
                                 |
                                 +--> TASK-GUI-0001 Tauri migration
                                           |
                                           +--> TASK-GUI-0002 frontend UX/polling
                                                     |
                                                     +--> TASK-EV-0001 full validation
```

---

## TASK-PROG-0001: Register scan-mode work package

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`, `CAP-OBD-RECON`, `CAP-DIAG-DTC`,
  `CAP-DIAG-UI`
- **COMP:** `COMP-OBD-SESS`, `COMP-DASH-PROF`
- **Repos:** workspace L0 documents
- **Files:** `HAULLOGIC-MASTER-DESIGN-MATRIX.md`

- [x] Add `WP-OBD-SCAN-MODES` to the work-package mapping with the four CAP
  links and this spec/plan as artifacts.
- [x] Record `obd2-gui` as the R&D proving surface, not the production Desktop
  owner.
- [x] Record the temporary `obd2-core` interface addition under
  `COMP-OBD-SESS`.
- [x] Note the `identify_vehicle` failure-propagation change under
  `COMP-OBD-SESS`: HaulLogic-Desktop inherits it at its next core rev bump and
  should adopt `identify_vehicle_identity` where lenient identity is intended.
- [x] Do not create a new long-lived component unless the matrix owner decides
  the runner is broader than `COMP-DASH-PROF`.

**Done when:** L0 traceability points to this plan without changing product
ownership.

---

## TASK-CORE-0001: Split identity from forced supported-PID discovery

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`
- **COMP:** `COMP-OBD-SESS`
- **Repo:** `obd2-core`
- **Files:**
  - Modify: `crates/obd2-core/src/session/mod.rs`
  - Modify: `crates/obd2-core/src/adapter/elm327.rs`
  - Test: inline test modules in both files

### Interfaces

```rust
pub async fn identify_vehicle_identity(
    &mut self,
) -> Result<VehicleProfile, Obd2Error>;

pub async fn refresh_supported_pids(
    &mut self,
) -> Result<HashSet<Pid>, Obd2Error>;
```

`identify_vehicle()` remains public and composes the two operations. It now
propagates capability-scan failure.

- [ ] **Write failing session tests**
  - `identity_only_does_not_query_supported_pids`
  - `identify_vehicle_propagates_supported_pid_failure`
  - `supported_pids_uses_cache`
  - `refresh_supported_pids_bypasses_cache`
  - `failed_refresh_does_not_replace_cached_supported_pids`

Use a small inline counting adapter. It records calls to `supported_pids` and
can return a scripted sequence of sets/errors. Do not add request-log behavior
to production `MockAdapter` solely for these assertions.

- [ ] **Run red tests**

```bash
cargo test -p obd2-core identity_only -- --nocapture
cargo test -p obd2-core refresh_supported_pids -- --nocapture
```

Expected: APIs are absent and `identify_vehicle` still hides the error.

- [x] **Implement session split**
  - Extract current VIN decode/spec match/profile population into
    `identify_vehicle_identity`.
  - Set the session profile/discovery exactly as today, with an empty supported
    set until a scan succeeds.
  - Make `supported_pids` return the cache or call `refresh_supported_pids`.
  - Make `refresh_supported_pids` call the adapter scan under the session busy
    flag, apply adapter events, and update cache/profile only on success.
  - Delete the private duplicate `query_supported_pids` loop.
  - Replace `unwrap_or_default` in `identify_vehicle` with `?`.

- [ ] **Write failing ELM tests**
  - `supported_pids_stops_when_continuation_bit_is_clear`
  - `supported_pids_follows_claimed_continuation`
  - `supported_pids_propagates_first_page_no_data`
  - `supported_pids_propagates_claimed_continuation_error`
  - retain/strengthen `supported_pids_ors_multiple_ecu_responses`

Use `MockTransport` command expectations. A clear continuation-bit test must
have no `0120` expectation, so an accidental request fails.
The existing multi-ECU test advertises PID `0x20` and then returns `NO DATA` for
`0120`; that fixture encoded the old partial-success behavior. Change it to
either clear the continuation bit while still proving cross-payload union, or
return a valid terminating `0120` page. Cover claimed-page `NO DATA` only in the
new error test.

- [x] **Implement ELM mask walk**
  - Start at base `0x00`.
  - Decode all payloads for the page and OR their PID bits.
  - Request the next page only if the union contains the continuation PID.
  - Propagate command, no-data, and malformed-payload errors for every requested
    page.
  - Return the broadcast union.
  - Do not cache partial results.

- [x] **Run core gates**

```bash
cargo fmt --check
cargo test -p obd2-core supported_pids -- --nocapture
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

- [x] **OWL audit**
  - Search for `unwrap_or_default` around supported-PID discovery.
  - Search for a second bitmap page loop outside the adapter.
  - Confirm `identify_vehicle_identity` does not call supported-PID discovery.
    Do not count an initialization-time `0100` protocol liveness probe as
    identity discovery.

- [x] **Commit in `obd2-core`**

```bash
git add crates/obd2-core/src/session/mod.rs \
  crates/obd2-core/src/adapter/elm327.rs
git commit -m "fix(session): split identity from forced PID discovery"
```

**Done when:** the core SHA is pushed/reviewable and all core gates pass.

---

## TASK-DASH-0001: Pin the corrected core identity

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`
- **COMP:** `COMP-OBD-SESS`
- **Repo:** `obd2-dash`
- **Files:** root `Cargo.toml`, `Cargo.lock`

- [x] Replace the `obd2-core` `rev` with the exact commit from
  `TASK-CORE-0001`.
- [ ] Refresh the lockfile through Cargo; do not hand-edit the checksum/source.
- [ ] Confirm there is one `obd2-core` source identity:

```bash
cargo tree -d
rg -n 'name = "obd2-core"|source = "git\+.*obd2-core' Cargo.lock -A4
```

- [ ] Run:

```bash
cargo test
cargo check --workspace --all-targets
```

- [ ] Commit:

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore(deps): pin core PID refresh API"
```

**Done when:** dash builds only against the reviewed core SHA.

---

## TASK-DB-0001: Add versioned, atomic capability persistence

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`
- **COMP:** `COMP-DASH-PROF`
- **Repo:** `obd2-dash`
- **Files:**
  - Create: `crates/obd2-db/src/migrations.rs`
  - Modify: `crates/obd2-db/src/models.rs`
  - Modify: `crates/obd2-db/src/lib.rs`
  - Test: inline `obd2-db` tests

### Models

Add typed models rather than passing database strings through the runner:

```rust
enum CapabilityKind { Pid, ProfileSignal, Service }
enum CapabilityOutcome { Supported, Unsupported, Unverified }
struct CapabilityContext { protocol, profile_id, probe_schema_version, probe_fingerprint }
struct CapabilityRecord { kind, request_id, module, outcome, observation_seq, rtt_ms, attempted_at, error_code }
struct VehicleCapabilitySet { vin, set_id, context, completed_at, records }
struct CapabilitySetReplacement { vin, context, completed_at, records }
enum CapabilityLoad { Hit(VehicleCapabilitySet), Miss, ContextMismatch }
```

Persist protocol through an explicit stable-token conversion; do not serialize
`Protocol` with `Debug` or a UI label.
`obd2-dash` reuses or re-exports these typed DB models; it must not define a
second `CapabilityOutcome` with a separate conversion path.

- [ ] **Write failing migration/schema tests**
  - `migration_preserves_legacy_vehicle_and_threshold_rows`
  - `capability_module_is_not_nullable`
  - `duplicate_capability_key_conflicts`
  - `invalid_kind_and_outcome_are_rejected`
  - `empty_completed_set_round_trips`
  - `capability_store_does_not_read_or_mutate_legacy_supported_pids_text`

Build the legacy DB in a temp file with the pre-change schema, close it, then
open it through `Database::open`.

- [ ] **Implement migration runner**
  - Keep the existing base schema creation.
  - Read `PRAGMA user_version`.
  - Apply the capability-table migration in a transaction.
  - Set the next version only after all statements succeed.
  - Reject a DB whose version is newer than the binary.
  - Run the same migration path for file and in-memory databases.

- [ ] **Write failing query/transaction tests**
  - `load_returns_context_mismatch_for_stale_fingerprint`
  - `replace_capability_set_is_atomic`
  - `replace_removes_rows_not_present_in_new_set`
  - `manual_outcome_update_is_transactional`
  - `stale_set_id_update_cannot_mutate_replacement_set`
  - `older_observation_sequence_cannot_overwrite_newer_outcome`
  - `replacement_allocates_a_new_nonempty_set_id`

For rollback, pass two records with the same composite key so the second insert
violates the primary key. Assert the old set remains after the error.

- [ ] **Implement synchronous DB APIs**

```rust
pub fn load_capability_set(
    &self,
    vin: &str,
    context: &CapabilityContext,
) -> Result<CapabilityLoad>;

pub fn replace_capability_set(
    &mut self,
    replacement: &CapabilitySetReplacement,
) -> Result<String>;

pub fn update_capability_outcomes(
    &mut self,
    vin: &str,
    set_id: &str,
    records: &[CapabilityRecord],
) -> Result<OutcomeUpdate>;
```

Use explicit `INSERT`, not `INSERT OR REPLACE`, inside the set-replacement
transaction so malformed duplicate input fails and rolls back.
For incremental upsert, condition the conflict update on incoming
`observation_seq >= stored observation_seq`; timestamps do not order writes.
Check `(vin, set_id)` inside the same transaction before the first upsert and
return `StaleSet` with zero writes on mismatch.

- [ ] **Run DB gates**

```bash
cargo fmt --check
cargo test -p obd2-db -- --nocapture
cargo clippy -p obd2-db --all-targets -- -D warnings
```

- [ ] Commit:

```bash
git add crates/obd2-db
git commit -m "feat(db): persist versioned vehicle capability sets"
```

**Done when:** old databases migrate, nullable module rows are impossible, and
rollback preserves the previous set.

---

## TASK-DASH-0002: Define pure capability, scheduler, command, and snapshot contracts

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`, `CAP-DIAG-UI`
- **COMP:** `COMP-DASH-PROF`
- **Repo:** `obd2-dash`
- **Files:**
  - Create: `crates/obd2-dash/src/mode_runner/mod.rs`
  - Create: `crates/obd2-dash/src/mode_runner/capability.rs`
  - Create: `crates/obd2-dash/src/mode_runner/scheduler.rs`
  - Create: `crates/obd2-dash/src/mode_runner/snapshot.rs`
  - Create: `crates/obd2-dash/src/diagnostics.rs`
  - Modify: `crates/obd2-dash/src/lib.rs`
  - Modify: `crates/obd2-dash/src/domain.rs`
  - Modify: `crates/obd2-dash/src/app.rs`
  - Modify: `crates/obd2-dash/src/session_runner.rs`
  - Modify: `crates/obd2-dash/src/widget/renderers.rs`
  - Modify: `crates/obd2-dash/tests/architecture.rs`
  - Create: `crates/obd2-dash/tests/mode_scheduler.rs`

- [ ] **Write failing pure tests**
  - `only_supported_entries_join_normal_tiers`
  - `unverified_entries_join_only_verifier_queue`
  - `unsupported_entries_never_schedule`
  - `profile_forced_pid_cannot_override_unsupported_outcome`
  - `mask_excluded_profile_forced_pid_gets_verifier_not_normal_tier`
  - `verifier_orders_a_then_b_then_visible_c`
  - `verifier_emits_at_most_one_request_per_cycle`
  - `tier_b_is_due_every_fifth_cycle`
  - `view_watch_uses_only_latest_view`
  - `fingerprint_is_stable_for_input_order`
  - `fingerprint_changes_for_required_request_route_or_decoder_change`
  - `fingerprint_ignores_tier_only_presentation_change`
  - `diagnostic_progress_has_five_stable_phases`
  - `discovery_origin_distinguishes_initial_scan_from_rescan`
  - `capability_persistence_and_verification_states_are_independent`

- [ ] **Implement typed state**
  - `CapabilityOutcome` is an enum throughout runner code.
  - Convert to/from DB strings only in the DB model boundary.
  - Keep required request descriptors prebuilt and sorted outside the hot loop.
  - Fingerprint construction is deterministic and occurs only on
    identity/profile/config change.
  - Model module sentinels as constants; never pass `Option<String>` to the DB.

- [ ] **Implement scheduler**
  - Precompute Tier A/B/C request lists.
  - Build each cycle with straightforward loops.
  - Read current `ViewId` once per cycle.
  - Append at most one verifier request.
  - Do not clone the entire capability map per cycle.

- [ ] **Move/define runner snapshot types**
  - Move the neutral `O2Reading`, `FreezeFrameSnapshot`,
    `DiagnosticScanEntry`, `DiagnosticScanScope`, `DtcService`, and
    `DiagnosticScanResult` definitions from binary-only `domain.rs` into the
    library `diagnostics.rs`; update legacy TUI imports without changing logic.
  - Preserve current serialized signal/capability/active-test field names.
  - Add `mode`, `capability_state`, and `foreground_result`.
  - Keep display conversions out of scheduler state.
  - Publish `Arc<RunnerSnapshot>` with `send_replace`; do not full-clone the
    snapshot on every request.
  - Share unchanged heavy collections inside the runner snapshot. Serialization
    belongs to the owned GUI DTO, not the runner type.

- [ ] **Extend architecture scans**
  - Add `src/mode_runner` to the live-dashboard scan set.
  - Assert no mode-runner file calls `.raw_request(` for supported-PID masks.
  - Keep manufacturer-routed calls behind profile runtime.

- [ ] Run:

```bash
cargo fmt --check
cargo test -p obd2-dash --test mode_scheduler -- --nocapture
cargo test -p obd2-dash --test architecture
cargo test -p obd2-dash --test architectural_import
```

- [ ] Commit:

```bash
git add crates/obd2-dash/src/mode_runner \
  crates/obd2-dash/src/diagnostics.rs \
  crates/obd2-dash/src/domain.rs \
  crates/obd2-dash/src/app.rs \
  crates/obd2-dash/src/session_runner.rs \
  crates/obd2-dash/src/widget/renderers.rs \
  crates/obd2-dash/src/lib.rs \
  crates/obd2-dash/tests/architecture.rs \
  crates/obd2-dash/tests/mode_scheduler.rs
git commit -m "feat(runner): define capability-pruned mode scheduler"
```

**Done when:** all scheduling and fingerprint behavior is deterministic without
constructing a session or opening SQLite.

---

## TASK-DASH-0003: Implement lifecycle, discovery, cache, and reconnect

**Status: COMPLETE (closed 2026-08-01; audit-confirmed in `26c8f99`).**
Closure audit: all seven named contracts verified; two were strengthened to
be falsifiable (`fallback_never_schedules_full_legacy_pid_set` had no forced
mask failure and a `< 64` bound the legacy sweep passes;
`reconnect_reacquires_vin_before_cache_load` now proves identity-before-load
via a switched-VIN cache hit). `drive_reconnect` cancellation-by-drop is
acceptable because the abandoned session is discarded wholesale; DASH-0004's
Shutdown must still observe request boundaries. Delivered and verified:
identity-only `acquire_identity` switch; verifier classification (separated
`NO DATA`, transient stays `Unverified`, per-session retry cap + backoff);
staged persistence (set-ID install-before-updates, latest-batch coalescing);
`spawn_blocking` store boundary; scripted connector harness with
request-boundary gating; fresh-session reconnect (connector re-invoked, old
session dropped); stable protocol tokens (`protocol_token`, audit fix).

**Slice 2 (`90c22f8`/`c69d177`, audited + fixed in `bfb8f7d`):** staging as
`Unverified` with per-cycle verifier execution, mask-failure and missing-VIN
conservative fallbacks, watch publication, set-ID/sequence adoption,
reconnect backoff, deterministic fingerprint. Audit fixes: cache-miss
outcomes were silently destroyed (staged replacement dropped; `flush()`
cleared pending without a set) — completed passes now persist atomically
via `replace_from_outcomes`; verification could never reach `Ready` after
any entry exhausted retries — `unresolved()` now drives completion with
`Degraded{unresolved}`; fingerprint included tier cadence via Debug
formatting against §8.1 — now sorted/deduped request identities only, with
the three spec-named fingerprint tests.

**Slice 3 (`6f2a846`, audited + fixed in `b8e5e85`):** profile-derived cache
`profile_id`, scheduler-backed telemetry, typed probe classification, watch
publication. Audit fixes: `poll_cycle` executed only the first planned
request (one gauge ever updated) — it now runs the full plan as an explicit
request loop; failing supported requests demote to `Unverified` per §10
instead of reaching `classify()` where one explicit NRC pruned a live gauge;
unverified work runs only through `Verifier::next()` so NO DATA
confirmations stay backoff-separated (the scheduler plan-tail bypassed
`next_due`); verifier resume now requires the same VIN — context alone let
two same-model trucks cross-contaminate no-data counters into persisted
`Unsupported`; preserved entries keep counters and classified outcomes on
the reconnect re-stage; verifier successes publish immediately (§9.1.5).
Regression tests: full per-cycle polling, demotion,
`reconnect_to_new_vin_discards_partial_verifier_state`,
`same_context_reconnect_resumes_unfinished_initial_verifier`.

**Remaining before this task can close** (spec references in parentheses):

- [x] Seed the verifier, scheduler tiers, and fingerprint from selected
  profile forced/display standard PIDs with deterministic Tier B/C cadences
  (§8.1, §9.1, §10) — slice 4 (`31c519b`), audit-corrected in `f341bb7`:
  tiers now match the §10 table (MAP and fuel-rail actual are Tier A, not
  B/C), and view gating was removed from standard-PID descriptors — the
  scheduler gates every tier by view, so `Some(Gauges)` would have silenced
  all gauges at the first `SetActiveView`. Class-2 profile signals and the
  `ATRV`/adapter row remain unseeded (later telemetry/diagnostics concern).
  Known mild deviation (§9.1 bullet 2): mask-excluded configured PIDs are
  staged `Unverified` and probed instead of starting `Unsupported`; the
  verifier self-corrects them via separated NO DATA within one pass.
- [x] Typed probe classification through the boundary; non-transport
  failures session-local (§8.2, §9.1.6) — slice 3.
- [x] Telemetry cycle executor with watch publication (§10) — slice 3 +
  `b8e5e85` full-plan fix.
- [x] Same-VIN/context verifier resume, different-VIN discard (§13) —
  `b8e5e85`.
- [x] Reconnect driver loop: `drive_reconnect()` retries indefinitely above
  the capped `reconnect()` backoff and remains cancellation-safe at the future
  boundary (§13).
- [x] Plan-named regression contracts are covered:
  `cache_miss_verifies_one_unknown_per_cycle`,
  `successful_verifier_value_is_published_immediately` (behavior
  implemented, unnamed), `fallback_never_schedules_full_legacy_pid_set`,
  `missing_vin_never_calls_store_replace`,
  `fingerprint_mismatch_runs_discovery`, the Tier-C gating pair, and
  `runner_snapshot_preserves_generic_and_lly_signal_shapes`.

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-POLL`, `CAP-OBD-RECON`
- **COMP:** `COMP-OBD-SESS`, `COMP-DASH-PROF`
- **Repo:** `obd2-dash`
- **Files:**
  - Create: `crates/obd2-dash/src/mode_runner/store.rs`
  - Create: `crates/obd2-dash/src/mode_runner/discovery.rs`
  - Modify: `crates/obd2-dash/src/mode_runner/mod.rs`
  - Modify: `crates/obd2-dash/src/profiles/selection.rs`
  - Create: `crates/obd2-dash/tests/mode_runner.rs`

### Interfaces

```rust
#[async_trait]
trait SessionConnector {
    type Adapter: Adapter;
    async fn connect(&self) -> Result<NewSession<Self::Adapter>, ConnectError>;
}

#[async_trait]
trait CapabilityStore {
    async fn load(...);
    async fn replace(...);
    async fn update_outcomes(...);
    async fn load_exact_vehicle_fuel_type(...);
}
```

- [ ] **Write failing async-boundary tests**
  - `sqlite_store_round_trips_through_async_wrapper`
  - `store_failure_does_not_stop_usable_in_memory_telemetry`
  - `store_open_failure_starts_runner_with_session_local_discovery`

Implement the production SQLite store with
`Arc<std::sync::Mutex<Database>>` inside `spawn_blocking`. Never hold the
standard mutex outside the blocking closure.
The fuel lookup delegates to the existing exact-VIN `Database::get_vehicle`;
do not call the VIN-pattern or NHTSA paths.
Opening and migration use the same blocking boundary.
OWL review the implementation to confirm every `Database` access, including
mutex acquisition, is lexically inside a `spawn_blocking` closure.

- [ ] **Write a scripted runner adapter/connector**
  - Record high-level requests in a shared test log.
  - Script identity, supported set, scalar/profile responses, errors, and
    connection failures.
  - Count connector invocations and assign each adapter instance an ID.
  - Keep this test support inside `tests/mode_runner.rs` unless another runner
    integration test needs it.

- [ ] **Write failing runner tests**
  - `cache_hit_starts_telemetry_without_supported_pid_refresh`
  - `cache_miss_refreshes_masks_before_telemetry`
  - `cache_miss_verifies_one_unknown_per_cycle`
  - `unseen_tier_c_does_not_block_initial_persistence`
  - `opening_tier_c_view_enqueues_its_unverified_requests`
  - `successful_verifier_value_is_published_immediately`
  - `runner_snapshot_preserves_generic_and_lly_signal_shapes`
  - `transient_verifier_error_remains_unverified`
  - `first_bare_no_data_remains_unverified_until_separated_confirmation`
  - `unsupported_verifier_result_is_never_repolled`
  - `supported_telemetry_failure_demotes_to_verifier_not_unsupported`
  - `fallback_never_schedules_full_legacy_pid_set`
  - `missing_vin_never_calls_store_replace`
  - `fingerprint_mismatch_runs_discovery`
  - `transport_loss_constructs_new_session`
  - `reconnect_reacquires_vin_before_cache_load`
  - `same_context_reconnect_resumes_unfinished_initial_verifier`
  - `different_context_reconnect_discards_unfinished_verifier`

Use paused Tokio time for poll/backoff assertions. Do not sleep wall-clock time
in tests.

- [ ] **Update identity acquisition**
  - Change dash `acquire_identity` to call the new identity-only core API.
  - Preserve corroborating VIN reads and confidence rules.
  - Do not call cached supported-PID discovery from identity code.

- [ ] **Implement runner loop**
  - Connector creates an uninitialized session; the runner calls
    `Session::initialize`.
  - Identity/profile selection precede cache load.
  - Exact cache hit schedules supported entries immediately.
  - Cache miss calls forced refresh, builds staged outcomes, then enters
    Telemetry with the verifier queue.
  - Snapshot is updated after each completed request/state transition.
  - Execute planned cycles as explicit request loops and service
    cancel/shutdown after every request. Do not wrap the entire cycle in
    `execute_poll_cycle`.
  - Port the current GUI's pure signal/evidence/capability-section snapshot
    construction into `mode_runner::snapshot`; keep the existing GUI builder
    temporarily as the characterization oracle until `TASK-GUI-0001`.
  - Reconnect drops the old session and recreates it.
  - Three short retries transition to capped slow retry, without terminating.

- [ ] **Implement verifier classification**
  - Complete-mask exclusion, `UnsupportedPid`, and explicit unsupported NRC ->
    `Unsupported`.
  - First bare `NO DATA` -> `Unverified`; a second occurrence after verifier
    backoff -> `Unsupported`.
  - stale, timeout, transport, and decode errors -> `Unverified`.
  - A transport error transitions immediately to Reconnecting; do not retry it
    on the broken session.
  - usable decoded value -> `Supported`.
  - Three failed attempts per entry for the session, with capped backoff.
  - Persist stable error codes only.

- [ ] **Implement first-connect persistence**
  - Persist the full completed/degraded set atomically after the verification
    pass.
  - Await the off-thread replacement at the request boundary so the returned
    set ID is installed before later incremental updates. Do not spawn a
    replacement that races live mutations.
  - Surface persistence errors in the snapshot.
  - Use the loaded/generated opaque set ID for incremental outcome writes.
  - Resume the per-set observation counter from the maximum loaded sequence and
    increment it at classification time, before publishing persistence work.
  - Submit rare telemetry demotions as full latest-value outcome batches over a
    persistence watch channel. The worker coalesces superseded batches; do not
    create an unbounded write queue.
  - An old set ID must turn a delayed update into `StaleSet`.
  - Flush the latest accepted background batch during shutdown after dropping
    the serial session.

- [ ] Run:

```bash
cargo fmt --check
cargo test -p obd2-dash --test mode_runner -- --nocapture
cargo test -p obd2-dash
cargo clippy -p obd2-dash --all-targets --all-features -- -D warnings
```

- [ ] Commit:

```bash
git add crates/obd2-dash/src/mode_runner \
  crates/obd2-dash/src/profiles/selection.rs \
  crates/obd2-dash/tests/mode_runner.rs
git commit -m "feat(runner): add discovery cache and lifecycle reconnect"
```

**Done when:** scripted transport loss creates a new instance and cache-hit
startup performs no post-initialization capability mask walk.

---

## TASK-DASH-0004: Add bounded foreground commands and diagnostic bundle

**Slice 1 (`3d3802f`, audited + fixed in `d95a07e`):** transport-independent
bounded command contract and mode-table enforcement (`RunDiagnostic`,
`RescanVehicle`, `CancelForeground`, `Shutdown`); mode table verified
faithful to the spec. Rejected commands are not retained, accepted commands
publish their transition, cancellation returns to telemetry. Audit fixes:
`poll_cycle` now refuses to run outside Telemetry (it previously kept
polling during Diagnostic and would reconnect after Shutdown via the
session-gone transport error); the vacuous pause/resume test was renamed to
what it asserts (`cancel_foreground_returns_to_telemetry`) — the plan-named
verifier pause/resume contract remains open. Still outstanding beyond the
diagnostic bundle and staged rescan executor: verifier pause/resume
machinery, Shutdown persistence flush + acknowledgement ordering,
`RequestActiveTest` routing, and the async command channel with oneshot
acknowledgements (the current surface is the synchronous state machine the
channel will wrap in TASK-GUI-0001).

**Slice 2 (`988f2fa`, audited + fixed in `28196af`):** diagnostic phase
contract and Mode-05 eligibility gate. Five stable ordered phases; Mode-05
requires explicit gasoline, a positively identified legacy protocol, no
cached unsupported outcome, and no LLY profile. Audit fix: the protocol
check was a CAN deny-list — `Protocol::Auto` (unresolved) and any future
non-exhaustive core variant passed it; it is now an allow-list
(J1850 VPW/PWM, ISO 9141, KWP2000) so unknown protocols deny by default.
Wire execution and fuel resolution from Session/DB remain next; when the
fuel resolver lands it must normalize only exact recognized labels
(spec §11) — no substring or heuristic matching, unknown stays Unknown.

**Slice 3 (`179fa49`, audited + fixed in `f136d0d`):** strict fuel
resolution with Session-spec precedence and exact-VIN database fallback.
Audit fix: precedence is by source, not value — a present-but-unparseable
session claim resolved via the DB, letting a cached NHTSA row overrule the
curated spec (a hypothetical spec "bio-diesel b20" + DB "Gasoline" enabled
Mode-05 on a diesel). Labels now classify recognized / explicit-no-claim
("unknown"/blank, which must keep falling through — the generic embedded
spec ships `fuel_type: unknown` and gasoline vehicles have no embedded
spec) / unrecognized, which resolves Unknown without consulting the DB.
Vocabulary verified against shipped specs (lowercase `diesel`, `unknown`).

**Slice 4 (`dde3883`, audit hardened in `a133786`):** `service_allowed`
accepts `03/07/0A` only in Diagnostic mode and permanently denies Mode-06.
Audit note: the gate is a pure predicate — "denied by construction" holds
only if every composer routes through it, so an architecture scan now fails
the suite if any mode_runner file outside `diagnostic.rs` composes DTC
service bytes. The gate covers DTC services only; phase execution must
apply its own mode gating to Mode-02 freeze frames and Mode-05 (which
`service_allowed` would deny even in Diagnostic mode if misrouted).

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-DIAG-DTC`, `CAP-OBD-POLL`
- **COMP:** `COMP-DASH-PROF`
- **Repo:** `obd2-dash`
- **Files:**
  - Create: `crates/obd2-dash/src/mode_runner/diagnostic.rs`
  - Modify: `crates/obd2-dash/src/mode_runner/mod.rs`
  - Modify: `crates/obd2-dash/src/mode_runner/snapshot.rs`
  - Modify: `crates/obd2-dash/tests/mode_runner.rs`

- [ ] **Write failing command tests**
  - `run_diagnostic_is_rejected_until_telemetry`
  - `duplicate_foreground_command_returns_busy_without_queueing`
  - `foreground_command_pauses_and_resumes_background_verifier`
  - `diagnostic_before_initial_persistence_merges_service_outcomes`
  - `cancel_is_observed_after_current_request`
  - `shutdown_is_observed_after_current_request`
  - `shutdown_ack_waits_for_session_drop_and_persistence_flush`
  - `closed_command_channel_shuts_down_instead_of_reconnecting`
  - `rescan_uses_forced_refresh_after_cached_scan`
  - `rescan_progress_total_is_stable_and_counts_skipped_units`
  - `cancelled_rescan_keeps_old_memory_and_db_sets`
  - `failed_rescan_keeps_old_memory_and_db_sets`

The test adapter must expose a controllable in-flight request. Assert the future
completes before cancellation changes mode.

- [ ] **Implement bounded control**
  - Create the discrete channel with capacity 8.
  - Put `ViewId` on a separate watch channel.
  - Carry oneshot acknowledgement in interactive command variants.
  - Accept/reject according to the design's mode table.
  - Never retain a rejected command for later execution.

- [ ] **Write failing diagnostic tests**
  - `telemetry_never_sends_dtc_services`
  - `diagnostic_runs_five_phases_in_order`
  - `freeze_frame_total_is_set_after_dtc_phase`
  - `nontransport_step_error_continues_bundle`
  - `transport_error_interrupts_bundle_and_reconnects`
  - `diesel_never_sends_mode05`
  - `unknown_fuel_never_sends_mode05`
  - `embedded_spec_fuel_class_wins_over_database_fallback`
  - `fuel_class_never_uses_profile_or_display_name_heuristics`
  - `gas_noncan_mode05_respects_cached_unsupported`
  - `manual_service_results_update_capability_store`
  - `manual_service_no_data_requires_separated_confirmation`
  - `mode06_is_never_sent`

- [ ] **Port the diagnostic behavior**
  - Use existing public `Session` and selected-profile runtime APIs.
  - Preserve stored/pending/permanent order, module order, GM Class-2 backoff,
    freeze-frame correlation, readiness, and module refresh behavior.
  - Keep all `03/07/0A` calls inside `diagnostic.rs`.
  - Resolve fuel class from embedded Session spec, then exact-VIN DB data,
    otherwise Unknown. Only the typed `Gasoline` variant permits Mode-05.
  - Do not introduce raw OEM routing outside profile runtime.
  - Update service capability outcomes after manual requests.

- [ ] **Implement rescan staging**
  - Pause telemetry but retain snapshot values.
  - Build a separate in-memory staged set.
  - Verify telemetry capabilities only.
  - Replace DB and active memory set only on successful completion.
  - Discard staged state on cancel/error.

- [ ] **Preserve active-test locked behavior**
  - Route `RequestActiveTest` through the bounded command surface.
  - Run evidence file I/O through `spawn_blocking`.
  - Add no newly actionable active-test path.

- [ ] Run:

```bash
cargo fmt --check
cargo test -p obd2-dash --test mode_runner -- --nocapture
cargo test -p obd2-dash
cargo clippy -p obd2-dash --all-targets --all-features -- -D warnings
```

- [ ] Commit:

```bash
git add crates/obd2-dash/src/mode_runner \
  crates/obd2-dash/tests/mode_runner.rs
git commit -m "feat(runner): add manual diagnostics and staged rescan"
```

**Done when:** transport logs contain no diagnostic request before the command,
and cancel/error cannot replace the old capability set.

---

## TASK-GUI-0001: Replace Tauri live backend with thin runner wiring

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-OBD-RECON`, `CAP-DIAG-UI`
- **COMP:** `COMP-DASH-PROF`
- **Repo:** `obd2-dash`
- **Files:**
  - Create: `apps/obd2-gui/src-tauri/src/serial_connector.rs`
  - Create: `apps/obd2-gui/src-tauri/src/runner_state.rs`
  - Create: `apps/obd2-gui/src-tauri/src/commands.rs`
  - Create: `apps/obd2-gui/src-tauri/src/snapshot_dto.rs`
  - Modify: `apps/obd2-gui/src-tauri/src/main.rs`
  - Create: `apps/obd2-gui/src-tauri/tests/architecture.rs`
  - Modify/add: Tauri unit tests

- [ ] **Freeze current JSON compatibility**
  - Move the existing generic-capability serialization assertion into a reusable
    snapshot DTO test.
  - Add assertions for new `mode`, `capability_state`, and
    `foreground_result`.
  - Keep old recording/replay input and active-test output shapes unchanged.
  - Add a test that cloning the watch value clones an `Arc`; full owned
    conversion occurs only in `diagnostic_snapshot`.

- [ ] **Write failing architecture test**
  - Permit `Session`, `Elm327Adapter`, and serial transport construction only in
    `serial_connector.rs`.
  - Forbid session request methods, raw requests, and adapter access in
    `commands.rs`, `runner_state.rs`, and `main.rs`.
  - Forbid `LiveBackend`, `try_snapshot`, and connect-inside-snapshot symbols.

- [ ] **Implement serial connector**
  - Move port selection, baud configuration, `LoggingTransport`, adapter
    construction, and session construction into `serial_connector.rs`.
  - Run synchronous port enumeration/open in `spawn_blocking`.
  - A connector call creates a fresh transport every time.
  - Keep the existing 500 ms post-open settle only if hardware evidence still
    requires it; do not sleep while holding app state locks.

- [ ] **Implement app runner state**
  - Resolve the capability DB under Tauri app data.
  - Hold the initial watch receiver, bounded command sender, and view sender.
  - Spawn one background bootstrap from Tauri setup; open/migrate the DB through
    the async blocking wrapper inside that bootstrap, then start the runner.
  - Do not open SQLite on the Tauri setup thread.
  - On open/migration failure, start the runner with a disabled store and expose
    the storage error in snapshots; do not strand the runner bootstrap.
  - Add an app-exit path that sends `Shutdown`.
  - Hold exit completion until the runner acknowledges session release and
    persistence flush; channel drop alone is only the crash/fallback path.

- [ ] **Implement thin commands**
  - `diagnostic_snapshot`: borrow/clone current snapshot only.
  - `run_diagnostic`, `rescan_vehicle`, `cancel_foreground`: send bounded command
    and await acknowledgement.
  - `set_active_view`: replace the watch value.
  - `request_active_test`: forward to runner.
  - Map busy/not-ready/closed errors to stable frontend strings.

- [ ] **Delete old live I/O**
  - Delete `LiveBackend::try_snapshot`, its `Session` field, inline connect,
    mutex-held serial sweep, GUI profile polling, GUI DTC polling, and direct
    PID/profile request helpers.
  - Preserve recording file inspection commands; they are unrelated local file
    operations.
  - Port the `1fb8ee4` GUI recording feature (`start_recording`,
    `stop_recording`, per-sample writes) into the runner: record from runner
    samples (per completed request, not per snapshot invoke), run file I/O off
    the async path, and surface mid-recording write failures in the snapshot
    instead of silently dropping the writer while the UI still shows
    recording.

- [ ] **Run Tauri gates**

```bash
cargo test -p obd2-gui -- --nocapture
cargo test -p obd2-gui --test architecture
cargo clippy -p obd2-gui --all-targets -- -D warnings
```

- [ ] Commit:

```bash
git add apps/obd2-gui/src-tauri
git commit -m "refactor(gui): read snapshots from background mode runner"
```

**Done when:** `diagnostic_snapshot` is a synchronous snapshot read/DTO
conversion, command senders await only their acknowledgements, and the
architecture test prevents serial I/O from returning to commands.

---

## TASK-GUI-0002: Add 500 ms snapshot polling and foreground scan UX

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** `CAP-DIAG-UI`
- **COMP:** `COMP-DASH-PROF`
- **Repo:** `obd2-dash`
- **Files:**
  - Modify: `apps/obd2-gui/src/types.ts`
  - Modify: `apps/obd2-gui/src/App.tsx`
  - Modify: `apps/obd2-gui/src/mockData.ts`
  - Modify: `apps/obd2-gui/tests/dashboard.spec.ts`

- [ ] **Extend frontend types**
  - Add discriminated unions for runner mode, capability persistence,
    capability verification, and foreground result.
  - Preserve the `initial` versus `rescan` discovery origin so the UI never
    offers Cancel for required first-connect discovery.
  - Extend every fixture with explicit idle/cached state.
  - Keep raw snapshot rendering able to display the new fields.

- [ ] **Write failing Playwright IPC tests**
  - Inject a fake `window.__TAURI_INTERNALS__.invoke`.
  - Count `diagnostic_snapshot` calls for 1.6 s and assert 500 ms cadence within
    timer tolerance.
  - Delay one fake snapshot response beyond 500 ms and assert maximum concurrent
    invokes remains 1.
  - Enter replay mode and assert live snapshot polling stops; exit replay and
    assert it resumes.
  - Click successive tabs and assert `set_active_view` receives the latest tab.
  - Assert duplicate diagnostic clicks result in one command.
  - Assert Cancel emits `cancel_foreground`.

- [ ] **Implement completion-scheduled refresh**
  - Replace `setInterval(2500)` with a 500 ms completion-scheduled timeout, or
    retain a ref-based in-flight guard.
  - Start the loop only in Tauri live mode; static browser fixtures do not need
    a 2 Hz state-update timer.
  - Do not update `lastRefresh` when invoke fails.
  - Cancel the pending timer and ignore late completion on component unmount.

- [ ] **Implement foreground controls**
  - Add `Run Diagnostic` and `Rescan vehicle` commands in their operational
    locations.
  - Disable both after acknowledgement while a foreground mode is active.
  - Show Cancel only for `Discovering` foreground rescan or `Diagnostic`.
  - Render five-phase diagnostic progress and request progress.
  - Grey existing gauge values without replacing them with zero/empty.
  - Report persistence and verification independently in the connection area,
    including no-VIN and store-error session-only states.
  - Send active view whenever the selected tab changes.

- [ ] **Run frontend gates**

```bash
cd apps/obd2-gui
npm run build
npx playwright test
```

- [ ] Commit:

```bash
git add apps/obd2-gui/src apps/obd2-gui/tests/dashboard.spec.ts
git commit -m "feat(gui): add scan controls and fresh snapshot polling"
```

**Done when:** visible cached-state latency is bounded to about 500 ms and a
delayed invoke cannot overlap the next one.

---

## TASK-EV-0001: Full integration, emulator, and hardware evidence

- **WP:** `WP-OBD-SCAN-MODES`
- **CAP:** all four
- **COMP:** both
- **Repos:** `obd2-core`, `obd2-dash`, hardware matrix owner

- [ ] **Run full core gates**

```bash
cd /Users/jared/Projects/HaulLogic/obd2-core
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

- [ ] **Run full dash/GUI gates**

```bash
cd /Users/jared/Projects/HaulLogic/obd2-dash
cargo fmt --check
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings
cd apps/obd2-gui
npm run build
npx playwright test
```

- [ ] **Run emulator request-mix assertions**
  - Cache miss has mask requests followed by bounded verification.
  - Cache hit has no post-initialization capability mask walk. A protocol
    negotiation `0100` is allowed and must be labeled separately in the log.
  - Tier A dominates steady-state request counts.
  - Unsupported request count stops after classification.
  - No `03/07/0A` appears before the command marker.
  - No Mode-05 appears for diesel/unknown fuel.
  - Rescan emits a forced new mask request.

- [ ] **Run LLY/STN/J1850 validation**
  - Record core SHA and dash SHA.
  - Measure cache-miss and cache-hit connect-to-first-RPM.
  - Measure Tier-A cycle p50/p95/max.
  - Measure runner sample age and GUI presentation age separately.
  - Physically interrupt/reconnect adapter and verify a new lifecycle.
  - Cancel rescan and prove old cached telemetry resumes.
  - Run/cancel diagnostic and verify ordered phases.

- [ ] **Update hardware matrix**
  - Add adapter, vehicle class, protocol, build SHAs, date, measurements, and
    pass/fail notes.
  - Do not claim a 1 Hz presentation rate unless measured GUI age supports it.

- [ ] **Final OWL source audit**

```bash
rg -n 'unwrap_or_default|supported_pids|refresh_supported_pids' \
  crates apps
rg -n 'raw_request|adapter_mut|Session<Elm327Adapter>|try_snapshot|LiveBackend' \
  crates/obd2-dash/src/mode_runner apps/obd2-gui/src-tauri/src
rg -n '03|07|0A|Mode-05|Mode-06' crates/obd2-dash/src/mode_runner
```

Review every hit; do not treat a source scan as proof by itself.

- [ ] **Commit evidence/docs**

```bash
git add docs
git commit -m "docs(validation): record scan-mode hardware evidence"
```

**Done when:** all automated gates pass, the emulator proves request ordering,
and the hardware matrix records runner and presentation latency independently.

## Completion criteria

- The GUI never owns a live session.
- Cache-hit connection performs no post-initialization supported-PID mask scan.
- Rescan always performs a new mask scan.
- Unsupported requests disappear from steady-state transport logs.
- Unverified/transient failures remain retryable and do not enter normal tiers.
- Diagnostic services are manual-only.
- Reconnect recreates the transport/session.
- Failed/cancelled rescan retains the old cache.
- SQLite migration is versioned and atomic.
- GUI snapshot invokes do not overlap and run at 500 ms cadence.
- Both repository SHAs and the LLY hardware evidence are recorded.
