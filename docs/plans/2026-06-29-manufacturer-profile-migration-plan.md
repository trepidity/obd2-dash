# Manufacturer Profile Migration Plan

Status: draft implementation plan
Date: 2026-06-29
Scope: production-level reusable diagnostic profiles for manufacturer-specific support

## Purpose

Build a production profile architecture for manufacturer-specific diagnostics.

The current code has a strong GM Class 2 / LLY Duramax foundation, but it is
not yet the reusable pattern for GM, Ford, Chrysler/Ram, and future vehicle
families. The migration must make this rule true by construction:

```text
No manufacturer-specific routed request can be sent unless the live session has
selected an exact matching profile that owns that request.
```

Generic SAE OBD-II remains available for every vehicle. Manufacturer-specific
reads, DTC services, passive monitors, evidence capture, and active tests are
enabled only through the selected profile.

## Current State

Implemented and useful:

- GM Class 2 routing helpers and `$19` DTC decoding exist.
- A GM enhanced registry exists with DID, selector, module, TXD/RXF/RXD/MTH,
  confidence, provenance, cadence, failure policy, and rejected-DID metadata.
- The LLY gate checks J1850 VPW, VIN identity, vehicle spec identity, engine,
  displacement, fuel type, and VIN eighth digit.
- TUI/session polling can append GM enhanced targets and scan GM Class 2 `$19`.
- GUI live mode has its own GM gate and its own GM request path.
- Active-test scaffolding exists and refuses execution until verified command
  bytes exist.
- GM evidence files exist for probes and blocked active-test attempts.

Do not describe this as full GM support. The accurate status is:

```text
GM Class 2 LLY-focused enhanced diagnostics foundation, with strict local gate,
but without a reusable manufacturer profile runtime yet.
```

## Production Invariants

These are non-negotiable:

1. The session owns profile selection.
2. UI code does not construct GM, Ford, Chrysler/Ram, or other manufacturer
   routed requests.
3. A selected profile token is required for every manufacturer-specific routed
   request.
4. No profile means generic SAE OBD-II only.
5. A partial profile match is visible for diagnostics but cannot poll
   manufacturer-specific requests.
6. Multiple exact profile matches are an ambiguity error, not a best guess.
7. Active tests remain locked unless a profile provides verified command bytes,
   preconditions, timeout behavior, cancel behavior, and evidence policy.
8. Recording and evidence must preserve raw request/response bytes for disputed
   manufacturer claims.
9. TUI and GUI must consume the same profile runtime and planned request graph.

## Terminology

Manufacturer:

- OEM family: GM, Ford, Chrysler/Ram, etc.
- Not enough by itself to select behavior.

Profile:

- A concrete vehicle/protocol support package.
- Example: `gm.gmt800.lly.class2`.
- Owns match rules, routed requests, decoders, DTC services, active tests,
  passive monitors, poll policy, evidence policy, and display metadata.

Vehicle context:

- Immutable session identity snapshot used for profile matching.
- Contains protocol, VIN, decoded spec, discovery state, active bus, modules,
  and adapter capabilities.

Selected profile:

- Runtime token created only by the profile resolver.
- Required by the profile dispatcher before sending profile-owned requests.

Capability:

- A profile-owned diagnostic feature such as a signal, DTC service, active
  test, or passive monitor.

Evidence:

- Durable bytes plus decoder metadata proving what was requested, what was
  returned, and how the system interpreted it.

## Target Module Layout

Start inside `obd2-dash`. Move to a shared crate later only if another binary
needs the same API.

```text
crates/obd2-dash/src/profiles/
  mod.rs
  model.rs
  registry.rs
  runtime.rs
  scheduler.rs
  evidence.rs
  recording.rs
  gm/
    mod.rs
    class2.rs
    lly.rs
    active.rs
  ford/
    mod.rs
  chrysler/
    mod.rs
```

The existing `gm_enhanced.rs`, `gm_class2.rs`, `gm_active.rs`, and
`gm_evidence.rs` should be migrated behind this boundary, not copied.

## Core Types

The exact Rust shape can change during implementation, but the boundaries must
exist.

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProfileId(&'static str);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Manufacturer {
    Gm,
    Ford,
    ChryslerRam,
    Generic,
}

