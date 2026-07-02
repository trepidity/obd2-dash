# OBD2 GUI Capability-Driven UI Spec

Status: draft implementation spec
Date: 2026-07-01
Scope: migrate `apps/obd2-gui` from LLY-shaped panels to vehicle capability rendering
Companions:
- `docs/plans/2026-06-29-manufacturer-profile-migration-plan.md`
- `docs/plans/2026-06-29-manufacturer-profile-implementation-waves.md`
- `docs/plans/2026-06-29-module-support-architecture.md`

## Purpose

Make the GUI render the selected vehicle's diagnostic capabilities instead of
rendering a fixed 2004.5 LLY Duramax dashboard shape.

The LLY should continue to get rich VGT, fuel rail, injector balance, GM Class 2
DTC, and active-test evidence views. Other vehicles should render only the
signals, diagnostics, modules, and active-test states that their selected
profile or generic OBD-II support actually exposes.

The source of truth is the existing Rust profile model, not a new TypeScript
capability system.

## Problem

The current GUI contract is still LLY-shaped.

Current hard fields in `apps/obd2-gui/src/types.ts` include:

- `vgt`
- `fuel_rail`
- `cylinders`
- `desired_map_psi`
- `barometric_psi`
- `boost_psi`
- `maf_g_s`
- `active_tests.vgt_vane`

Current hard assembly in `apps/obd2-gui/src-tauri/src/main.rs` manually reads
LLY and diesel-oriented values field by field:

- selected LLY profile check
- GM barometer DID
- GM desired MAP DID
- GM actual and desired fuel rail DIDs
- VGT actual and desired DIDs
- injector balance DIDs
- GM Class 2 DTC scan
- VGT active-test snapshot

Current UI navigation in `apps/obd2-gui/src/App.tsx` is fixed around:

- Overview
- Air / Boost
- Fuel / VGT
- Active Tests
- Diagnostics
- Thermal / System
- Replay
- Raw
- Settings

That shape is correct for the current truck, but it cannot correctly represent
generic OBD-only vehicles, gas GM vehicles, newer CAN GM vehicles, Ford, Ram,
or any vehicle without turbo, diesel rail, injector balance, or GM Class 2
services.

## Existing Source Of Truth

Do not invent a second vocabulary. The Rust profile model already owns the
capability concepts:

- `SignalDefinition`
- `SignalCategory`
- `Confidence`
- `Provenance`
- `FailurePolicy`
- `EvidencePolicy`
- `DtcServiceDefinition`
- `ActiveTestDefinition`
- `ActiveCommandProfile`
- `SafetyClass`
- `DiagnosticProfile`
- `CapabilityId` (runtime capability-ownership token; informs active-test
  locking, not projected as a signal/DTO field)

The GUI DTO must be a serde projection of these definitions plus runtime decode
state, not a parallel model with different names.

## Design Decision

The GUI should render from a capability graph:

```text
ProfileRegistry / SelectedProfile
  -> scheduler/runtime planned requests
  -> Tauri snapshot projection
  -> SignalSnapshot[] / CapabilitySection[] / ActiveTestSnapshot[]
  -> GUI capability rail and generic renderers
```

The profile owns what exists. The Tauri layer projects profile capabilities and
runtime state. The GUI owns layout and widget rendering.

The GUI must not know LLY DIDs, GM node IDs, Class 2 headers, or profile-owned
request bytes.

## Scope

In scope:

- Add capability-driven DTO fields beside the current LLY-shaped snapshot.
- Add a signal-to-widget composition model.
- Build capability sections from `SignalCategory`, DTC services, active tests,
  and utility views.
- Render the left rail from capabilities.
- Render generic scalar, paired, table, derived, diagnostics, and active-test
  widgets.
- Preserve current LLY behavior until parity tests prove replacement.
- Add non-LLY fixture snapshots that prove the GUI is not tied to the LLY.
- Delete legacy LLY-shaped GUI fields only after parity gates pass.

Out of scope:

- Redesigning the Rust profile architecture.
- Adding a new OEM support package.
- Enabling VGT vane control or any unverified active command.
- Replacing the TUI.
- Replacing the existing profile selection gate.
- Moving protocol/adapter behavior into the GUI.

## Requirements

### Capability Requirements

1. DTO names and values must project the Rust profile model.
2. `SignalCategory` drives normal rail sections.
3. `SignalDefinition.confidence` controls whether a signal appears as normal,
   candidate/discovery, rejected/debug, or hidden.
