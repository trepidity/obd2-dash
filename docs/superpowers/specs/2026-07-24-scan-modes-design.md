# Scan Modes Design - Discover / Telemetry / Diagnostic

- **Date:** 2026-07-24
- **Status:** Revised after OWL review; implementation-ready
- **Scope:** GUI-first (`apps/obd2-gui`), shared mode runner in the
  `obd2-dash` library
- **Follow-on:** Phase 2 migrates the TUI onto the same runner
- **CAP:** `CAP-OBD-POLL`, `CAP-OBD-RECON`, `CAP-DIAG-DTC`,
  `CAP-DIAG-UI`
- **COMP:** `COMP-OBD-SESS`, `COMP-DASH-PROF`
- **WP:** `WP-OBD-SCAN-MODES`

## 1. Operator outcome

The diagnostics GUI shows the freshest completed gauge values without queuing
serial sweeps behind UI refresh calls. Unsupported requests are learned once,
persisted per vehicle context, and never left in a hot polling tier. Slow
diagnostic work runs only when the operator requests it.

This remains an R&D/proving surface in `obd2-dash`; it does not move product
shell or compliance ownership out of HaulLogic Desktop.

## 2. Measured problem

The UI lags the live OBD-II stream by seconds. Measured from live-truck sessions
(LLY Duramax, J1850 VPW, STN22xx serial at 115200):

- Average poll cycle **2.23 s** against a configured 250 ms interval; 82 of 437
  cycles exceeded 4 s (`logs/obd2-dash.log.2026-06-26`).
- **~47% of wire time** was spent on requests answering `NO DATA`: unsupported
  PIDs were re-polled every cycle, and a Mode-05 O2 sweep (about 64 requests,
  about 8 s) ran every 20th cycle on a diesel with no O2 sensors.
- Saturation caused response desynchronization (for example stale `41 62 87`
  answering `0146`) and parse errors that discarded valid samples.

Root causes in the current code:

1. `pollable_pids()` (`crates/obd2-dash/src/main.rs:123`) returns all scalar
   Mode-01 PIDs, not only the configured display set.
2. `prepare_session`
   (`crates/obd2-dash/src/session_runner.rs:189-194`) treats an empty or failed
   supported-PID query as "poll everything."
3. The actual supported-PID call path is not the adapter method alone:
   `acquire_identity` calls `Session::identify_vehicle`, which calls and caches
   `Session::supported_pids`; `identify_vehicle` then hides failure with
   `unwrap_or_default`. A later discovery pass can therefore observe a cached
   empty set without touching the wire.
4. Slow enhanced reads, DTC scans, Mode-05 O2, and readiness run inline on the
   same serialized loop as gauges.
5. The GUI performs the complete serial sweep inside `diagnostic_snapshot`
   while holding a mutex. The frontend invokes it from an unguarded 2.5 s
   interval, so requests can queue and the displayed state grows older.

J1850 VPW costs about 135-200 ms per request (about 5-7 requests/s). A uniform
250 ms full refresh is physically impossible. The runner must prioritize wire
time and publish each completed value promptly.

## 3. Approved decisions

| Decision | Choice |
| --- | --- |
| Discovery cadence | Once per VIN and matching probe context, persisted in `obd2-db`; manual `Rescan vehicle` |
| First-connect discovery | Mask scan first, then gauges run while individual capabilities verify in the background |
| Explicit rescan | Foreground scan with paused gauges and progress; old cache remains active until atomic replacement |
| Telemetry during diagnostic scan | Pause; retain and grey last-known gauge values |
| Passive DTC awareness | None; `03/07/0A` run only after `Run Diagnostic` |
| First surface | `obd2-gui`, including its background-runner refactor |
| Architecture | Shared mode runner in `obd2-dash`; GUI is a snapshot reader and command sender |
| Standard PID module attribution | Persist the broadcast union under `module='broadcast'`; current core APIs do not expose per-responder mask provenance |
| GUI snapshot cadence | 500 ms, with no overlapping invoke; event push remains out of scope |

The standard PID broadcast union is deliberate. The ELM path can OR multiple
ECU mask payloads, which is sufficient for broadcast telemetry scheduling.
Per-responder standard PID masks require response-source provenance that the
current `Session` contract does not expose and are not required by this phase.
Addressed profile signals and diagnostic services still retain their canonical
module.

## 4. Hard invariants

1. Exactly one runner owns the live `Session`; no GUI or TUI command performs
   serial I/O beside it.
2. Only `Unsupported` capability outcomes are permanently pruned.
   `Unverified` is never silently converted to unsupported.