pub struct VehicleContext {
    pub generation: u64,
    pub protocol: obd2_core::vehicle::Protocol,
    pub vin: Option<String>,
    pub vin_confidence: IdentityConfidence,
    pub spec: Option<obd2_core::vehicle::VehicleSpec>,
    pub discovered_modules: Vec<obd2_core::domain::ModuleId>,
    pub active_bus: Option<String>,
}

pub enum ProfileMatch {
    Exact { confidence: MatchConfidence },
    Partial { reason: String },
    NoMatch,
}

pub struct SelectedProfile {
    profile_id: ProfileId,
    context_generation: u64,
    // Private fields prevent callers from fabricating a selected profile.
    _sealed: (),
}
```

`ChryslerRam` is intentionally one manufacturer family here. Public branding
changed over time, but the diagnostic integration should be grouped by protocol
family and platform support, not by badge text.

Profile definition:

```rust
pub trait DiagnosticProfile {
    fn id(&self) -> ProfileId;
    fn manufacturer(&self) -> Manufacturer;
    fn matches(&self, ctx: &VehicleContext) -> ProfileMatch;
    fn standard_pid_overrides(&self) -> &[StandardPidOverride];
    fn signals(&self) -> &[SignalDefinition];
    fn dtc_services(&self) -> &[DtcServiceDefinition];
    fn active_tests(&self) -> &[ActiveTestDefinition];
    fn passive_monitors(&self) -> &[PassiveMonitorDefinition];

    fn decode_signal(
        &self,
        signal: &SignalDefinition,
        payload: &[u8],
    ) -> Result<DecodedSignal, ProfileDecodeError>;

    fn decode_dtc_response(
        &self,
        service: &DtcServiceDefinition,
        payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError>;
}
```

Signal definition:

```rust
pub struct SignalDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub category: SignalCategory,
    pub route: RouteDefinition,
    pub service_id: u8,
    pub request_data: &'static [u8],
    pub decoder_id: &'static str,
    pub unit: &'static str,
    pub cadence: PollCadence,
    pub confidence: Confidence,
    pub provenance: &'static [Provenance],
    pub source_fields: SourceFields,
    pub evidence_policy: EvidencePolicy,
    pub failure_policy: FailurePolicy,
    pub preferred_over: Option<&'static str>,
}
```

`source_fields` preserves vendor-auditable data such as ScanGauge TXD, RXF,
RXD, RXD width, raw MTH, and source URL or document id. Execution may use a
compiled decoder, but the published source fields must remain inspectable. This
prevents regressions such as losing the fuel-rail `RXD=3008` range caveat after
moving decode logic behind `decoder_id`.

DTC service definition:

```rust
pub struct DtcServiceDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub route_set: RouteSet,
    pub service_id: u8,
    pub request_data: &'static [u8],
    pub decoder_id: &'static str,
    pub backoff_policy: BackoffPolicy,
}
```

Active-test definition:

```rust
pub struct ActiveTestDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub safety_class: SafetyClass,
    pub command_profile: ActiveCommandProfile,
    pub preconditions: &'static [ActivePrecondition],
    pub timeout: std::time::Duration,
    pub cancel_command: Option<ProfileRequestDefinition>,
    pub evidence_policy: EvidencePolicy,
}
```

## Profile Matching Floor

Individual profiles may provide their own match logic, but the framework must
enforce a minimum floor before any profile can return `Exact`.

An exact match requires:

- protocol family match.
- at least one VIN-derived identity dimension, such as model year, engine digit,
  platform, or manufacturer WMI.
- vehicle spec consistency when a decoded spec is available.
- no explicit module evidence contradicting the profile.

The registry must include a cross-profile corpus test:

- no two profiles return `Exact` for the same `VehicleContext`.
- no profile returns `Exact` without protocol plus VIN-derived identity.
- corrupted VIN text does not produce a false `Exact`.
- partial matches remain visible but cannot create a `SelectedProfile`.

Ambiguity is a diagnostic state. When multiple profiles claim exact match, log
the colliding profile ids and the match evidence that caused the collision.

## Vehicle Identity Lifecycle

The profile resolver must not snapshot identity from a single weak VIN read.
J1850/ELM traffic can be lossy, and this truck has already shown VIN character
corruption such as `I`/`1` confusion in live sessions.

Identity acquisition rules:

- read VIN more than once when the first read is malformed or disagrees with
  the last known session identity.
- compute a VIN confidence state before profile selection.
- reject corrupted VINs for `Exact` matching.
- allow generic OBD-II while identity is still uncertain.
- cache last-good identity for the current session and mark when it is being
  used.
- invalidate cached identity on disconnect, adapter change, or explicit user
  reset.

Manual profile confirmation may exist, but it must be deliberate and visible:

- user must explicitly choose the suspected profile.
- UI must show that the profile is manually confirmed because live identity is
  weak.
- active tests remain locked under manual confirmation unless the normal exact
  match is later established.
- all evidence records must mark manual confirmation.

Selected profile tokens are valid only for their `VehicleContext.generation`.
Disconnect, reconnect, adapter change, protocol change, VIN change, or decoded
spec change increments the generation and invalidates all old tokens.

## Profile Runtime

The runtime is the only place that may execute manufacturer-specific routed
requests.

```text
VehicleContext
  -> ProfileRegistry::select
  -> Option<SelectedProfile>
  -> ProfileRuntime::plan_poll_cycle
  -> ProfileRuntime::execute_request
  -> profile decoder
  -> domain messages