4. `FailurePolicy::DoNotPoll` cannot enter normal UI.
5. `Confidence::Rejected` cannot enter normal UI.
6. `Confidence::Candidate` renders under Discovery unless explicitly promoted
   by a later, reviewed policy.
7. Unsupported capability and supported-but-waiting value are separate states.
8. Evidence metadata remains available for every profile-owned signal.

### Render Requirements

1. The rail is built from visible capability sections.
2. Empty capability sections are not shown.
3. Utility sections remain available:
   - Replay
   - Raw
   - Settings
   - Evidence or Discovery when useful
4. The GUI must retain semantic widgets currently useful on the LLY:
   - VGT actual / desired / error
   - fuel rail actual / desired / delta
   - injector balance table
   - scalar gauges
   - derived values
   - module scan / DTC coverage
   - locked active-test evidence card
5. A generic OBD-only vehicle must render without Turbo, diesel rail, injector,
   or active-test sections.

### Active-Test Requirements

1. Active tests are projected from `ActiveTestDefinition`.
2. A test is actionable only when backed by `ActiveCommandProfile::Verified`
   **and** its resolved target module is not `ModuleSafetyClass::WriteForbidden`.
3. `ActiveCommandProfile::Locked` renders as a disabled evidence card.
4. Forbidden write targets must not surface executable controls.
   `ActiveTestDefinition` carries no module directly — the target module is
   reachable only through `ActiveCommandProfile::Verified(ProfileRequestDefinition).route`.
   A `Locked` test therefore has no route and is inherently non-actionable; a
   `Verified` test whose resolved module is `WriteForbidden` is downgraded to a
   disabled/forbidden card with no command payload in the DTO.
5. The UI must not be able to construct a command payload for a locked test.
6. Verified tests must include:
   - precondition status
   - safety class
   - timeout
   - cancel command when applicable
   - evidence path/status

### Regression Requirements

1. The old LLY-shaped fields remain until generic parity is proven.
2. The generic signal graph must reproduce old LLY field values exactly, compared
   through the same unit formatter: the parity test formats both the legacy field
   and the generic signal `value` to their display string and asserts string
   equality, not a raw-float epsilon. "Display precision" is enforced by the
   shared formatter, not a hand-picked tolerance.
3. Non-LLY fixture shapes must pass GUI rendering tests.
4. Static grep gates must prove production GUI code no longer depends on LLY
   fields before deletion.

## DTO Contract

Add these fields beside the existing `DiagnosticSnapshot` fields first:

```ts
export type SignalCategory =
  | "Powertrain"
  | "Turbo"
  | "Fuel"
  | "Transmission"
  | "Body"
  | "Chassis"
  | "Emissions"
  | "Other";

export type Confidence =
  | "Candidate"
  | "LiveObserved"
  | "Community"
  | "Verified"
  | "Rejected";

export type SignalRuntimeState =
  | "ok"
  | "waiting"
  | "cached"
  | "unsupported"
  | "error";

export type SignalComposition =
  | { kind: "scalar" }
  | {
      kind: "pair";
      group_key: string;
      role: "actual" | "desired" | "error" | "delta";
    }
  | {
      kind: "table_row";
      table_key: string;
      row_index: number;
      row_label: string;
    }
  | {
      kind: "derived";
      group_key: string;
      formula_key: string;
      input_keys: string[];
    };

export interface SignalSnapshot {
  key: string;
  label: string;
  category: SignalCategory;
  module: string;
  unit: string;
  value: number | null;
  state: SignalRuntimeState;
  confidence: Confidence;
  provenance: string[];
  evidence_policy: string;
  failure_policy: string;
  preferred_over: string | null;
  evidence: SignalEvidence | null;
  composition: SignalComposition;
}

export interface CapabilitySection {
  id: string;
  category:
    | SignalCategory
    | "Discovery"
    | "Diagnostics"
    | "ActiveTests"
    | "Evidence"
    | "Replay"
    | "Raw"
    | "Settings";
  label: string;
  signal_keys: string[];
  active_test_keys: string[];
  diagnostic_service_keys: string[];
  visible: boolean;
}

export interface ActiveTestSnapshotV2 {
  key: string;
  label: string;
  safety_class: string;
  command_profile: "Locked" | "Verified";
  actionable: boolean;
  preconditions: ActiveTestPrecondition[];
  last_result: ActiveTestResult | null;
}
```

