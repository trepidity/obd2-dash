# OBD2 GUI Design-System Audit

Prepared: 2026-07-03  
Scope: Phase A audit plus Phase B design direction for `apps/obd2-gui`.  
Branch: `design/obd2-dashboard-system`

This is a checkpoint artifact. No app components were changed in this pass.

## Verdict

The layout system mostly fits this app if we take the desktop/embedded "density wins" path and treat the reference components as patterns, not drop-in code. The current GUI already moved in the right direction: category rail navigation is capability-driven, primary status tiles stay visible above the rail, static module/cylinder data uses native tables, and the UI now derives sections from vehicle capabilities instead of LLY-only fields.

The system is not complete for this domain yet. It needs first-class signal trust metadata, runtime-state tokens beyond `ok/warn/crit`, and a precise dense telemetry table/card pattern for actual/desired/delta signals. The current app also violates one hard performance rule: `backdrop-blur` is applied to the sticky header.

## Audit Table

| Area | Current App Evidence | System Result | Finding |
|---|---|---|---|
| IA depth | `CategoryRail` is the root, with one active `tabpanel`; sections render in-place. | Mostly agrees | The app stays within two levels. Session menu is an action overlay, not navigation. |
| Desktop density | `StatusStrip` always shows voltage, RPM, speed, source, DTCs, ECUs, MIL, Record. | Partly agrees | Critical telemetry is mostly visible, but coolant and active DTC severity are not always in the first dense strip. The overview also shows only the first three capability sections. |
| Capability scaling | `capability_sections`, `signals`, and `active_tests_v2` drive rail tabs and content. | Agrees | This is better than the reference components. Preserve this model. |
| State vocabulary | `StateKind` has `ok/warn/crit/muted`; `SignalRuntimeState` has `ok/waiting/cached/unsupported/error`. `runtimeTone()` collapses waiting/cached/Candidate into `warn`, unsupported into `muted`. | Violates / incomplete | The design-system `ok/warn/crit` map is too small. The app has partial mapping, but it does not visually distinguish stale cached data from pending data from candidate confidence. |
| Palette | Base page is `#090b0d`, text `#f2f5f2`, panels use Tailwind zinc classes and custom hex backgrounds. | Partly agrees | Do not introduce another palette. Keep the app base and map system tokens into it via `@theme` or CSS variables. |
| Typography | Inter is defined globally. `font-mono` is used for request bytes/formula labels, but no mono stack or tabular numeric policy exists. | Violates | Add a mono/tabular numeric rule. Otherwise fuel rail, MAP, injector, and raw-byte columns will not align deterministically across platforms. |
| Spacing | Most classes use Tailwind multiples of 2/3/4; several controls are `h-8` or `h-9`. | Mostly agrees for desktop | Desktop controls can be denser than mobile, but any touch-oriented target must meet 48px. Add explicit "desktop compact" vs "touch" variants to the system. |
| Static tables | `ModuleScanPanel` and `GenericTablePanel` use native `<table>`. | Agrees | Good. Keep native tables for injector balance and module scan. |
| ARIA tabs | Rail uses `role="tablist"`, `aria-controls`, `aria-selected`, roving `tabIndex`, arrow/Home/End keys, and `tabpanel`. | Agrees | This now matches the expected tab pattern. |
| Icon-only controls | Most icon controls include text; hidden file input has `aria-label`. | Mostly agrees | Session menu button has label. No immediate blocker found. |
| Progressive disclosure | Raw snapshot is a tab and session controls are a menu. No primary telemetry is hidden in accordions. | Agrees | Use collapsible cards only for evidence/provenance and raw details, not primary values. |
| Performance | Header uses `backdrop-blur`. | Violates | The design system explicitly forbids backdrop filters. Replace with an opaque or near-opaque surface in Phase C. |
| Contrast | Measured key pairs against app surfaces. `zinc-400` on app bg is 7.69:1; `zinc-500` on panel `#18181b` is 3.67:1. | Partly agrees | `zinc-400` can carry labels. `zinc-500` must remain decorative metadata only. Current app uses `zinc-500` for session detail, formula keys, and module/confidence metadata. That is acceptable only if not meaning-critical. |
| Test coverage | `tests/dashboard.spec.ts` still expects separate `Record`/`Replay` buttons. | Violates test freshness | Confirmed by running Playwright: 1 stale-test failure at `getByRole('button', { name: 'Record' })`; three capability fixture tests pass. |