3. `Unverified` requests do not enter normal telemetry tiers. They run only
   through the bounded verifier queue until classified or backed off.
4. No `03`, `07`, or `0A` request occurs before `RunDiagnostic`.
5. A failed or cancelled rescan cannot erase a previously valid capability set.
6. Cache replacement is one SQLite transaction.
7. Reconnect creates a new transport, adapter, and `Session`; it never assumes
   a broken serial link can be repaired by soft reinitialization.
8. Cancellation never drops an in-flight serial request future. It takes effect
   after the current request completes or times out.
9. A closed control channel is shutdown, not reconnect.
10. The GUI's snapshot command performs no blocking work and no serial I/O.

## 5. State machine and ownership

```text
Connecting --> Discovering(mask/cache miss) --> Telemetry <--> Diagnostic
     ^                   ^                       |
     |                   +-- RescanVehicle ------+
     +------------- Reconnecting <--------------+
```

- **Connecting:** use the connector to create a new `Session`, initialize it,
  acquire identity without probing supported PIDs, select a profile, build the
  probe fingerprint, and load the matching cache.
- **Discovering:** first-connect mask phase or the complete explicit rescan.
  First-connect leaves this state after masks; explicit rescan remains here
  through verification and persistence.
- **Telemetry:** capability-pruned tier scheduling. First-connect verification
  may run one bounded request per cycle alongside telemetry.
- **Diagnostic:** entered only after an accepted user command. Telemetry pauses.
- **Reconnecting:** the current foreground operation is marked interrupted, the
  broken session is dropped, and the connector recreates the lifecycle with
  backoff.

### 5.1 Ownership

| Repo / crate | Owns in this design |
| --- | --- |
| `obd2-core` | Identity-only session API; uncached supported-PID refresh; ELM multi-response union, continuation-bit walk, and error propagation |
| `crates/obd2-dash` | Mode runner, connector trait, capability store (trait plus `spawn_blocking` SQLite implementation), tier scheduler, capability state, discovery/verifier, diagnostics bundle, transport-neutral runner snapshot |
| `crates/obd2-db` | Versioned SQLite migration, capability models, synchronous transactional queries |
| `apps/obd2-gui/src-tauri` | Serial connector, capability-store instantiation (app-data DB path), runner lifecycle, thin Tauri commands, stable DTO/error mapping |
| `apps/obd2-gui` | Cached snapshot polling, view commands, scan controls, progress and stale-value presentation |

The `obd2-core` change is still mechanics-only, but it is broader than the
original adapter-local fail-open patch because the live call path and cache
belong to `Session`.

## 6. `obd2-core` prerequisite contract

The core change adds two explicit paths:

```rust
impl<A: Adapter> Session<A> {
    pub async fn identify_vehicle_identity(
        &mut self,
    ) -> Result<VehicleProfile, Obd2Error>;

    pub async fn refresh_supported_pids(
        &mut self,
    ) -> Result<HashSet<Pid>, Obd2Error>;
}
```

Contract:

- `identify_vehicle_identity` reads and validates VIN, matches the embedded
  vehicle spec, populates `Session::vehicle/spec/discovery`, and performs no
  supported-PID request. Its returned `VehicleProfile::supported_pids` is empty
  until a supported-PID scan succeeds.
- Existing `identify_vehicle` composes identity plus supported-PID discovery and
  propagates discovery failure. It no longer hides failure with
  `unwrap_or_default`.
- This failure propagation is a behavioral change for downstream `obd2-core`
  consumers pinned to older revs (HaulLogic-Desktop): at their next rev bump, a
  transient mask-scan failure fails `identify_vehicle` instead of yielding an
  empty supported set. Consumers that want lenient identity migrate to
  `identify_vehicle_identity`. Recorded under `COMP-OBD-SESS` at work-package
  registration.
- Existing `supported_pids` remains the cached read.
- `refresh_supported_pids` always touches the wire, bypasses the session cache,
  and replaces both the cache and `VehicleProfile::supported_pids` only after a
  complete successful scan.
- `Session` delegates the wire scan to the adapter's supported-PID mechanism
  instead of maintaining a second behaviorally different bitmap loop.
- `Elm327Adapter::supported_pids` decodes all payloads for each requested mask
  page and ORs them into the broadcast union.
- The scan begins at `0100` and requests the next page only when the union's
  continuation bit claims it. It never blindly sends all four pages.
- An error or malformed payload on `0100` returns `Err`.
- An error or malformed payload on a page whose continuation bit requested it
  also returns `Err`; a partial set is not cached as complete.
