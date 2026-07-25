# Scan Modes Design — Discover / Telemetry / Diagnostic

- **Date:** 2026-07-24
- **Status:** Approved (design review in session; this document is the written spec)
- **Scope:** GUI-first (`apps/obd2-gui`), shared mode runner in the `obd2-dash` lib
- **Follow-on:** Phase 2 migrates the TUI onto the same runner (out of scope here)

## 1. Problem

The UI lags the live OBD-II stream by seconds. Measured from live-truck sessions
(LLY Duramax, J1850 VPW, STN22xx serial @ 115200):

- Average poll cycle **2.23 s** against a configured 250 ms interval; 82 of 437
  cycles exceeded 4 s (`logs/obd2-dash.log.2026-06-26`).
- **~47% of wire time** spent on requests answering `NO DATA` — unsupported
  PIDs re-polled every cycle, plus a Mode-05 O2 sweep (~64 requests ≈ 8 s)
  every 20th cycle on a diesel with no O2 sensors.
- Saturation side effects: response desynchronization (stale `41 62 87`
  answering `0146`) and parse errors dropping valid samples.

Root causes in code:

1. `pollable_pids()` (`crates/obd2-dash/src/main.rs:123`) polls all ~43 scalar
   Mode-01 PIDs, not the displayed set.
2. Supported-PID pruning fails open: `prepare_session`
   (`crates/obd2-dash/src/session_runner.rs:189-194`) treats empty/error as
   "poll everything"; `Elm327Adapter::supported_pids` returns `Ok(empty)` when
   the first `0100` probe errors.
3. Slow diagnostics (enhanced DIDs, DTC scans, Mode-05 O2, readiness) run
   inline on the same serialized loop as the gauges, on 5/10/20-cycle cadences.
4. The GUI has no background poller at all: the `diagnostic_snapshot` Tauri
   command performs the entire serial sweep inline under a mutex
   (`apps/obd2-gui/src-tauri/src/main.rs:324-482`), and the frontend invokes it
   on an unguarded 2.5 s `setInterval` (`apps/obd2-gui/src/App.tsx:1699-1705`),
   so invokes queue and displayed data grows staler without bound.

Physics constraint: J1850 VPW costs ~135–200 ms per request (~5–7 requests/s).
A uniform 250 ms full-refresh is impossible regardless of pruning; bandwidth
must be prioritized.

## 2. Approved decisions

| Decision | Choice |
| --- | --- |
| Discovery cadence | Once per VIN, persisted in `obd2-db`; mask-first, verify in background; manual "Rescan vehicle" |
| Telemetry during diagnostic scan | Pause with determinate progress; gauges greyed with last-known values |
| Passive DTC awareness | None — strictly manual (`Run Diagnostic` button); no PID 0x01 sentinel |
| First surface | GUI (`obd2-gui`), including its background-runner refactor |
| Architecture | **A** — shared mode runner in the `obd2-dash` lib consumed by both surfaces; consolidates existing GUI/TUI duplication |

## 3. Architecture

One background runner owns the `Session` and is the only code that touches the
serial pipe. UIs read cached snapshots and send commands.

```
Connecting ──► Discovering (capability-cache miss only) ──► Telemetry ◄──► Diagnostic
     ▲                    ▲                                    │  (RunDiagnostic)
     │                    └── RescanVehicle ───────────────────┤
     └──────────── Reconnecting (serial loss, backoff) ◄───────┘
```

- **Connecting:** open transport, `Session::initialize`, acquire VIN/identity
  (existing `acquire_identity` path), select profile (existing).
- **Discovering:** only on capability-cache miss (or explicit rescan). Never
  blocks gauges beyond the mask reads.
- **Telemetry:** steady state. Tiered, capability-pruned polling (§6).
- **Diagnostic:** entered only by user command. Telemetry paused, progress
  reported (§7).
- **Reconnecting:** on serial error; capability cache survives, so resume goes
  straight to Telemetry.

No state ever schedules a request the capability map marks unsupported, and no
state runs the legacy poll-everything sweep.

### Ownership (per program rules)

| Repo / crate | Owns in this design |
| --- | --- |
| `obd2-core` | Mechanics only: `supported_pids` fail-open fix (§5.1); existing Session/request APIs unchanged |
| `crates/obd2-dash` (lib) | Mode runner, tier scheduler, capability model, discovery/verification logic |
| `crates/obd2-db` | `vehicle_capabilities` table, queries, migration |
| `apps/obd2-gui` | Background task wiring, watch/mpsc plumbing, buttons, progress/greyed-gauge UI, tab reporting |

## 4. Components

### 4.1 Mode runner (`crates/obd2-dash/src/mode_runner/` — new module in the lib)

- Generic over `A: Adapter` like `session_runner`.
- Inputs: `mpsc::UnboundedReceiver<RunnerCommand>`, capability store handle,
  display/tier configuration.