```

The dispatcher must validate:

- selected profile id.
- capability id belongs to selected profile.
- protocol family matches the selected profile.
- route/module belongs to the selected capability.
- service id and request payload match the capability definition.
- active-test safety state, if applicable.
- evidence policy.

Probe examples may keep lower-level routed access only behind a probe-only API
boundary. That boundary must be unavailable to live GUI/TUI code by visibility,
feature flag, crate split, or an equivalent compile-time constraint.

Required guardrails:

- live dashboard modules cannot import probe-only raw routed helpers.
- probe tools write evidence by default.
- probe-only requests are labeled as probe traffic in evidence.
- an architectural test fails if live code calls raw manufacturer routed APIs
  outside `ProfileRuntime::execute_request`.

## Layered Architecture and Protocol Separation

The system has five owned layers. Each layer has one job and a forbidden list.
A request flows down; decoded data flows up. No layer reaches around another, and
no layer above Layer 1 builds transport framing or adapter command bytes.
Profiles may own diagnostic service/payload bytes because those bytes are the
capability being requested; they must not own headers, transport framing, or ELM
setup commands.

```text
Layer 4  Domain / UI             consumes DecodedSignal, DecodedDtc, coverage
            ^   typed values only; never bytes, headers, or DIDs
Layer 3  Profile (manufacturer)  WHAT to ask + HOW to decode it
            |   owns: SignalDefinition, DtcServiceDefinition, decoders, match rules
            |   forbidden: wire framing, transport choice, ELM AT commands
Layer 2  Profile Runtime + Dispatch   protocol-agnostic execution + safety gate
            |   owns: SelectedProfile validation, RouteDefinition -> PhysicalAddress,
            |          scheduling, evidence
            |   forbidden: manufacturer knowledge, hardcoded DIDs, per-OEM branches
Layer 1  Protocol Adapter (obd2-core)  HOW bytes go on the wire
            |   owns: Adapter trait, ELM327, AT setup, framing, BusFamily codec,
            |          negative-response detection
            |   forbidden: manufacturer semantics, profile awareness