- A clear continuation bit is successful completion, not an error.
- Manual rescan calls `refresh_supported_pids`, so it cannot reuse the session
  cache accidentally.

The `obd2-dash` implementation must consume these APIs. It must not reproduce
bitmap parsing through `raw_request`.

## 7. Runner components

### 7.1 Connector and lifecycle

The runner is generic over a connector, not a preconstructed adapter:

```rust
#[async_trait]
pub trait SessionConnector: Send + Sync + 'static {
    type Adapter: Adapter;

    async fn connect(&self) -> Result<NewSession<Self::Adapter>, ConnectError>;
}
```

`NewSession` contains an uninitialized `Session` plus a stable connection
label. The runner always calls `Session::initialize`, so initialization state
and errors have one owner. The GUI implements a serial connector from port/baud
preferences. Synchronous port enumeration/open runs in `spawn_blocking` before
the session is returned. Tests use a scripted mock connector that can return
successive sessions or failures.

The runner owns `Option<Session<C::Adapter>>`. On transport loss it drops the
entire session before calling the connector again.

### 7.2 Commands and latest-value view state

Discrete commands use a bounded `mpsc` channel (capacity 8). Each interactive
command carries a `oneshot` acknowledgement so Tauri can return `accepted`,
`busy`, or `not_ready` without pretending queued work has started.

```rust
enum RunnerCommand {
    RunDiagnostic { reply: CommandReply },
    RescanVehicle { reply: CommandReply },
    CancelForeground { reply: CommandReply },
    RequestActiveTest {
        command: GmActiveTestCommand,
        reply: ActiveTestReply,
    },
    Shutdown { reply: ShutdownReply },
}
```

`ViewId` is a separate `watch` value. Repeated tab changes overwrite old state
instead of accumulating stale `SetActiveView` commands.

Command rules:

| Current mode | Run Diagnostic | Rescan | Cancel | Shutdown |
| --- | --- | --- | --- | --- |
| Connecting/Reconnecting | `not_ready` | `not_ready` | `not_running` | accepted |
| Initial Discovering | `not_ready` | `not_ready` | `not_running` | accepted |
| Telemetry | accepted | accepted | `not_running` | accepted |
| Rescan Discovering/Diagnostic | `busy` | `busy` | accepted | accepted |

Cancellation and shutdown are observed between serial requests. Duplicate scan
clicks never queue another scan. `RequestActiveTest` preserves the existing
locked/evidence behavior and is routed through runner-owned state; this phase
does not unlock new active-test writes. It is accepted only from Telemetry and
returns `not_ready`/`busy` in connection or foreground modes.

Telemetry accepts a foreground command even while background verification is
active. The verifier queue and retry state are paused, not discarded.
Diagnostic service outcomes merge into the pending initial replacement when no
`set_id` exists yet. A successful rescan supersedes the paused verifier map; a
cancelled/failed rescan resumes it.

The runner executes a planned cycle as an explicit request loop and checks
control state after every request; it does not hand an entire multi-request
cycle to an opaque helper that prevents boundary cancellation. The core
supported-PID refresh remains one bounded exception (at most four mask
requests); cancel/shutdown is observed when that call returns.

### 7.3 Snapshot

`RunnerSnapshot` lives in `obd2-dash` and contains domain values, signal
evidence, DTC/module results, mode, capability state, and the existing
active-test status. The runner publishes
`watch::Sender<Arc<RunnerSnapshot>>` with `send_replace` after each completed
request and state transition. Receiver clones are therefore atomic reference
count operations, not full snapshot clones.

```rust
mode: ModeState,
capability_state: CapabilityState,
foreground_result: Option<ForegroundResult>,
```

`ModeState` includes:

```text
Connecting
Discovering { origin: Initial | Rescan, phase, step, total }
Telemetry
Diagnostic { phase, phase_index, phase_total, step, total }
Reconnecting { attempt }
ShuttingDown
```

`CapabilityState` has two orthogonal axes so persistence problems do not hide
verification progress:

```text
CapabilityState {
    persistence: Cached | Pending | SessionOnlyNoVin | SessionOnlyStoreError,
    verification: Ready | Verifying { remaining }
                  | Degraded { unresolved } | ConservativeFallback,
}
```

Large unchanged collections inside the runner snapshot are shared and replaced
only when their content changes; the hot publication path must not clone the
entire signal/evidence/module payload per request. The owned, serializable Tauri
DTO preserves today's JSON field names and is built at the 500 ms IPC boundary.
That conversion is pure and contains no session access. Phase 2 may consume
`RunnerSnapshot` directly from the TUI.