- Output: `tokio::sync::watch::Sender<RunnerSnapshot>` — a serializable
  snapshot updated after every completed request batch (not once per cycle, so
  Tier-A values land as soon as they are read).
- Absorbs the logic currently duplicated between `session_runner.rs` and the
  GUI's `try_snapshot` (profile selection refresh, identity sync, context
  fingerprint, profile DTC scanning).

```rust
enum RunnerCommand {
    RunDiagnostic,
    RescanVehicle,
    SetActiveView(ViewId),   // drives Tier C membership (§6)
    Shutdown,
}
```

`RunnerSnapshot` extends today's GUI `DiagnosticSnapshot` content with:

```rust
mode: ModeState,             // Connecting | Discovering{step,total} | Telemetry
                             // | Diagnostic{step,total,label} | Reconnecting{attempt}
capability_source: CapSource // Cached | FreshScan | ConservativeFallback
```

### 4.2 Capability store (`crates/obd2-db`)

```sql
CREATE TABLE vehicle_capabilities (
    vin              TEXT NOT NULL,
    kind             TEXT NOT NULL,   -- 'pid' | 'profile_signal' | 'service'
    request_id       TEXT NOT NULL,   -- e.g. '01:0C', 'lly.1543', 'svc:05'
    module           TEXT,            -- responding module when known (nullable)
    supported        INTEGER NOT NULL,-- 0 | 1
    avg_rtt_ms       INTEGER,         -- measured during verification
    last_verified_at TEXT NOT NULL,   -- ISO-8601
    PRIMARY KEY (vin, kind, request_id, module)
);
```

- `kind='service'` records whether Mode-05/Mode-06/readiness answered at all,
  so Diagnostic mode can skip dead services too.
- Store API: `load_capabilities(vin) -> Option<CapabilityMap>`,
  `replace_capabilities(vin, map)`. A rescan replaces the VIN's rows wholesale.
- `CapabilityMap` feeds the existing
  `CoverageMap::with_supported_standard_pids` seam; no scheduler rewrite of
  that structure.

### 4.3 GUI integration (`apps/obd2-gui`)

- On app start (or `Connect`), spawn the runner as a background Tauri task.
- `diagnostic_snapshot` command: `watch::Receiver::borrow().clone()` — returns
  instantly; the existing 2.5 s frontend interval is retained (pileup is
  impossible against an instant read). Event-push is a non-goal (§10).
- New commands: `run_diagnostic`, `rescan_vehicle`, `set_active_view`.
- Frontend: `Run Diagnostic` and `Rescan vehicle` buttons; progress bar +
  greyed gauges when `mode` is `Diagnostic`/`Discovering`; a `capability_source`
  badge ("cached" / "scanning" / "fallback") in the connection area.
- Deleted: all inline serial I/O in `LiveBackend::try_snapshot`, the
  connect-inside-snapshot path, and the mutex-held-during-I/O pattern.
  `LiveBackend` shrinks to command forwarding + snapshot relay (or is removed).

## 5. Discovery mode

### 5.1 Prerequisite fix (obd2-core)

`Elm327Adapter::supported_pids` currently `break`s on the first probe error and
returns `Ok(empty)`. Change: an error on the **first** probe (`0100`) returns
`Err`; errors on continuation probes (`0120`/`0140`/`0160`) end the walk with
the masks collected so far (standard OBD behavior — absent continuation bit
means done). Callers must distinguish "vehicle reports nothing" from "probe
failed".

### 5.2 Flow (capability-cache miss)

1. Read the four mask PIDs (~4 requests, <1 s). Record per responding module
   (the LLY answers `0100` from two modules; keep both rows).
2. **Start Telemetry immediately** on `mask-claimed ∩ display set`.
3. In the background — interleaved one verification request per telemetry
   cycle — verify: probe each mask-claimed Mode-01 PID once, each profile
   Class-2 display signal once, and each diagnostic service once. Record
   supported / `NO DATA` / error and measured RTT. During this phase the mode
   remains `Telemetry`; `capability_source = FreshScan` is what tells the UI
   verification is still running.
4. Persist the completed map; snapshot `capability_source` flips
   `FreshScan → Cached`.

Mask probe fails entirely (§5.1 `Err`) → `ConservativeFallback`: poll the
display set only, persist nothing, surface the fallback badge.

No VIN (unread/corrupted) → run steps 1–3 for the session but persist nothing.

### 5.3 Cache hit

Load map, enter Telemetry directly. Total connect-to-gauges overhead: identity
reads only.

"Rescan vehicle" (button) repeats the *steps* of §5.2 but deliberately inverts
the execution style: it is user-initiated, so it runs as a **foreground**
Discovering pass with determinate progress and paused gauges (same UX as
Diagnostic mode), finishing faster than the interleaved first-connect verify.
First connect prioritizes time-to-gauges; explicit rescan prioritizes scan
completion.