Layer 0  Transport (obd2-core)   serial / BLE byte pipe
```

The protocol-adapter boundary already exists in obd2-core and must not move into
the profile layer:

- `trait Adapter` (`../obd2-core/crates/obd2-core/src/adapter/mod.rs` in the
  current workspace layout) is the only seam to the wire: `initialize`,
  `request`, `routed_request`, `supported_pids`, `battery_voltage`.
- `PhysicalAddress` already generalizes addressing across protocols:
  `J1850 { node, header }`, `Can11Bit { request_id, response_id }`,
  `Can29Bit { request_id, response_id }`, and `J1939 { source_address }`.
  J1939 still needs a complete protocol/adapter/codec path before profiles can
  rely on it.
- `Protocol` already covers `J1850Vpw`, `J1850Pwm`, `Can11Bit500/250`,
  `Can29Bit500/250`.
- `codec::decode_frame` dispatches per `BusFamily` (`Can`, `J1850`, `Iso9141`,
  `Kwp2000`) in separate match arms, so per-protocol decode is already isolated.

Profiles consume this boundary; they never reimplement it.

### Protocol is a route property, not a profile property

A capability route owns the protocol and address, not the profile id. This lets
one profile span more than one bus (for example a truck exposing J1850 Class 2
enhanced data and a separate CAN bus). This is the concrete shape of the
`RouteDefinition`/`RouteSet` referenced under Core Types. The full address and
module model is authoritative in the companion
`2026-06-29-module-support-architecture.md`; this summary is reconciled to it.

`RouteDefinition` carries the module reference ONLY. The bus and address are NOT
repeated on the route; they live once in the profile `ModuleMap` (a table of
`ModuleDefinition`s), so a node such as the ECM's `0x10` is defined in one place
instead of being copied onto every signal (today's `gm_enhanced` repeats the ECM
node in 22 of 24 DID entries -- exactly the leakage this kills):

```rust
pub struct RouteDefinition {
    pub module: ModuleKey,   // identity always from the route; never a display-string fallback
}