### 7.4 Capability store async boundary

`obd2-db` remains synchronous. `obd2-dash` defines an async `CapabilityStore`
trait and a SQLite implementation that opens/migrates the database and executes
each operation in `spawn_blocking` around
`Arc<std::sync::Mutex<Database>>`. No SQLite call or standard mutex acquisition
runs on a Tokio worker or the Tauri setup thread.

Store calls are connection/rescan/diagnostic-boundary work, not hot-path work.
The same async wrapper exposes an exact-VIN vehicle fuel lookup for the
diagnostic fuel-resolution fallback; it does not perform pattern or network
lookup.

Initial-set and rescan replacement are awaited at a request boundary so the
runner receives the new `set_id` before publishing later updates. This may delay
the next cycle by the off-thread transaction duration, but it cannot block a
Tokio worker and it avoids a split-brain in-memory/DB generation.

## 8. Capability persistence

### 8.1 Schema

The migration uses `PRAGMA user_version` and preserves all existing tables and
rows.

The legacy `vehicles.supported_pids` text column is not this cache. It remains
untouched for compatibility and is never read or written by the mode runner.

```sql
CREATE TABLE vehicle_capability_sets (
    vin                  TEXT PRIMARY KEY CHECK(length(vin) = 17),
    set_id               TEXT NOT NULL CHECK(length(set_id) > 0),
    protocol             TEXT NOT NULL CHECK(length(protocol) > 0),
    profile_id           TEXT NOT NULL CHECK(length(profile_id) > 0),
    probe_schema_version INTEGER NOT NULL CHECK(probe_schema_version >= 1),
    probe_fingerprint    TEXT NOT NULL CHECK(length(probe_fingerprint) > 0),
    scan_completed_at    TEXT NOT NULL CHECK(length(scan_completed_at) > 0)
);

CREATE TABLE vehicle_capabilities (
    vin               TEXT NOT NULL CHECK(length(vin) = 17)
                      REFERENCES vehicle_capability_sets(vin)
                      ON DELETE CASCADE,
    kind              TEXT NOT NULL
                      CHECK(kind IN ('pid', 'profile_signal', 'service')),
    request_id        TEXT NOT NULL CHECK(length(request_id) > 0),
    module            TEXT NOT NULL CHECK(length(module) > 0),
    outcome           TEXT NOT NULL
                      CHECK(outcome IN ('supported', 'unsupported', 'unverified')),
    observation_seq   INTEGER NOT NULL CHECK(observation_seq >= 0),
    rtt_ms            INTEGER CHECK(rtt_ms IS NULL OR rtt_ms >= 0),
    last_attempted_at TEXT NOT NULL CHECK(length(last_attempted_at) > 0),
    last_error_code   TEXT,
    PRIMARY KEY (vin, kind, request_id, module)
);
```

Module values are canonical module IDs or the fixed sentinels `broadcast` and
`adapter`; `NULL` is forbidden. Standard Mode-01 requests use `broadcast`, and
`ATRV` uses `adapter`.

`protocol` uses explicit stable tokens such as `j1850_vpw` and
`can_11bit_500`, never `Debug` or user-facing display output.
Vehicles without a selected profile use the stable profile token `generic`,
not an empty string.

The probe fingerprint is a deterministic, sorted serialization of protocol,
selected profile ID, required capability keys, request/route descriptors, and
decoder IDs. Presentation-only tier cadence and active view are excluded
because they do not change whether a request is supported.
`probe_schema_version` changes when classification semantics change without
changing those descriptors. No hashing dependency is required.

`set_id` is a new opaque ID generated inside every atomic replacement
transaction. Incremental diagnostic/telemetry outcome updates carry the set ID
they observed and update only while it still matches the current VIN row. A
delayed write from before a rescan therefore cannot mutate the replacement set.
The replacement API does not accept a caller-provided set ID.

Each set also owns a monotonic `observation_seq`. The runner resumes from the
maximum loaded sequence and increments it for every classified observation.
Incremental SQL updates apply a row only when the incoming sequence is not older
than the stored sequence. This prevents an older coalesced telemetry batch from
overwriting a newer manual diagnostic result within the same set. Wall-clock
timestamps are informational and are never used for write ordering.

A cache is a hit only when VIN, protocol, profile ID, schema version, and
fingerprint all match. A mismatch is structural invalidation, not time-based
expiry.

### 8.2 Outcome semantics