## Reconciliation Decisions

### 1. State Vocabulary

Decision: extend the design system to model both source trust and runtime state. Do not force everything into `ok/warn/crit`.

Required token mapping:

| Model Value | Visual Treatment | Meaning |
|---|---|---|
| `ok` | high-contrast normal value, semantic green only when "healthy" is intended | Fresh usable value. |
| `waiting` | muted value placeholder, no alarm color | Signal is expected but not populated yet. |
| `cached` | normal value plus amber `cached` badge | Last known value retained; not a live fault by itself. |
| `unsupported` | muted row/card, low-emphasis `unsupported` label | Vehicle/profile does not expose it. |
| `error` | red value/badge with visible error context | Read or decode failed. |
| `Candidate` confidence | amber outline/source badge, not necessarily amber value | Provenance is not verified; value can still be numerically normal. |
| `Rejected` / `DoNotPoll` | hidden from operational sections or shown only in Discovery/Evidence | Must not appear as normal telemetry. |

Current implementation is close but too coarse: `runtimeTone()` maps `waiting`, `cached`, and `Candidate` all to `warn`. Phase C should split these.

### 2. Palette

Decision: keep the existing app palette and map the design system onto it. Do not adopt a separate zinc-only surface language.

Rationale: the app is already dark, dense, and readable with `#090b0d` as the stable base. Replacing it app-wide to pure Tailwind zinc would churn existing screenshots and does not improve diagnostics. The correct implementation is a CSS-first Tailwind v4 theme layer in `styles.css`:

- app background: `#090b0d`
- app surface: `#111416` / `#101316`
- panel surface: zinc-derived but named explicitly
- primary text: `#f2f5f2`
- semantic status colors: emerald/amber/red/cyan, bounded by contrast checks

### 3. Density Path

Decision: lead with a dense telemetry surface. Use collapsible cards only for evidence, raw packet detail, history, and troubleshooting notes.

The overview should not be a marketing-style card grid. It should be a dense but readable status board:

- top status strip: connection, voltage, RPM, speed, coolant, DTC/MIL
- dense capability telemetry below: powertrain/turbo/fuel/temperatures
- DTC and alerts stay visible on the right or below depending on width

### 4. Monospace / Tabular Figures

Decision: add a deterministic numeric policy before building new components.

Minimum Phase C change:

- add a mono stack in `styles.css`
- add `font-variant-numeric: tabular-nums` for telemetry values
- use tabular figures for all dense value cells, even when using Inter

This avoids hidden platform drift in injector, rail, MAP, and raw-byte columns.

### 5. Provenance / Confidence

Decision: add a compact trust lane to dense signal components. This is the biggest design-system gap.

Signals need to expose:

- `confidence`
- `provenance[]`
- `module`
- `request` / source fields when available
- `preferred_over`
- `composition` role

For dense cards, this should not be full prose. The design should use a small source badge row and a disclosure panel for raw evidence. Example: `ECM / Community / ScanGauge` visible; raw request/response inside evidence disclosure.

## Phase B Design Direction

Build these three representative surfaces in Phase C, as new components under `src/components/`, using existing `types.ts` shapes and `mockData.ts` fixtures.

### Surface 1: Dense Telemetry Board

Purpose: apply the desktop density path to real `SignalSnapshot[]`.

Input:

- `DiagnosticSnapshot`
- `SignalSnapshot[]`
- `CapabilitySection[]`
- `UnitMode`

Behavior:

- Always show voltage, RPM, speed, coolant, MAP, MAF, DTC/MIL summary when present.
- Render pair groups as actual/desired/error or actual/desired/delta rows.
- Render table-row groups as true tables.
- Suppress rejected/unowned signals from operational view.
- Show Candidate signals in a Discovery band, not as verified telemetry.

Expected outcome:

- Overview becomes a dense dashboard, not a stack of generic cards.
- LLY, generic OBD, gas/no-turbo, and transmission fixtures all render without diesel-only leftovers.

### Surface 2: Evidence Disclosure Card