// Bus + address live on the ModuleDefinition in the profile ModuleMap, not on the route.
// The J1850 header [priority, node, source] is composed from bus data, never a dispatcher constant.
pub enum AddressTemplate {
    J1850 { node: u8 },
    Can11 { request_id: u16, response_id: u16 },
    Can29 { request_id: u32, response_id: u32 },
}
```

The profile id segment such as `class2` or `can` is documentation. The routing
source of truth is the profile `ModuleMap`, keyed by `RouteDefinition.module`.
Profile matching still declares which protocols a profile can safely run on;
per-capability routing resolves through the module map. A module is a route
target inside one profile, never a sub-profile.

### The protocol-agnostic dispatcher

`ProfileRuntime::execute_request` is the single execution path and contains no
manufacturer-specific branches. Protocol variation is limited to converting
`AddressTemplate` into `PhysicalAddress`; that mapping is mechanical route
resolution, not manufacturer behavior. It:

1. validates the `SelectedProfile` (id + generation).
2. confirms the capability and route belong to the selected profile.
3. resolves `RouteDefinition.module` through the profile `ModuleMap` to an
   address, then to `obd2_core::vehicle::PhysicalAddress` (J1850 header composed
   from bus data, never a dispatcher constant).
4. builds a `RoutedRequest { service_id, data, target }`.
5. calls `Adapter::routed_request`; obd2-core handles framing and the active
   protocol.
6. hands the raw payload to the profile decoder identified by `decoder_id`.
7. writes evidence.

There is no `match manufacturer { Gm => ..., Ford => ... }` anywhere in Layer 2.
There may be a small exhaustive `match AddressTemplate` for route resolution.

### Where each kind of change lands (the extensibility contract)

```text
Add a vehicle/engine for an existing manufacturer on a known protocol
  -> new profile data under profiles/<mfr>/, register it, add a golden corpus
  -> touches:      profiles/<mfr>/*  +  registry registration  +  tests/corpus
  -> never touches: dispatcher, other profiles, adapter, codec

Add a new manufacturer on a protocol obd2-core already speaks
  -> new profiles/<mfr>/ module: decoders + match rules + corpus
  -> touches:      profiles/<mfr>/*  +  registry  +  Manufacturer enum variant
  -> never touches: dispatcher logic, other profiles, adapter, codec

Add a protocol obd2-core models but the dispatcher has no AddressTemplate for
  -> add one additive AddressTemplate arm + its PhysicalAddress mapping
  -> touches:      profiles/runtime route resolution (additive arm)  +  tests
  -> never touches: existing arms, other profiles

Add a brand new wire protocol obd2-core does not speak
  -> obd2-core change: new Protocol + BusFamily + codec arm (+ maybe Adapter impl)
  -> additive only: new enum variants and match arms; existing arms untouched
  -> guarded by the obd2-core protocol golden corpus (Layer 1), not the profile layer
```

This is the explicit answer to "handle all the models easily": every model is
additive data plus, at most, one additive route/protocol arm. The shared
execution path is written once and never branched per OEM.

## Regression Firewall

Goal: adding a model, manufacturer, or protocol cannot change the behavior of an
already-supported one. This is enforced by pinned behavior, additive-only shared
changes, and a required-green corpus, not by reviewer vigilance.

### 1. Golden replay corpus (the primary firewall)

Every supported profile and every supported protocol family ships a frozen
corpus of real captured traffic with expected decoded output.

- Seed it now from existing real captures under `raw-captures/` (the LLY J1850
  VPW sessions) plus synthetic fixtures per `BusFamily`.
- A corpus entry is: raw request/response bytes + expected DecodedSignal /
  DecodedDtc / error classification.
- CI replays every entry through the real decoders and asserts identical output,
  byte-for-byte and value-for-value.

```text
tests/corpus/
  protocol/j1850-vpw/*.jsonl              // frame-decode goldens, per BusFamily
  protocol/can-11bit/*.jsonl
  profile/gm.gmt800.lly.class2/*.jsonl    // request -> decoded value/DTC goldens
  profile/<next-profile>/*.jsonl
```

Rule: no change merges if any existing corpus entry changes output. New support
adds new corpus files; it never edits existing expected outputs unless a bug is
being deliberately corrected and called out in the commit.

### 2. Additive-only changes to shared layers

The shared layers (Layer 1 adapter/codec, Layer 2 dispatcher/runtime) may only
grow by addition:

- new `Protocol`, `BusFamily`, `PhysicalAddress`, and `AddressTemplate` variants
  are additive; existing variants and their match arms are not modified.
- the dispatcher gains no per-manufacturer branch, ever.
- a shared-layer signature change requires the full protocol + profile corpus to
  pass before merge.

### 3. Isolation requirements already in this plan are part of the firewall

- `SelectedProfile` + dispatcher: a new profile cannot fire on a vehicle it does
  not exactly match.
- `decoder_id` ownership: a new profile cannot decode through another profile's
  registry.
- cross-profile corpus test: no two profiles claim the same vehicle.
- generation-bound tokens: stale authority cannot execute.

### 4. Change protocol (what a contributor must do)

```text
Adding a profile or model:
  1. add profiles/<mfr>/... data + decoders + match rules
  2. register in the profile registry
  3. add a golden corpus for the new profile
  4. run existing protocol corpus + existing profile corpora + new corpus
  5. all existing goldens stay green with zero diffs

Changing a shared layer (adapter / codec / dispatcher):
  1. change must be additive where possible
  2. run the full protocol + profile golden corpus
  3. any output diff on an existing entry blocks the merge
  4. a deliberate decode correction updates exactly one golden, with a written
     reason in the commit
```

### 5. CI gate (required green before merge)

```text
cargo test -p obd2-core      // protocol/codec/adapter unit + protocol golden corpus
cargo test -p obd2-dash      // profile runtime, dispatcher, decoders, profile corpus
profile selection corpus     // no false or overlapping exact matches
architectural import test    // live code cannot reach probe-only raw routed APIs
replay compatibility         // old recordings still replay identically
```

The LLY is the first frozen baseline. Once its golden corpus exists, no future
manufacturer, model, or protocol work may change LLY decoded output without an
explicit, reviewed, single-purpose correction.

## Migration Phases

### Phase 0: Freeze New Manufacturer Work

Do not add Ford, Chrysler/Ram, later GM GMLAN, or broad "Chevy" behavior until
the profile boundary exists.

Allowed work:

- bug fixes.
- evidence capture.
- tests proving current LLY safety.
- read-only migration that preserves current LLY behavior.

### Phase 1: Add Neutral Profile Model

Add `profiles::model`, `profiles::registry`, and `profiles::runtime` with no
behavior change.

Wrap the current LLY implementation as:

```text
profile_id: gm.gmt800.lly.class2
manufacturer: GM
protocol: J1850 VPW / Class 2
platform: GMT800
engine: LLY Duramax
```

Keep current LLY tests and add profile-selection tests:

- exact LLY match succeeds.
- missing VIN or spec falls back to generic only.
- corrupted VIN text falls back to generic or partial, never exact.
- wrong VIN eighth digit rejects.
- wrong protocol rejects.
- wrong engine/spec rejects.
- no registered profile can return exact without protocol plus VIN-derived
  identity.
- the registered profile corpus has no overlapping exact matches.

### Phase 2: Session-Owned Profile Selection

Build one `VehicleContext` after vehicle identification, VIN confidence checks,
and discovery.

The session stores:

```rust
pub struct ProfileState {
    pub generation: u64,
    pub selected: Option<SelectedProfile>,
    pub exact_matches: Vec<ProfileId>,
    pub partial_matches: Vec<PartialProfileMatch>,
    pub ambiguity: Option<ProfileAmbiguity>,
}
```

Remove scattered live gates as decision points. Call sites may read profile
state, but they may not re-decide whether LLY applies.

Profile state invalidates on disconnect, reconnect, adapter change, protocol
change, VIN change, decoded spec change, or manual identity reset. Stale
`SelectedProfile` tokens must fail dispatcher validation.

If VIN reads are malformed or inconsistent, retry before profile selection and
surface an identity-confidence warning. Use generic OBD-II until either identity
becomes exact or the user deliberately confirms a manual profile override.

### Phase 3: Central Profile Request Dispatcher

Replace direct manufacturer requests in live paths with:

```rust
execute_profile_request(
    selected_profile: &SelectedProfile,
    capability_id: CapabilityId,
    request_id: RequestId,
)
```

This phase must remove or quarantine:

- GUI `request_gm_node` as a live dashboard API.
- TUI/session ad hoc GM `$19` execution.
- direct `find_lly_did` from generic enhanced target execution.
- direct use of LLY DIDs outside the LLY profile module.
- probe-only raw routed request APIs from live GUI/TUI modules.

The TUI and GUI should receive planned profile requests from the same runtime.

### Phase 4: Move Poll Policy Into Profiles

Move these policies out of global/session code:

- forced standard PID reads caused by this truck's unreliable Mode 01 bitmap.
- enhanced poll cadence.
- no-data/unsupported backoff.
- candidate-DID suppression.
- preference rules such as generic actual rail over range-suspect enhanced rail.

For the LLY profile, the forced standard Mode 01 PIDs remain valid. They are
not a global rule for every vehicle.

This is a deliberate behavior change: a GM vehicle that does not exactly match
the LLY profile may lose the LLY bitmap workaround and therefore may show fewer
forced standard values. That is safer than sending profile assumptions to the
wrong vehicle. If another GM profile has the same bitmap behavior, it must own
its own standard PID override policy.

### Phase 5: Migrate GM LLY Definitions

Move the current GM LLY data under the profile:

- VGT desired/actual.
- injector balance.
- desired/actual fuel rail.
- desired MAP candidate.
- barometer candidate.
- oil pressure.
- TCM transmission temperature.
- injector pulse width.
- rejected DIDs.
- GM `$19` services.
- active-test placeholders.

Fix known leakage during this migration:

- Do not hardcode every enhanced module label as `ecm`.
- TCM signals must carry TCM route/module identity.
- Node `0x11` remains unresolved until capture proves identity.
- Desired MAP `$1542` remains provisional unless cross-checked.
- Barometer `$1251`/`$119D` remains profile evidence-gated.
- ScanGauge-sourced signals preserve TXD/RXF/RXD/MTH/source metadata even when
  runtime decoding uses compiled Rust functions.

### Phase 6: Generalize Evidence

Replace GM-only evidence records with profile evidence records:

```text
timestamp
adapter identity
protocol
vehicle identity
identity confidence
profile_id
capability_id
module/route
request service
request data
raw write text
raw read text
parsed response bytes
decoder_id
decoded value or DTCs
confidence/provenance
source fields such as TXD/RXF/RXD/MTH when available
manual profile confirmation flag
error classification
```

Normal live profile reads should be able to emit bounded evidence, not only
probe tools and blocked active tests.

### Phase 7: Recording and Replay v3

Add typed profile frames without breaking older recordings.

Proposed new frames:

- `FRAME_PROFILE_REQUEST`.
- `FRAME_PROFILE_RESPONSE`.
- `FRAME_PROFILE_VALUE`.
- `FRAME_PROFILE_DTC`.
- `FRAME_PASSIVE_BUS_FRAME`.
- `FRAME_ACTIVE_TEST_ATTEMPT`.

Rules:

- old recordings remain readable.
- unknown future profile frames are skippable.
- profile frames retain raw bytes where size allows.
- replay can reproduce decoded manufacturer values without hardware.

Do not keep overloading a scalar enhanced frame until it becomes impossible to
audit request/response behavior.

### Phase 8: Active Tests Under Profiles

Move active-test definitions under profile ownership.

VGT vane control stays locked until verified bytes are captured from Tech2,
EFILive DVT, HP Tuners controls, Snap-on functional tests, or an equivalent
trusted source.

An enabled active test requires:

- exact selected profile.
- verified command bytes.
- stationary-only gate when applicable.
- idle-only gate when applicable.
- voltage/coolant/RPM/speed preconditions.
- short timeout.
- hold-to-command behavior for manual controls.
- automatic cancel on timeout and disconnect.
- evidence for every attempt and every transport response.

### Phase 9: Add One Proof Profile

Before adding real Ford/Ram support, add a tiny read-only mock or fixture
profile that is not GM.

Purpose:

- prove the scheduler is profile-neutral.
- prove GM decoders cannot be used by non-GM profiles.
- prove UI tabs render from capabilities, not hardcoded GM assumptions.
- prove recording/replay handles non-GM profile frames.

Only after this should real additional manufacturers be added.

## UI Model

The UI should render categories from the selected profile and generic OBD
state.

Base tabs:

- Overview.
- Generic OBD.
- Powertrain.
- DTCs.
- Evidence.
- Active Tests.
- Raw.

Profile-provided tabs or sections:

- GM Class 2.
- Turbo/VGT.
- Fuel/Rail.
- Transmission.
- Body/Chassis modules.

Rules:

- Non-GM vehicles do not show GM-specific controls as applicable.
- "Chevy" is not a profile. It is part of a GM vehicle identity.
- Profile ids should look like `gm.gmt800.lly.class2`,
  `gm.gmlan.lmm.can`, `ford.powerstroke.6_0.can`, etc.
- UI labels come from capability metadata.
- UI never constructs request bytes.

## Tests Required Before New Manufacturer Support

Profile selection:

- LLY exact profile matches only the correct VIN/spec/protocol.
- LB7 does not match LLY.
- later GM CAN does not match GM Class 2 LLY.
- Ford/Ram mock does not match GM.
- missing VIN/spec falls back to generic only.
- corrupted VIN text does not produce a false exact match.
- multiple exact matches create ambiguity and block profile polling.
- ambiguity logs the colliding profile ids and match evidence.
- profile corpus test proves no profile returns exact without protocol plus a
  VIN-derived identity dimension.
- token invalidates on disconnect, identity change, and protocol change.

Request safety:

- no manufacturer-specific request executes without `SelectedProfile`.
- no request executes if capability id is not owned by the selected profile.
- stale selected-profile generation fails dispatcher validation.
- GUI cannot call a raw GM node helper in live mode.
- TUI and GUI planned requests are identical for the same selected profile.
- live code cannot import probe-only raw routed helpers.

Decoder isolation:

- Ford/mock profile cannot decode through GM registry.
- selector-based target execution does not call `find_lly_did` directly.
- DTC decoder is selected by profile service definition.

Policy isolation:

- LLY forced standard PIDs are not global.
- unsupported/no-data backoff is per profile service or signal.
- TCM signal routes use TCM module labels.

Evidence/replay:

- profile request/response evidence includes `profile_id` and `capability_id`.
- evidence records source fields such as TXD/RXF/RXD/MTH when available.
- evidence marks manual profile confirmation when used.
- old recordings replay.
- new profile frames replay.
- unknown future frames do not crash replay.

Active tests:

- locked tests do not send bytes.
- invalid values are rejected before transport.
- timeout/disconnect cancels enabled tests.
- every active-test attempt writes evidence.

## Risk Register

Duplicate protocol paths:

- Risk: TUI and GUI drift.
- Mitigation: central profile runtime and dispatcher used by both.

False-positive profile match:

- Risk: wrong vehicle receives wrong routed request.
- Mitigation: exact/partial/no-match model; framework-level match floor;
  cross-profile corpus tests; generic-only fallback.

Weak or corrupted VIN:

- Risk: flaky VIN reads leave the truck in generic-only mode or, worse, create
  a false exact match.
- Mitigation: VIN retry, identity confidence, corrupted-VIN rejection, visible
  manual confirmation path, and evidence marking when manual confirmation is
  used.

Stale selected profile:

- Risk: reconnecting to another vehicle keeps an old profile token alive.
- Mitigation: generation-bound `SelectedProfile`; invalidate on disconnect,
  adapter change, protocol change, VIN change, and spec change.

Probe API bypass:

- Risk: live code calls raw routed manufacturer helpers outside the dispatcher.
- Mitigation: probe-only API visibility or feature flag plus architectural
  import tests.

Decoder leakage:

- Risk: future profile decodes through LLY registry.
- Mitigation: `decoder_id` belongs to profile capability; no global LLY lookup.

Global poll quirks:

- Risk: LLY bitmap workaround affects other vehicles.
- Mitigation: standard PID overrides move to profile poll policy; generic-only
  fallback may show fewer values until a matching profile is selected.

Module identity leakage:

- Risk: ECM/TCM/BCM labels become wrong as support expands.
- Mitigation: module identity comes from route definition, never from a
  hardcoded display fallback.

Active-test safety:

- Risk: output controls run on the wrong vehicle or without cancel behavior.
- Mitigation: verified bytes, selected profile, preconditions, timeout, cancel,
  evidence, and locked default state.

Recording compatibility:

- Risk: new profile frames break old replay.
- Mitigation: versioned v3 frames with old readers preserved and unknown frame
  skipping.

Latency:

- Risk: profile scheduling adds allocations or work to each poll cycle.
- Mitigation: build schedules when discovery/profile state changes; poll cycles
  execute prebuilt request plans.

## Documentation Deliverables

Technical writer handoff:

1. `docs/architecture/manufacturer-profiles.md`
   - high-level architecture and invariants.
2. `docs/contributing/adding-a-diagnostic-profile.md`
   - how to add a profile safely.
3. `docs/diagnostics/evidence-and-signal-promotion.md`
   - confidence levels and evidence requirements.
4. `docs/diagnostics/active-tests-safety.md`
   - bidirectional controls, gates, and evidence.
5. `docs/diagnostics/gm-gmt800-lly-class2-status.md`
   - current support matrix and unresolved evidence items.

## Owl Review

Verdict: the current LLY gate is a necessary emergency fix, not the final
architecture.

Critical findings:

- Profile matching strictness is the real safety hinge. The sealed token only
  helps if the resolver cannot mint it from weak match criteria.
- Flaky VIN reads are expected on this hardware path. A malformed VIN must not
  either strand the user silently or create a false exact match.
- Selected profile tokens need a context generation. Otherwise a disconnect or
  identity change can leave stale authority in memory.
- Profile gating is not yet enforced at the lowest send boundary. A future
  caller can still bypass the current boolean gate if a raw routed helper is
  reused incorrectly.
- TUI and GUI have separate GM paths. That is the main regression vector.
- Selector-based enhanced decoding still has LLY lookup assumptions.
- The forced standard PID workaround is global even though it is justified by a
  specific LLY/J1850 behavior.
- Module labels leak. ECM is currently too easy to assume for signals that may
  belong to TCM or another module.
- Active-test scaffolding is safely blocked, but still GM/ECM shaped.
- UI text and raw snapshots contain static GM assumptions.
- Evidence is useful but still too GM-specific and incomplete for replayable
  multi-profile proof.
- Vendor source fields such as TXD/RXF/RXD/MTH must remain inspectable after
  decoders move behind profile abstractions.

Required correction:

```text
Make selected profile ownership the only path to manufacturer-specific
requests.
```

Do not add broad Ford, Chrysler/Ram, or later GM support until this extraction
is complete. Adding another manufacturer before the profile runtime exists
would multiply the exact duplication risk we are trying to remove.

## Implementation Checkpoint

The first implementation milestone is not "add Ford" or "finish GM." It is:

```text
GM LLY runs through the neutral profile runtime with no behavior regression.
```

At that point the codebase has an established pattern:

- generic OBD always works.
- exact profile unlocks manufacturer-specific features.
- UI is profile-driven.
- recording/evidence preserve raw profile behavior.
- active tests are profile-owned and locked by default.

That is the baseline required before expanding manufacturer support.