| Outcome | Meaning | Scheduler behavior |
| --- | --- | --- |
| `supported` | A usable response was decoded, or a manual service returned a valid empty/non-empty response | Eligible for its normal tier/service |
| `unsupported` | Excluded by a complete standard mask, `UnsupportedPid`, an explicit unsupported-service NRC, or confirmed repeated `NO DATA` | Never scheduled until rescan/context invalidation |
| `unverified` | Not attempted, missing from an old set, transient error, stale response, malformed response, or retry budget exhausted | Verifier only; never normal telemetry |

Raw adapter responses and arbitrary error strings are not stored. The bounded
`last_error_code` uses stable classifications such as `timeout`, `stale`,
`transport`, or `decode`.

### 8.3 Store operations

```text
load_capability_set(vin, context)        -> Hit | Miss | ContextMismatch
replace_capability_set(replacement)      -> new set_id after atomic replacement
update_outcomes(vin, set_id, outcomes)   -> Applied | StaleSet
```

`replace_capability_set` uses one transaction. A rollback test must prove an
insert failure retains the old set. The set table allows an empty but completed
scan to be represented without fabricating a capability row.
`update_outcomes` verifies `(vin, set_id)` inside its transaction before any
upsert; checking it in application code before the transaction is insufficient.

Rare hot-path demotions update memory synchronously and publish a full
latest-value outcome batch to a persistence `watch` channel. The store worker
coalesces superseded batches and applies the newest batch with its `set_id`;
there is no unbounded write queue and overwriting a pending batch cannot lose a
different key because every batch contains the full current outcome set.
Shutdown flushes the latest accepted batch after releasing the serial session.
Foreground rescan replacement is awaited and never uses the background path.

## 9. Discovery

### 9.1 Cache miss on first connect

1. Enter `Discovering` and call `refresh_supported_pids`.
2. Follow only advertised continuation pages. Persist the resulting standard
   PID union under `module='broadcast'`.
3. Build the required telemetry capability set from display/tier configuration
   and the selected profile:
   - a mask-claimed configured PID starts `unverified`;
   - a configured PID excluded by a complete mask scan starts `unsupported`;
   - exception: a selected-profile `forced` PID excluded by the mask starts
     `unverified` and gets a controlled direct verifier request; it does not
     enter a normal tier unless that request succeeds;
   - profile signals and `ATRV` start `unverified`.
4. Enter `Telemetry` immediately. Order the verifier Tier A first, then Tier B,
   then visible Tier C. Execute at most one verifier request per telemetry
   cycle.
5. A successful verifier request publishes the value immediately and promotes
   the capability to `supported`; subsequent cycles schedule it normally.
6. Mask exclusion, `UnsupportedPid`, or an explicit unsupported NRC promotes it
   to `unsupported` immediately. A bare `NO DATA` must repeat on a second
   attempt separated by verifier backoff; the first leaves it `unverified`.
   Stale, timeout, or decode failures also leave it `unverified` and apply
   capped retry backoff. A transport failure leaves it `unverified` but
   immediately transitions the runner to Reconnecting; the broken session is
   not used for retries.
7. After the per-session retry budget (three attempts per entry), persist the
   full set atomically. Persistence becomes `Cached`; verification becomes
   `Ready` if no currently eligible entry remains unresolved, otherwise
   `Degraded { unresolved }`.

Tier C capabilities whose view has not been opened remain `unverified` and do
not keep the initial pass open or make verification `Degraded`. They are
dormant, enter the verifier when their view becomes active, and apply successful
classification through the incremental store path. `remaining` and
`unresolved` count only Tier A/B and active Tier C entries.

No DTC service is probed during this flow. Readiness and Mode-05 are learned
only during a manual diagnostic scan. Mode-06 is not in this phase's diagnostic
bundle and is neither probed nor persisted.

### 9.2 Mask failure

If the complete mask walk returns `Err`, set verification to
`ConservativeFallback`:

- persist nothing;
- enqueue only the configured display requests in Tier A/B/view order;
- attempt at most one unverified request per telemetry cycle;
- promote successes for the current session;
- never place failures into a normal tier;
- retain bounded backoff so a dead request cannot consume every cycle.

This fallback does not mean "poll everything." It is a session-local controlled
verification path.

### 9.3 No usable VIN

Run the mask and controlled verifier for the session, but persist nothing and
set persistence to `SessionOnlyNoVin`. Verification still reports
`Verifying`, `Degraded`, or `ConservativeFallback` independently. A corrupted
VIN is not a cache key.

### 9.4 Exact cache hit