## 6. Telemetry mode

Tier membership is computed as `display config ∩ capability map`; the lists
below are the LLY-profile defaults. Unsupported entries drop out per vehicle
automatically.

| Tier | Cadence | Default members |
| --- | --- | --- |
| A | every cycle | RPM `010C`, MAP/boost `010B`, speed `010D`, fuel rail actual `0123`, VGT actual `lly.1543`, voltage `ATRV` |
| B | every 5th cycle | coolant `0105`, IAT `010F`, MAF `0110`, load `0104`, baro `0133`, desired MAP `lly.1542`, rail desired `lly.163D`, VGT desired `lly.1540` |
| C | only while the owning view is visible (`SetActiveView`), every cycle while visible | cylinder balance table (8 × Class-2 reads), secondary/evidence profile signals |

- Expected Tier-A cycle on J1850: ~6 requests ≈ 1.0–1.3 s → headline gauges
  ~1 Hz (vs 0.3 Hz today). Tier-B cycles ≈ 2–2.5 s. These are expectations,
  not contracts; the scheduler is cadence-driven, not deadline-driven.
- The poll interval remains configurable; the interval is a *floor* between
  cycle starts, and a cycle that overruns simply starts the next one
  immediately (current behavior, now with far shorter cycles).
- No DTC sentinel: per the approved decision, nothing DTC-related runs in
  Telemetry. The `03/07/0A` cadences, Mode-05 O2, and readiness reads are
  removed from the steady-state loop entirely.

## 7. Diagnostic mode

Entered only via `RunDiagnostic`. Telemetry pauses; the UI greys gauges and
shows determinate progress (`step/total` — total is computable up front from
the capability map + discovered module list).

Bundle, in order:

1. DTC Stored/Pending/Permanent — broadcast, then per discovered module
   (existing `scan_standard_dtcs` + profile DTC path, including the
   GM Class-2 backoff cache).
2. Freeze frames for each code found (existing correlated-PID set).
3. Readiness (skipped if `kind='service'` row says unsupported).
4. Mode-05 O2 — **only if** fuel type is gasoline **and** protocol is non-CAN,
   and the capability row confirms it answers. (LLY: never runs.)
5. Module scan refresh (existing GUI `ModuleScan` data).

Per-step failures record their `DiagnosticScanResult` and the bundle
continues; the scan never aborts wholesale on one dead module. On completion,
results land in the snapshot and Telemetry resumes automatically.

## 8. Error handling

- **Serial error in any mode:** snapshot flips to `Reconnecting{attempt}`,
  runner retries with backoff (reusing the existing 3-attempt connect
  pattern, then continuing at a slow cadence rather than giving up).
  Capability cache is VIN-keyed and survives; resume path is §5.3.
- **Stale/desynchronized responses:** the existing
  `is_stale_pid_response_error` suppression carries over unchanged.
  Saturation-induced desync is expected to mostly disappear.
- **Capability staleness:** rows carry `last_verified_at`; nothing expires
  automatically in this phase — "Rescan vehicle" is the invalidation path.
- **Command channel closed / UI gone:** runner shuts down cleanly, releasing
  the serial port.

## 9. Testing & verification

Mock-first, per `obd-session-integration`:

1. **Scheduler unit tests** (`crates/obd2-dash`): tier rotation math,
   mode transitions (incl. Diagnostic pause/resume, Rescan re-entry),
   capability pruning (unsupported members never scheduled), tab-visibility
   gating for Tier C.
2. **Runner integration tests** against `MockAdapter`: cache-miss flow
   (masks → immediate telemetry → background verify → persisted map),
   cache-hit flow, `ConservativeFallback` on first-probe error (regression
   test for the §5.1 fix), no Mode-05 requests when fuel type is diesel.
3. **`obd2-db` tests:** migration, replace-on-rescan semantics, composite-key
   handling for multi-module rows.
4. **Emulator end-to-end** (`--emu` harness): assert the *request mix* from
   the transport log — no unsupported re-polls in steady state, Tier-A
   cadence dominance, diagnostic bundle only after the command.
5. **Truck validation:** headline-gauge refresh rate and cycle-time
   measurement repeated against `logs/`; update the hardware matrix (program
   rule: user-visible connect/telemetry behavior changed).

Commands: `cargo test` (workspace), `cd apps/obd2-gui/src-tauri && cargo test`,
plus existing emulator scripts.

## 10. Non-goals (this phase)

- TUI migration to the shared runner (Phase 2; `session_runner.rs` keeps
  working untouched until then).
- Event-push snapshot delivery to the frontend (interval polling of the
  instant read is sufficient).
- Automatic capability expiry / re-verification policy.
- Graduating capability maps or the mode scheduler into `obd2-core`
  (revisit after Phase 2 proves the shape across both surfaces).
- Any change to HOS/ELD paths, raw-capture formats, or profile definitions.