Purpose: apply the disclosure pattern to secondary diagnostic details.

Input:

- one `SignalSnapshot`
- optional `SignalEvidence`

Behavior:

- collapsed view shows label, value, runtime state, confidence, module
- expanded view lazily mounts provenance, request, response, source fields, notes
- use `aria-expanded`
- no hidden primary telemetry inside the disclosure

Expected outcome:

- Provenance is available without bloating the main telemetry view.
- Candidate desired MAP/barometer can show why they are provisional.

### Surface 3: DTC / Module Diagnostic Table

Purpose: make DTC and module scan state dense and inspectable.

Input:

- `DtcSnapshot[]`
- `ModuleScan[]`
- diagnostic services when available

Behavior:

- use native `<table>` for module scan
- distinguish `empty`, `unsup`, `no data`, `probe`, `error`, and `N dtc`
- keep active codes and alert count visible
- avoid `role="grid"`

Expected outcome:

- "No codes" and "not scanned / unsupported / no data" cannot visually collapse into one state.

## Concrete Execution Board

| Step | Work | Files | Gate |
|---|---|---|---|
| C1 | Add theme/numeric primitives: app tokens, mono stack, tabular value utility, remove `backdrop-blur`. | `src/styles.css`, `src/App.tsx` | `npm run build`; no `backdrop` grep hit in `src`. |
| C2 | Add status/trust mapping helpers. | `src/components/diagnosticTokens.ts` | Unit-free TypeScript compile; states cover `StateKind`, `SignalRuntimeState`, `Confidence`. |
| C3 | Build dense telemetry components. | `src/components/TelemetryBoard.tsx` | LLY/default and generic fixtures render with no hard-coded LLY field access. |
| C4 | Build evidence disclosure component. | `src/components/EvidenceDisclosure.tsx` | Primary value visible collapsed; evidence lazily mounted expanded. |
| C5 | Build DTC/module table component. | `src/components/DiagnosticTables.tsx` | Native tables; no `role="grid"`. |
| C6 | Wire into existing tabs minimally. | `src/App.tsx` | Existing rail/session behavior preserved. |
| C7 | Update Playwright for Session menu. | `tests/dashboard.spec.ts` | Full Playwright suite passes. |
| C8 | Capture screenshots and contrast report. | `docs/design-system/*.png`, `REVIEW.md` | `npm run build`, screenshots saved, contrast numbers reported. |

## Verification So Far

Commands run during audit:

```sh
npx playwright test tests/dashboard.spec.ts --reporter=line
```

Result:

- 3 passed
- 1 failed due to stale test path expecting old `Record` button
- failure location: `tests/dashboard.spec.ts:82`
- current UI has `Session: Live` menu instead of separate `Record` / `Replay` buttons

Contrast spot checks computed against known current colors:

| Pair | Ratio | Result |
|---|---:|---|
| `zinc-400` on `#090b0d` | 7.69:1 | Pass for labels |
| `zinc-500` on `#090b0d` | 4.08:1 | Metadata only |
| `zinc-400` on `#18181b` | 6.91:1 | Pass for labels |
| `zinc-500` on `#18181b` | 3.67:1 | Metadata only |
| `cyan-200` on `#090b0d` | 15.79:1 | Pass |
| `emerald-300` on `#090b0d` | 12.93:1 | Pass |
| `amber-300` on `#090b0d` | 13.67:1 | Pass |
| `red-400` on `#090b0d` | 7.13:1 | Pass |

These are audit measurements, not a substitute for Phase D rendered contrast checks.

## Back-Port Recommendations For Canonical Design System

1. Add a diagnostic runtime state taxonomy: `ok`, `waiting`, `cached`, `unsupported`, `error`.
2. Add a separate confidence/provenance lane: `Candidate`, `LiveObserved`, `Community`, `Verified`, `Rejected`.
3. Add a pair/table composition pattern for actual/desired/error and cylinder/injector tables.
4. Add a desktop compact-control exception distinct from mobile 48px touch targets.
5. Require tabular numeric alignment for telemetry even when the app font is not monospace.
6. Add guidance that `warn` color must not mean "unverified source" and "active vehicle fault" at the same time.

## Stop Point

This completes Phase A and proposes Phase B. Phase C build should not start until the design direction above is approved.