Start Telemetry from cached `supported` entries immediately. No additional
capability mask walk occurs after initialization on the cache-hit path. Adapter
initialization may still emit `0100` as its protocol-selection liveness probe;
that request is not a discovery refresh. Any missing or cached `unverified`
required entry enters the background verifier and sets
verification to `Verifying { remaining }`.

### 9.5 Explicit rescan

`Rescan vehicle` uses a staged capability set:

1. Pause telemetry and retain last-known values.
2. Compute a stable progress total before wire work: four possible mask-page
   slots, every configured telemetry capability descriptor, and one persistence
   unit. Unneeded continuation pages and mask-excluded capabilities advance as
   skipped units, so the denominator never changes.
3. Force a fresh mask walk through `refresh_supported_pids`.
4. Verify the required telemetry capabilities in the foreground with request
   progress.
5. Do not run DTC, readiness, Mode-05, or Mode-06 requests.
6. Atomically replace the old set only after the foreground pass completes.

Cancel, mask failure, transport failure, or persistence failure discards the
staged set and leaves the prior DB set intact. After a non-transport failure,
the runner resumes Telemetry using the old in-memory set and reports the rescan
failure. This is the intended asymmetry: first connect prioritizes
time-to-gauges; explicit rescan prioritizes scan completion.

## 10. Telemetry mode

Normal tier membership is:

```text
display configuration intersect capability outcome == supported
```

The verifier queue is separate and cannot be expanded by tier scheduling.
Legacy profile `forced` standard-PID policy cannot override an
`unsupported` runner outcome. It may nominate a mask-excluded PID for the
controlled verifier and may order supported requests, but cannot put
`unverified` or `unsupported` work into a normal tier. The new runner does not
reuse the legacy scheduler's force-through behavior.

LLY defaults:

| Tier | Cadence | Default members |
| --- | --- | --- |
| A | Every cycle | RPM `010C`, MAP/boost `010B`, speed `010D`, fuel rail actual `0123`, VGT actual `lly.1543`, voltage `ATRV` |
| B | Every 5th cycle | Coolant `0105`, IAT `010F`, MAF `0110`, load `0104`, baro `0133`, desired MAP `lly.1542`, rail desired `lly.163D`, VGT desired `lly.1540` |
| C | Every cycle only while owning view is visible | Cylinder balance table (8 Class-2 reads), secondary/evidence profile signals |

- Expected fully verified Tier-A cycle on J1850 is about 6 requests or
  1.0-1.3 s. This is measured/likely, not a deadline contract.
- Tier-B cycles are expected around 2-2.5 s.
- The configured poll interval is a floor between cycle starts. An overrun does
  not create overlapping cycles.
- `ViewId` is read once when building each cycle. A view change affects the next
  cycle without replaying intermediate tabs.
- A non-transport failure from a normally scheduled `supported` request never
  changes it directly to `unsupported`. Demote it to `unverified`, retain the
  last-known value, and re-enter it through verifier backoff. This removes a
  newly dead request from the hot tier without turning one transient failure
  into permanent vehicle truth. Apply the in-memory demotion before the next
  cycle and persist it asynchronously when a writable cache exists.
- `03/07/0A`, readiness, Mode-05, and Mode-06 never run in Telemetry.

## 11. Diagnostic mode

`RunDiagnostic` is accepted only from Telemetry. Telemetry pauses and gauges
show greyed last-known values.

Progress uses five stable top-level phases, so the overall denominator never
changes after the command begins. Each phase may expose request subprogress;
the freeze-frame sub-total becomes known after the DTC phase.

1. **DTCs:** stored/pending/permanent, broadcast then discovered modules,
   including the selected-profile DTC path and GM Class-2 backoff behavior.
2. **Freeze frames:** one substep per code found using the existing correlated
   PID set.
3. **Readiness:** skip only when a matching service row is already
   `unsupported`; `unverified` is attempted because the operator explicitly
   requested diagnostics.
4. **Mode-05 O2:** run only when fuel type is explicitly gasoline, protocol is
   non-CAN, and the row is not already `unsupported`. Unknown fuel type skips
   fail-closed. LLY never sends Mode-05.
5. **Module refresh:** update existing GUI module-scan data.

Mode-06 is deliberately absent.

Fuel classification is deterministic: use the selected embedded
`Session::spec().identity.engine.fuel_type` first, then the exact-VIN
`obd2-db` vehicle row, otherwise `Unknown`. Normalize only recognized fuel
labels into `Gasoline`, `Diesel`, or `Other`; do not infer fuel from a profile
name, vehicle display string, or the presence of O2 PIDs. Only `Gasoline`
permits Mode-05.

Manual diagnostic results update service capability rows:

- valid empty/non-empty response -> `supported`;
- explicit unsupported NRC -> `unsupported`;
- first bare no-data -> `unverified` with error code `no_data`; a later
  separated no-data with no intervening different outcome -> `unsupported`;
- stale, decode, or other non-transport error -> `unverified`;
- transport error -> abort the bundle, mark it interrupted, and reconnect.

Non-transport per-step failures produce `DiagnosticScanResult` and the bundle
continues. Cancel completes after the current request, retains clearly marked
partial results, and resumes Telemetry. Normal completion publishes the full
result and resumes Telemetry automatically.

## 12. GUI integration

- Tauri setup creates the channels and initial snapshot, then spawns one
  background bootstrap. The bootstrap opens/migrates the SQLite capability
  store off-thread, creates the serial connector, and starts the runner.
- `diagnostic_snapshot` clones the current snapshot `Arc`, converts it to the
  owned DTO, and returns without I/O or waiting on the runner.
- The frontend reads it every 500 ms using a completion-scheduled timeout or an
  explicit in-flight guard; invokes never overlap. Browser fixture/replay-only
  mode does not run the live Tauri polling loop.
- New commands: `run_diagnostic`, `rescan_vehicle`,
  `cancel_foreground`, and `set_active_view`.
- Existing `request_active_test` becomes a runner command and retains its
  current locked behavior and evidence output.
- Foreground scan buttons disable after acceptance. The active scan exposes one
  Cancel command.
- Diagnostic/rescan modes grey, but do not clear, last-known gauges.
- The connection area reports persistence and verification independently, for
  example `cached + ready`, `cached + verifying`, `session only: no VIN`, or
  `session only: storage error + fallback`.
- All inline serial I/O, connect-inside-snapshot logic, and mutex-held I/O are
  deleted from the GUI backend.
- An architectural test prevents `apps/obd2-gui/src-tauri` command modules from
  importing `Session<Elm327Adapter>` or issuing raw/session requests.

The 500 ms interval bounds presentation latency without increasing wire load.
Snapshot clone/serialization cost is measured in the GUI integration test and
truck validation; the interval may be raised only with recorded evidence that
500 ms harms the UI.

## 13. Error handling and shutdown

- Reconnect performs three short-backoff attempts, then retries indefinitely at
  a slower capped cadence. Every attempt calls the connector and constructs a
  new lifecycle.
- Reconnect reacquires identity before loading a cache. It never applies the
  prior VIN's cache before identity is known.
- An unfinished first-connect verifier map may resume only after reconnect
  confirms the same VIN, protocol, profile, and fingerprint. Otherwise it is
  discarded. Explicit-rescan staging is always discarded on transport loss.
- A foreground diagnostic interrupted by transport loss is not silently
  restarted after reconnect. Its result says `interrupted`, then the new
  session resumes Telemetry/cache discovery normally.
- A foreground rescan interrupted by transport loss discards its staged map.
- Existing stale PID response classification carries over. A stale response is
  not evidence of unsupported capability.
- Command-channel close and `Shutdown` stop accepting work, wait for the current
  request boundary, drop the session, and release the port.
- A requested `Shutdown` acknowledgement is sent only after the session is
  dropped and the latest accepted persistence batch is flushed. Channel close
  follows the same cleanup path without an acknowledgement.
- SQLite/store errors do not stop live telemetry if an in-memory capability set
  is usable. They surface in the snapshot and leave the previous DB set intact.
- If the database cannot open or migrate at startup, bootstrap starts the runner
  with a disabled store, reports the storage error, and uses session-local
  discovery with persistence `SessionOnlyStoreError`. Database availability is
  not a prerequisite for gauges.

## 14. Testing and evidence

### 14.1 `obd2-core`

- Identity-only acquisition does not invoke supported-PID discovery. This
  assertion starts after adapter initialization, whose protocol selection may
  use `0100`.
- `identify_vehicle` propagates a first-page mask failure.
- Cached `supported_pids` performs one scan; `refresh_supported_pids` performs
  another wire scan and replaces the cache.
- A clear continuation bit prevents `0120`.
- A claimed continuation page is requested.
- First-page and continuation-page errors return `Err` and cache nothing.
- Multiple ECU payloads are ORed into the broadcast union.

### 14.2 `obd2-db`

- Migration upgrades a legacy database without losing vehicle/threshold rows.
- The legacy `vehicles.supported_pids` column is neither loaded nor mutated by
  capability-set operations.
- `module` cannot be null and duplicate primary keys conflict.
- Outcome check constraints reject invalid strings.
- Atomic replace rollback retains the prior set.
- A delayed incremental update carrying an old `set_id` cannot mutate a
  replacement set.