Notes:

- TypeScript string unions must match the Rust enum labels exactly enough that
  serde projection is mechanical and testable.
- `SignalEvidence` already exists and should be reused.
- The compatibility snapshot may temporarily duplicate values between legacy
  fields and `signals[]`.
- `SignalRuntimeState` is derived in the Wave 3 projection, not stored on the
  profile: `ok` = decoded this cycle; `waiting` = requested, no reply yet;
  `cached` = last-good value shown after a missed poll; `unsupported` = not in
  the selected profile or marked absent by a negative response; `error` =
  decode/transport failure. It is orthogonal to `Confidence` (trust in the
  source): a signal can be `Verified` yet `waiting`.

## Rust Composition Model

The current Rust model has grouping (`SignalCategory`) but not widget
composition. Add composition as Rust-owned profile metadata.

Preferred shape:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalComposition {
    Scalar,
    Pair {
        group_key: &'static str,
        role: PairRole,
    },
    TableRow {
        table_key: &'static str,
        row_index: u8,
        row_label: &'static str,
    },
    Derived {
        group_key: &'static str,
        formula_key: &'static str,
        input_keys: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairRole {
    Actual,
    Desired,
    Error,
    Delta,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalDisplayDefinition {
    pub signal_key: &'static str,
    pub composition: SignalComposition,
    pub priority: u16,
}
```

Add to `DiagnosticProfile` as an additive default method:

```rust
fn signal_display(&self) -> &[SignalDisplayDefinition] {
    &[]
}
```

Reason:

- Avoid a TypeScript-only semantic map.
- Avoid making every existing `SignalDefinition` initializer change at once.
- Let fixture and LLY profiles opt in incrementally.
- Keep grouping and composition orthogonal.

If implementation proves simpler, composition may move directly into
`SignalDefinition`, but that increases migration churn and must be justified.

The Wave 3 projection joins each `SignalDefinition` with its optional
`SignalDisplayDefinition` by `signal_key`; a signal with no display entry
projects as `composition: scalar`. That join is what lets composition roll out
incrementally without editing every existing signal initializer at once.

## Composition Semantics

Composition is semantic, not styling.

Examples:

- `Scalar`: MAP, MAF, coolant, voltage.
- `Pair`: VGT actual/desired/error.
- `Pair`: fuel rail actual/desired/delta.
- `TableRow`: injector balance cylinder 1 through 8.
- `Derived`: boost from MAP and barometer.
- `Derived`: rail delta from actual and desired rail pressure.

The legacy `temperatures` struct (coolant, intake air, oil, transmission,
ambient) decomposes into individual `Scalar` signals: coolant and intake-air are
standard PIDs; oil/trans/ambient are profile/extended signals. `snapshot.temperatures`
is therefore part of the legacy deletion set and the parity/grep gates below.

The GUI chooses visual treatment for each composition kind, but the profile
decides signal relationships.

`Derived` values are computed in Rust and emitted with `value` already
populated; `formula_key` and `input_keys` are display provenance only — the GUI
never evaluates a formula. This preserves the rule that the GUI knows no
profile-owned math: boost = f(MAP, baro) and rail delta are computed in the
Tauri projection, exactly as `main.rs` computes them today.

## Capability Section Rules

Build sections from `SignalCategory` plus fixed utility sections.

Normal sections:

- `Powertrain`
- `Turbo`
- `Fuel`
- `Transmission`
- `Body`
- `Chassis`
- `Emissions`
- `Other`

Special sections:

- `Discovery`: candidate or unverified signals.
- `Diagnostics`: DTC services and module scan coverage.
- `ActiveTests`: active tests that may be shown.
- `Evidence`: raw evidence/source confidence/debug.
- `Replay`, `Raw`, `Settings`: utility sections.

Visibility rules:

```text
normal section visible if at least one non-rejected, non-DoNotPoll,
non-candidate signal or service belongs to it

Discovery visible if candidate signals exist

Diagnostics visible if standard or profile DTC services exist

ActiveTests visible if at least one active test exists
  (locked and write-forbidden tests still render, as disabled cards with a
   reason, never hidden — a safety-locked capability shown disabled is safer
   and more honest than one silently omitted)

Evidence visible if evidence records exist or debug mode is enabled
```

Candidate rule:

`Confidence::Candidate` does not render as normal operational data. Desired MAP
`0x1542` stays Discovery until a reviewed policy promotes it.

## Execution Board

| Wave | Name | Primary files | Expected output | Acceptance gate |
| --- | --- | --- | --- | --- |
| 0 | Baseline and fixtures | `apps/obd2-gui/tests`, mock snapshots, corpus fixtures | Current LLY GUI behavior frozen; non-LLY fixture shapes added | Existing GUI tests pass; fixture snapshots cover generic OBD-only, gas/no-turbo, and trans-capable profiles |
| 1 | DTO projection | `apps/obd2-gui/src/types.ts`, `apps/obd2-gui/src-tauri/src/main.rs` | `signals[]`, `capability_sections[]`, `active_tests_v2[]` added beside old fields | No visible UI change; `npm run build`; `cargo test -p obd2-gui` |
| 2 | Rust display composition | `crates/obd2-dash/src/profiles/model.rs`, LLY profile, fixture profile | `SignalDisplayDefinition` and profile display metadata exist | LLY VGT, fuel rail, injector balance, boost, and scalar signals have composition metadata |
| 3 | Generic snapshot builder | `apps/obd2-gui/src-tauri/src/main.rs` | Generic `SignalSnapshot[]` populated from profile signals and standard PIDs | Legacy fields and generic fields match for LLY values; runtime states distinguish ok/waiting/cached/unsupported/error |
| 4 | Capability rail | `apps/obd2-gui/src/App.tsx` | Left rail renders from `CapabilitySection[]` | No empty Turbo/Fuel/Transmission tabs; candidate data appears in Discovery; utilities remain |
| 5 | Generic renderers | `apps/obd2-gui/src/App.tsx` | Scalar, pair, table, derived, diagnostics, and active-test cards render from generic DTOs | LLY visual parity retained; generic OBD fixture does not show diesel/turbo controls |
| 6 | Parity gate | Rust tests, Playwright tests | Automated proof that generic graph equals old LLY fields | VGT, rail, injectors, MAP/baro/desired MAP, MAF, temps, DTCs match old fields within display precision |
| 7 | Flip GUI | `apps/obd2-gui/src/App.tsx` | Production GUI consumes generic capability arrays | Static grep gate (see Regression And Safety Gates) shows no production GUI dependency on any legacy field except compatibility mocks |
| 8 | Remove legacy shape | `types.ts`, `main.rs`, tests | LLY-specific snapshot fields deleted | All tests pass; no profile-specific GUI assumptions remain |

## Expected Outcome

After Wave 8:

- The GUI is vehicle-capability-driven.
- The LLY still displays rich diesel/turbo views.
- Other vehicles display only their supported capability sections.
- Missing support does not appear as fake `--` gauges.
- Candidate data is visibly quarantined.
- Locked active tests cannot become executable from UI code.
- New profiles can add signals and compositions without editing hard-coded GUI
  tabs.

## Regression And Safety Gates

Required checks during implementation:

```text
npm run build                         # apps/obd2-gui
npx playwright test tests/dashboard.spec.ts
cargo test -p obd2-gui
cargo test -p obd2-dash
```

Additional gates to add:

- LLY parity test over generic signal graph.
- Fixture GUI tests for:
  - generic OBD-only vehicle
  - gas vehicle with no turbo/diesel rail
  - transmission-capable vehicle
- Static grep gate before legacy deletion:

```text
rg 'snapshot\.(vgt|fuel_rail|cylinders|map_psi|desired_map_psi|barometric_psi|boost_psi|maf_g_s|temperatures)\b|vgt_vane' apps/obd2-gui/src

# Covers all ten legacy fields App.tsx reads today. `active_tests` is caught via
# the `vgt_vane` token (its field name may survive as a generic array, so the
# new active-test field must be named distinctly, e.g. active_tests_v2).
```

Expected result after Wave 7:

- no production GUI references
- only compatibility tests or removal notes

## Requirements Checklist

- [ ] Uses existing Rust profile vocabulary.
- [ ] Adds no TypeScript-only capability truth.
- [ ] Separates grouping from widget composition.
- [ ] Keeps candidate signals out of normal operational tabs.
- [ ] Keeps rejected and DoNotPoll signals out of normal UI.
- [ ] Renders locked active tests as disabled evidence only.
- [ ] Preserves LLY display until parity is proven.
- [ ] Proves non-LLY rendering with fixtures.
- [ ] Deletes old snapshot shape last.

## Risks

Candidate values displayed as facts:

- Mitigation: hard section gate by `Confidence`.

Loss of paired/table views:

- Mitigation: explicit Rust-owned composition model.

Snapshot allocation growth:

- Mitigation: reserve vectors from known profile signal/test counts; keep raw
  evidence bounded; avoid large string churn inside render loops.

Active-test safety regression:

- Mitigation: actionability is derived only from
  `ActiveCommandProfile::Verified`; locked tests have no command payload in the
  DTO.

Duplicated behavior during migration:

- Mitigation: old fields and new graph are populated from the same decoded
  values during the compatibility window.

## Open Questions

1. Should SAE standard PIDs be represented as a generic profile or projected by
   the Tauri adapter first?
2. Should `Provenance::SaeStandard` be added, or should standard PIDs carry a
   separate source label outside profile provenance?
   Note: a standard-PID to evidence path already exists (`standard_signal_evidence`
   in `main.rs`), so Q1/Q2 are modeling decisions, not missing capability — Wave 3
   can populate standard-PID signals through that path while provenance is settled.
3. Should composition stay in `SignalDisplayDefinition`, or move into
   `SignalDefinition` after the migration settles?
4. What exact policy promotes a `Confidence::Candidate` signal into a normal
   section once its confidence advances past `Candidate` (e.g. to `LiveObserved`
   or `Verified`)? `Candidate` and `LiveObserved` are distinct variants; a signal
   leaves Discovery when it is no longer `Candidate`.
5. Should Discovery be always visible when candidates exist, or hidden behind an
   advanced/debug setting?

## Definition Of Done

The migration is complete when:

- `DiagnosticSnapshot` no longer contains LLY-specific top-level fields.
- `App.tsx` renders operational views from capability sections and signal
  composition.
- LLY parity tests pass.
- Non-LLY fixture rendering tests pass.
- Locked active tests cannot be executed from GUI state.
- Adding a new profile with new categories/signals requires profile metadata
  and tests, not hard-coded GUI tab edits.

## Review Findings (resolved 2026-07-01)

Grounded against the tree during review; all resolved inline above.

- **F1 — Incomplete legacy grep gate (high).** The Wave 7/gate pattern matched
  only `vgt`, `fuel_rail`, `cylinders`, `vgt_vane` — 4 of the 10 legacy fields
  `App.tsx` actually reads. It would report "no legacy dependency" while
  `map_psi`, `desired_map_psi`, `barometric_psi`, `boost_psi`, `maf_g_s`, and
  `temperatures` were still live, so Wave 8 deletion would break the GUI behind a
  green gate. Pattern expanded to all ten fields.
- **F2 — "non-forbidden active test" was undefined (safety).** `ActiveTestDefinition`
  has no module field; forbidden-ness lives on the module
  (`ModuleSafetyClass::WriteForbidden`), reachable only through a `Verified`
  command's route. Defined the resolution path: locked tests are inherently
  non-actionable, forbidden verified tests downgrade to disabled cards, and
  visibility no longer hides them.
- **F3 — Derived-value ownership (correctness).** Clarified that `Derived` values
  are Rust-computed and emitted with `value` set; the GUI never evaluates
  `formula_key`, preserving "GUI knows no profile math."
- **F4 — `temperatures` unaddressed (completeness).** Added its decomposition into
  scalar signals and its inclusion in the deletion / parity / grep set.
- **F5 — Parity tolerance (rigor).** Replaced "within display precision" with a
  concrete same-formatter string-equality comparison.
- **F6 — `SignalDisplayDefinition` join/default (completeness).** Specified the
  Wave 3 join by `signal_key` and the `scalar` default for signals with no
  display entry, and defined `SignalRuntimeState` derivation (orthogonal to
  `Confidence`).
- **F7 — Minor cleanups.** Annotated `CapabilityId` as runtime-only (not a DTO
  field); fixed the `Candidate`/`LiveObserved` contradiction in Open Question 4;
  noted the existing `standard_signal_evidence` path so Q1/Q2 don't block Wave 3.

Verified sound, no change needed: `cargo test -p obd2-gui` (package name
`obd2-gui` and workspace membership confirmed), `apps/obd2-gui/tests/dashboard.spec.ts`
exists, `rg` is on PATH, and the DTO's `SignalCategory`/`Confidence` unions match
`model.rs` variant-for-variant.