- An older `observation_seq` cannot overwrite a newer row within the same set.
- Exact context hit and fingerprint/schema mismatch behavior.
- Manual service outcome update is transactional.

### 14.3 Runner

- Cache miss: masks -> Telemetry -> bounded verification -> atomic persistence.
- Cache hit: no post-initialization capability mask walk; cached gauges begin
  immediately.
- Unverified/error requests never enter normal tiers.
- A single bare `NO DATA` remains unverified; only the separated confirmation
  prunes it.
- Confirmed unsupported requests are never scheduled again.
- A profile-forced standard PID cannot override an unsupported outcome.
- A failed formerly-supported telemetry request leaves the hot tier and is
  reclassified through the verifier.
- Fallback never becomes a poll-everything sweep.
- Rescan cancel/failure retains old in-memory and DB maps.
- Rescan progress uses a stable denominator and counts skipped mask/capability
  units without sending them.
- Mock connector proves transport loss creates a second adapter/session.
- Duplicate foreground commands return `busy` and do not queue.
- Foreground work pauses and resumes background verification without losing
  retry/outcome state.
- Latest view state gates Tier C without replaying stale views.
- No `03/07/0A` occurs before `RunDiagnostic`.
- Diesel and unknown-fuel cases never send Mode-05.
- Embedded-spec fuel classification wins over the DB fallback, and no string
  heuristic can turn unknown fuel into gasoline.
- Diagnostic progress has a stable five-phase denominator.
- Shutdown/cancel waits for the current request boundary.

### 14.4 GUI and emulator

- Tauri snapshot command performs no I/O and returns the latest watch value.
- Frontend fake-clock test proves 500 ms cadence with no overlapping invoke.
- Controls reflect accepted/busy/cancelled/completed states.
- Emulator transport log proves request mix, Tier-A dominance, unsupported
  pruning, and manual-only diagnostics.
- Architectural source scan proves GUI commands cannot reach the live session.

### 14.5 Hardware

Repeat the LLY/STN/J1850 run and record:

- build SHA and core dependency SHA;
- connect-to-first-RPM time on cache miss and hit;
- wire request counts by tier/service;
- runner value age and GUI presentation age separately;
- Tier-A cycle distribution;
- reconnect behavior after physical adapter interruption;
- explicit rescan and diagnostic completion/cancel behavior.

Update the hardware matrix because connect and telemetry behavior are
user-visible.

Commands:

```bash
# obd2-core workspace
cargo test
cargo clippy --all-targets --all-features -- -D warnings

# obd2-dash workspace
cargo test
cargo clippy --workspace --all-targets --all-features -- -D warnings

# GUI
cd apps/obd2-gui
npm run build
npx playwright test
cd src-tauri
cargo test
```

## 15. OWL review

These are the failure paths a future caller is expected to attempt:

- Calling cached `supported_pids` from rescan. The rescan contract names the
  forced-refresh API and has a wire-count regression test.
- Reintroducing bitmap parsing with `raw_request` in the dash runner. An
  architectural test restricts mask mechanics to core.
- Treating timeout/decode/stale as unsupported. The outcome enum and scheduler
  tests allow pruning only for `Unsupported`.
- Deleting the old DB set before a rescan finishes. Staging plus a rollback test
  make atomic replacement the only persistence path.
- Soft-reinitializing a broken adapter. The connector test requires a second
  constructed session.
- Flooding an unbounded channel with tab changes or duplicate scan commands.
  View state is watch-based; discrete commands are bounded and acknowledged.
- Cancelling by dropping a request future and leaving the ELM response queued.
  Cancellation is checked only at request boundaries.
- Moving serial work back into a Tauri command. The GUI architectural test
  forbids the imports/call sites.
- Forgetting cache invalidation when profile requests change. The deterministic
  fingerprint changes with required request/route/decoder descriptors, while a
  tier-only UI change deliberately preserves the cache.
- Claiming 1 Hz gauges while presenting at 0.4 Hz. Hardware evidence records
  runner age and presentation age independently.

## 16. Non-goals

- TUI migration to the shared runner. The legacy `session_runner.rs` remains
  functional until Phase 2.
- Event-push snapshot delivery.
- Time-based capability expiry.
- Per-responder standard PID mask provenance.
- Mode-06 diagnostics.
- New active-test write capability.
- Moving the proven runner/scheduler into `obd2-core` before both GUI and TUI
  consume it.
- HOS/ELD changes, raw-capture format changes, or profile-definition expansion.
