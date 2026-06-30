# GM-Specific Diagnostics Integration Spec

Status: implemented foundation; live evidence gates remain
Date: 2026-06-29
Primary target: 2004.5 GMC Sierra 2500HD LLY Duramax, J1850 VPW / GM Class 2

## Purpose

Build GM-specific diagnostics as a first-class path in `obd2-dash`, not as
GUI-local probes or one-off examples.

The system must read and display the GM enhanced data needed for LLY diesel
diagnostics, retrieve module-specific GM Class 2 DTCs, preserve enough raw
protocol evidence to debug incorrect assumptions, and keep the TUI, GUI,
recording, replay, and probes on the same decoding rules.

## Current State

Implemented pieces with reviewable code support:

- J1850 VPW physical routing works through `RoutedRequest`.
- GM Class 2 request headers use `[0x6C, target_node, 0xF1]`.
- Standard Mode 01 direct reads are forced for dashboard-critical PIDs because
  the LLY PID support bitmap is unreliable.
- GM Class 2 `$19` decoder exists in `crates/obd2-dash/src/gm_class2.rs`.
  The decoder strips the `0x59` positive-response byte, parses
  `[dtc_hi, dtc_lo, gm_status]` triplets, decodes the two-byte DTC using the
  existing J2012 path, preserves the raw GM status byte, and does not reuse UDS
  status-byte semantics.
- A shared GM enhanced registry exists in
  `crates/obd2-dash/src/gm_enhanced.rs`. It carries TXD, RXF, RXD, RXD width,
  raw MTH, decoded transform, display unit, confidence, provenance, cadence,
  failure policy, and rejected-DID records.
- The registry includes:
  - VGT desired/actual `0x1540`/`0x1543`.
  - injector balance `0x162F` through `0x1636`.
  - desired/actual fuel rail `0x163D`/`0x163E`.
  - barometric pressure `0x1251`.
  - desired MAP candidate `0x1542`.
  - oil pressure `0x1470`.
  - TCM transmission temperature `0x1940`.
  - injector pulse width `0x1193` through `0x119A`.
- The evidence writer exists in `crates/obd2-dash/src/gm_evidence.rs`.
  GM probes and the drive logger now write JSONL evidence under
  `raw-captures/` when run.
- Main TUI/session polling appends registry-backed GM enhanced targets and
  carries confidence/provenance metadata into the domain model.
- Main TUI/session DTC polling now scans standard `03`, `07`, `0A`, plus
  conservative GM Class 2 `$19 FF FF 00` and `$19 92 FF 00` cycles with
  no-data/unsupported backoff.
- TUI Desired MAP, GM barometer fallback, desired/actual fuel rail summary,
  VGT summary, and injector balance table are wired to the enhanced reading
  model.
- GUI backend uses the shared GM registry for Mode 22 decoding instead of a
  GUI-local decoder. It also scans GM Class 2 `$19` over the default Class 2
  node list and exposes category tabs, record/replay controls, settings, raw
  snapshot, and source-confidence metadata.
- Drive logger captures actual boost, desired boost, rail actual/desired, MAF,
  VGT actual/desired, and temperatures, and writes evidence records.

Known gaps:

- Probe results for `62 12 51 ...`, `62 15 42 ...`, `62 16 3D ...`,
  `62 16 3E ...`, and `$19/59` traffic are not persisted as raw routed
  request/response artifacts. A human saw live behavior, but the repo does not
  yet contain reviewable byte evidence.
- ScanGauge fuel-rail MTH is vendor-published, but its RXD is an 8-bit selected
  value. Effective range and display interpretation must be validated against
  persisted truck bytes before using it as the only full-range rail decoder.
- GM `$19` end-to-end live behavior is not proven by a saved `59` response.
  The decoder and polling path are ready, but module support on this truck still needs captured
  evidence.
- `vehicle-specs/chevy_duramax_2004_turbo.yaml` contains useful data but also
  stale or unverified values; it cannot be treated as a direct source of truth
  without cleanup.
- Passive GM Class 2 bus monitoring is not implemented.
- Recording/replay does not yet preserve typed GM `$19` records or passive
  Class 2 frames.

## Definitions

GM Class 2:

- GM's J1850 VPW serial data network used by this truck.
- OBD DLC pin 2.
- 10.4 kbps.
- Single serial adapter owner. Do not run a second process against the same
  serial port.

Physical routed request:

- Header format: `[priority, target, source]`.
- For a tool querying ECM node `0x10`: `6C 10 F1`.
- Request payload is the service byte plus service data.

Mode 22 DID:

- Manufacturer-specific "read data by identifier" request.
- On this platform, many useful DIDs require a trailing selector byte `01`.
- Example: `22 15 43 01`.

GM Class 2 DTC service:

- Service: `0x19`.
- Positive response: `0x59`.
- Request data:
  - all statuses: `FF FF 00`.
  - Tech2-like active/history/current mask: `92 FF 00`.
- Positive payload is repeating triplets: `[dtc_hi, dtc_lo, gm_status]`.
- Decode DTC bytes with SAE J2012-style two-byte DTC decoding.
- Do not decode the GM status byte as a UDS status byte.

GM status byte:

- `0x80`: warning indicator requested / MIL-equivalent indicator requested.
- `0x40`: pending.
- `0x20`: old.
- `0x10`: history.
- `0x02`: current.
- `0x01`: immature.
- `0x0C`: reserved bits; preserve raw byte.

## Goals

1. Provide a shared GM diagnostics service used by TUI, GUI, examples, and
   future recording/replay.
2. Make LLY-specific enhanced data reliable and clearly labeled.
3. Read GM Class 2 module-specific DTCs and merge them into the normal DTC
   display without hiding coverage state.
4. Preserve protocol failures as visible diagnostic states: empty, no data,
   unsupported, error.
5. Keep all GM protocol work deterministic and serial-owner safe.
6. Keep raw values and confidence/provenance available for disputed DIDs.
7. Add a passive monitor path for GM Class 2 console/system messages without
   corrupting active polling.
8. Make live probe evidence durable: every GM-specific "supported" claim must
   be backed by a persisted raw capture or a cited external source.

## Non-Goals

- Do not implement full CAN/GMLAN GM support in this phase.
- Do not implement security access or module programming.
- Do not perform actuator control or bidirectional tests until read-only
  diagnostics are stable.
- Do not document `22 15 42 01` as factory-verified Desired MAP until it is
  cross-checked against Tech2, EFILive, HP Tuners, or equivalent.
- Do not promote a live-probed DID from candidate/provisional to supported if
  the positive response bytes are not persisted.
- Do not claim module-specific GM DTC retrieval is working on this truck until
  at least one `$19` scan cycle is captured to disk showing positive `59`,
  negative `7F 19 xx`, empty, or no-data results per module.

## Evidence Model

Every GM-specific request path must be able to produce a durable evidence
record. This is a protocol requirement, not just debug logging.

Required evidence fields:

- timestamp.
- adapter port and protocol.
- vehicle identity if known: VIN, year/make/model/engine.
- module label and node.
- request header.
- request service and data bytes.
- raw adapter write text.
- raw adapter read text.
- parsed response bytes.
- decoder selected.
- decoded value or decoded DTCs.
- decode confidence.
- error classification when applicable.

Evidence sinks:

- raw capture sidecar for human review.
- structured JSONL or CSV probe output for tools.
- recording/replay frames for dashboard replay.

Minimum artifact examples:

```text
raw-captures/gm-probe-YYYYMMDD-HHMMSS.jsonl
raw-captures/gm-class2-dtc-YYYYMMDD-HHMMSS.jsonl
raw-captures/gm-drive-YYYYMMDD-HHMMSS.csv
```

Promotion rules:

- `candidate`: live behavior or research suggests the DID/service, but no
  persisted on-truck positive response exists.
- `live-probed`: at least one persisted on-truck positive response exists with
  decoded value and raw bytes.
- `community`: independent public source exists and local bytes are plausible.
- `verified`: OEM/factory tool cross-check or authoritative service data exists.
- `rejected`: persisted response proves unsupported, stale, wrong unit, wrong
  module, or wrong semantic meaning.

Probe tools must not only print to stdout. They must write an evidence file by
default and print that path before exiting.

Public-source evidence can raise a DID to `community`, but it does not replace
local capture. For live diagnostic support, keep a local evidence reference
after the first successful on-truck response.

## Target Vehicle Gate

The first complete profile is:

- Platform: GMT800.
- Make/model: GMC Sierra / Chevrolet Silverado HD.
- Model year: 2004.5-2005.
- Engine: 6.6L Duramax LLY.
- VIN eighth digit: `2`.
- ECM: Bosch E60.
- Protocol: J1850 VPW / GM Class 2.

The profile must not be automatically applied to LB7 trucks.

## Module Map

Use this as the default probe list. A module may return `NO DATA` or an
unsupported negative response depending on vehicle equipment.

| Node | Label | Header | Notes |
|---:|---|---|---|
| `0x10` | ECM/PCM | `6C 10 F1` | Primary LLY engine module |
| `0x11` | Engine node 2 / FICM candidate | `6C 11 F1` | Identity unresolved on this truck; resolve by capture before asserting FICM |
| `0x18` | TCM | `6C 18 F1` | Allison transmission controller |
| `0x1A` | TCCM | `6C 1A F1` | Transfer case, if equipped |
| `0x20` | IPC | `6C 20 F1` | Instrument panel cluster |
| `0x29` | EBCM/ABS | `6C 29 F1` | Brake controller |
| `0x40` | BCM | `6C 40 F1` | Body controller |
| `0x58` | SDM/SIR | `6C 58 F1` | Airbag controller |
| `0x60` | HVAC | `6C 60 F1` | Equipment-dependent |
| `0x80` | Radio/IRC | `6C 80 F1` | Equipment-dependent |
| `0xA0` | DDM | `6C A0 F1` | Driver door module, if equipped |

Node `0x11` must stay in the "resolve by capture" bucket. Some references map
similar U-code traffic to a second engine/ECM node, while the LLY diagnostic
context often expects FICM-related data. If it returns no data for FICM voltage
and DTC services, the UI must not present it as a confirmed FICM.

## LLY Enhanced Data Registry

Each entry must carry:

- service.
- DID.
- selector bytes.
- module.
- request header/TXD.
- ScanGauge RXF when provenance comes from an X-Gauge entry.
- RXD byte selector and width.
- raw MTH word when provenance comes from an X-Gauge entry.
- response shape.
- scaling.
- unit.
- confidence.
- provenance.
- display label.
- failure policy.
- poll cadence class.

### Published or Community-Backed Entries

Grouped cylinder rows below must expand into one registry entry per cylinder.

| Signal | Module | TXD / request | RXF | RXD | MTH / scale | Unit | Confidence | Notes |
|---|---|---|---|---|---|---|---|---|
| Transmission temperature | TCM | `6C18F122194001` | `046205190640` | `3008` (8-bit) | `00090005FFD8` = `raw * 9 / 5 - 40` | `deg F` | ScanGauge published | Header `6C18F1`; add before guessing other TCM DIDs |
| Oil pressure | ECM | `6C10F122147001` | `046205140670` | `3008` (8-bit) | `001D00320000` = `raw * 29 / 50` | `psi` | ScanGauge published | 2003+ listing |
| Desired fuel rail pressure | ECM | `6C10F122163D01` | `04624516063D` | `3008` (8-bit) | `0091000A0000` = `raw * 145 / 10` | ScanGauge label: `1000's of PSI` | ScanGauge published + live-observed | Range-suspect; persist local `62 16 3D ...` evidence and compare against expected rail behavior |
| Actual fuel rail pressure | ECM | `6C10F122163E01` | `04624516063E` | `3008` (8-bit) | `0091000A0000` = `raw * 145 / 10` | ScanGauge label: `1000's of PSI` | ScanGauge published + live-observed | Prefer standard PID `01 23` for actual rail when available because it is a full-range two-byte value |
| VGT vane desired | ECM | `6C10F122154001` | `046205150640` | `3008` (8-bit) | `006400FF0000` = `raw * 100 / 255` | `%` | ScanGauge published + live | Compute error as actual minus desired |
| VGT vane actual | ECM | `6C10F122154301` | `046205160643` | `3008` (8-bit) | `006400FF0000` = `raw * 100 / 255` | `%` | ScanGauge published + live | High value at idle/closed, lower value more open |
| Injector pulse width cyl 1-8 | ECM | `6C10F1221193..119A01` | `046245110693..069A` | `3010` (16-bit) | `00C800830000` = `raw * 200 / 131` | `ms` | ScanGauge published | Add as lower priority than balance rate |
| Injector balance cyl 1-8 | ECM | `6C10F122162F..163601` | `04628516062F..0636` | `3010` (16-bit) | `00050020EC00` = `raw * 5 / 32 - 5120` | `mm3` | ScanGauge published + live | Warm idle diagnostic value; offset is two's-complement `0xEC00` |

The ScanGauge TXD/RXF/RXD/MTH values above are vendor-published source data.
The MTH numeric decoding used by this project is the community-standard
`value = raw * multiplier / divisor + offset` interpretation, with signed
16-bit MTH words. The vendor page confirms the raw fields, while persisted
local captures must still be retained so future scaling corrections can be
made without losing evidence.

Fuel rail is the special case. ScanGauge publishes desired and actual fuel rail
with RXD `3008`, an 8-bit selected value, and MTH `0091000A0000`. That is
vendor-correct as an X-Gauge entry, but an 8-bit `raw * 14.5` value cannot
represent the full Duramax rail-pressure range directly. Treat `163D/163E` as
published DIDs with range-suspect display scaling until Phase 0 captures are
cross-checked. Actual rail should prefer standard PID `01 23`; desired rail has
no standard PID fallback.

### Live-Probed Entries Requiring Persisted Byte Evidence

These are plausible from the current live session, but they must remain below
`verified` until positive response bytes and cross-checks are persisted.

| Signal | Module | Request | Payload | Scale | Unit | Confidence | Required evidence |
|---|---|---|---|---|---|---|---|
| Barometric pressure | ECM | `22 12 51 01` | `A` | `A` | `kPa abs` | live-observed, evidence pending | Persist KOEO response; prove `0x1251` is the responding LLY V8 value and accept/reject `0x119D` from the same capture |

### Candidate Entry

| Signal | Module | Request | Payload | Scale | Unit | Confidence | Required validation |
|---|---|---|---|---|---|---|---|
| Desired MAP / boost target | ECM | `22 15 42 01` | `A` | `A` | `kPa abs` | candidate, live behavior observed | Persist `62 15 42 ...`; prove the selected `u8 kPa absolute` scale from raw bytes; cross-check against Tech2/EFILive/HP Tuners desired boost/MAP |

### Rejected or Superseded Entries

Maintain rejected entries explicitly so stale DIDs do not re-enter the codebase.

| Signal | Candidate | Status | Reason |
|---|---|---|---|
| Fuel rail actual | `22 11 70` | rejected for LLY live path until proven otherwise | Older spec entry; current truck work uses `01 23` and ScanGauge-published/live-observed `$22 163E 01` |
| Fuel rail desired | `22 11 71` | rejected for LLY live path until proven otherwise | Older spec entry; current truck work uses ScanGauge-published/live-observed `$22 163D 01` |
| Fuel rail error | `22 11 72` | rejected for LLY live path until proven otherwise | Older spec entry; compute actual minus desired until a verified LLY error DID exists |
| Desired boost/MAP | `22 11 17` | unverified legacy entry | Do not wire until a positive persisted response proves meaning and scale |
| Barometric pressure | `22 11 9D 01` | unresolved | Public/community label may point to V8, but current live-observed path used `0x1251`; capture both before deciding |

Display Desired MAP as absolute pressure when the panel is MAP-focused.
Display Desired Boost as `desired_map_abs - baro_abs` when the panel is
boost-focused.

Underboost shortfall can be computed without barometer if `$1542` is confirmed
as desired absolute MAP:

```text
underboost_shortfall_kpa = desired_map_kpa - actual_map_kpa
```

Actual MAP is standard PID `01 0B`, also absolute. Barometer cancels in the
shortfall equation. Barometer is still required to display gauge boost:

```text
boost_gauge = actual_map_abs - baro_abs
desired_boost_gauge = desired_map_abs - baro_abs
```

Sanity bounds for `$1542` while candidate:

- KOEO/idle should be near atmospheric pressure, roughly the same as actual MAP.
- Loaded desired MAP should climb well above atmospheric pressure.
- The selected scale must make values plausible in kPa absolute before it is
  used for alerts or automated diagnosis.

## Embedded Spec Cleanup Requirements

Update `vehicle-specs/chevy_duramax_2004_turbo.yaml` only after the registry
rules above are applied.

Required cleanup:

- Add `22 15 43 01` and `22 15 40 01` with community source and persisted
  evidence references when available.
- Add injector balance `22 16 2F 01` through `22 16 36 01`.
- Add candidate/live-probed entries for `22 16 3D 01`, `22 16 3E 01`,
  `22 12 51 01`, and `22 15 42 01`.
- Add ScanGauge-published entries for `22 14 70 01`, `22 19 40 01` at TCM node
  `0x18`, and injector pulse width `22 11 93 01` through `22 11 9A 01`.
- Mark stale fuel rail entries `0x1170`, `0x1171`, and `0x1172` as rejected,
  legacy, or unverified for this LLY profile unless new captures prove them.
- Mark `0x119D` barometer as unresolved until a persisted comparative probe
  proves whether this truck responds to it.
- Each YAML entry must include confidence, source/evidence reference, module,
  selector byte, response shape, scale, unit, and rejection reason when
  applicable.

## Standard PID Policy

The LLY PID support bitmap is not reliable enough to gate dashboard-critical
PIDs. The session must directly poll these PIDs even if `01 00` reports an
incomplete or sparse support mask:

- `01 04` engine load.
- `01 05` coolant temperature.
- `01 0B` intake MAP.
- `01 0C` engine RPM.
- `01 0D` vehicle speed.
- `01 0F` intake air temperature.
- `01 10` MAF.
- `01 23` fuel rail gauge pressure.
- `01 33` barometric pressure, but treat stale mismatched responses as stale.
- `01 42` control module voltage.

If a response is a positive Mode 01 frame for a different PID than requested,
classify it as a stale response and do not surface it as a dashboard error.

## GM-Specific Capability Scope

The GM integration should grow in layers. Each layer must expose coverage state
instead of hiding unsupported modules or unverified services.

### Layer A: LLY Engine Enhanced Data

- VGT actual/desired/error.
- Injector balance cylinders 1-8.
- Actual and desired fuel rail pressure.
- MAP, desired MAP, barometer, derived boost, and desired boost.
- MAF and core standard PIDs forced despite the unreliable support bitmap.

### Layer B: Module DTC Coverage

- Standard direct Mode `03`.
- Standard direct Mode `07`.
- Standard direct Mode `0A`, expected unsupported on many 2004 modules.
- GM Class 2 `$19 FF FF 00`.
- GM Class 2 `$19 92 FF 00`.
- Optional GM enhanced clear `$14` only after read coverage and module targeting
  are validated.

### Layer C: Module Capability Discovery

For each configured node, persist and display:

- module responds or no data.
- standard DTC services supported.
- GM `$19` supported.
- Mode 22 responds to profile DIDs.
- negative response codes.
- last successful timestamp.
- backoff state.

### Layer D: Transmission, Body, Brake, Cluster, and FICM Expansion

Do not add guessed DIDs. Add each capability only through the evidence model.

Priority candidates:

- TCM: current gear, commanded gear, TCC slip, transmission temperature if not
  available through standard profile data, TCM DTCs. Add ScanGauge-published
  `22 19 40 01` at node `0x18` before guessing other TCM temperature DIDs.
- FICM: voltage and communication status if the module responds.
- EBCM/ABS: brake/ABS DTCs.
- BCM: body and theft/security DTCs.
- IPC: cluster warning-related DTCs or status frames.
- SDM: airbag DTC visibility, read-only only.
- ECM extras: ScanGauge-published oil pressure `22 14 70 01` and injector
  pulse width `22 11 93 01` through `22 11 9A 01`.

### Layer E: Passive Class 2 Console/Status Traffic

- Bounded monitor mode.
- Raw frame storage.
- Source/target labeling.
- Unknown payload preservation.
- Future decoders can be added after repeated captured patterns exist.

## Architecture Requirements

### Shared GM Service

Create a shared GM service layer under `crates/obd2-dash` or, if the boundary
requires it, `obd2-core`.

Required capabilities:

- Build routed GM Class 2 requests.
- Read Mode 22 DIDs with optional selector bytes.
- Decode typed GM enhanced values.
- Read GM Class 2 `$19` DTCs.
- Preserve raw request/response bytes in mandatory evidence records and optional
  recording frames.
- Let probes, dashboard polling, and replay use the same decoders.
- Expose typed errors:
  - no data.
  - unsupported service.
  - unsupported subfunction.
  - malformed payload.
  - stale response.
  - transport error.

Do not duplicate this logic in the GUI backend.

### Profile Registry

Add a profile registry for manufacturer-specific signals.

Minimum model:

```rust
struct GmProfile {
    id: &'static str,
    applies_to: VehicleMatcher,
    protocol: GmProtocol,
    nodes: &'static [GmClass2Node],
    dids: &'static [GmDidDefinition],
    dtc_services: GmDtcServices,
}

struct GmDidDefinition {
    did: u16,
    selector: &'static [u8],
    module: GmClass2NodeId,
    name: &'static str,
    unit: &'static str,
    decode: GmDecodeKind,
    confidence: Confidence,
    provenance: &'static str,
    evidence_id: Option<&'static str>,
    cadence: PollCadence,
}
```

The actual implementation can use existing project types, but it must preserve
the fields above somewhere inspectable.

### Polling Cadence

J1850 VPW is slow. Do not poll every enhanced value every dashboard tick.

Suggested cadence classes:

- fast: actual MAP, RPM, MAF, VGT actual/desired, fuel rail actual/desired.
- medium: injector balance, barometer.
- slow: module scan, DTC scan.
- on-demand: passive monitor capture, clear DTC, freeze frame, special probes.

GM `$19` module scans must not run often enough to make live PIDs feel sticky.
Cache unsupported/no-data module-service combinations and back off.

### Evidence Writer

Add an evidence writer that is available to examples, live sessions, and tests.

Requirements:

- Append-only JSONL format for probe and service evidence.
- No blocking disk flush on the hot polling path unless explicitly requested.
- Include raw bytes and decoded values.
- Include negative responses and no-data outcomes.
- Include enough context to replay a single routed request offline.
- Redact nothing from vehicle protocol bytes; VIN may be included because the
  existing recordings already carry VIN metadata.

Probe examples must use this writer by default.

### Serial Ownership

All active polling, DTC scans, raw capture, and passive monitoring must run
through one session owner.

Do not open the same serial port twice.

`AT MA` / passive monitor mode is exclusive. The session must enter monitor mode
for a bounded window, exit it, then resume active polling. Active requests must
not be interleaved while the adapter is in monitor mode.

## GM Class 2 DTC Requirements

Implement `$19` scanning as an additive path beside standard DTC scanning.

For each configured module:

1. Send standard direct Mode `03`.
2. Send GM Class 2 `$19 FF FF 00`.
3. Send GM Class 2 `$19 92 FF 00`.
4. Decode positive `59` payloads.
5. Deduplicate by code, source module, and raw GM status byte.
6. Merge decoded records into the normal DTC panel.
7. Update module scan coverage panel with per-module service states.

States:

- `N dtc`: positive response with at least one decoded DTC.
- `empty`: positive response with zero DTCs.
- `no data`: no responder.
- `unsup`: negative response service/subfunction unsupported.
- `error`: transport, malformed payload, or unexpected protocol response.

Acceptance:

- A console/cluster message that corresponds to a module DTC must appear in the
  normal DTC list once `$19` reports it.
- A module that does not support `$19` must show `unsup`, not disappear.
- A module that does not respond must show `no data`, not disappear.
- Raw GM status byte must be visible in notes/details.

## Passive Class 2 Monitor Requirements

Add a bounded monitor API after active GM DTC support is stable.

Purpose:

- Capture Class 2 frames that may explain instrument cluster warnings not
  exposed by standard DTC services.
- Provide raw evidence for future decoders.

Required behavior:

- Session-owned only.
- Explicit start/stop or bounded duration.
- Emits typed frames with timestamp, priority, source, target, data bytes, and
  raw text.
- Does not corrupt active polling state.
- Can be recorded and replayed.

Initial decoder scope:

- Identify source/target module labels.
- Preserve unknown payloads as raw hex.
- Do not invent meanings for unknown frames.

## TUI Requirements

The TUI must consume the same GM service as the GUI.

Runtime layout must use category-specific tabs rather than one long mixed
dashboard. Tabs should change presentation only; polling and state ownership
remain in the session layer.

Required categories:

- Overview: compact live state, current alerts, most important active faults.
- Air / Boost: MAP, desired MAP, barometer, boost, desired boost, MAF, VGT.
- Fuel / VGT: rail actual/desired/delta, VGT actual/desired/error, injector
  balance table.
- Diagnostics: DTCs, GM `$19` module scan matrix, readiness, alert history.
- Thermal / System: coolant, intake air, oil/trans/ambient when available,
  battery, VIN, protocol.
- Record / Replay: recording state, replay picker, capture/evidence files.
- Raw / Evidence: raw snapshot, last routed requests, evidence file path.
- Settings: units, polling cadence, profile/confidence display.

Required changes:

- Show Desired MAP from the shared GM registry when available.
- Show Barometric pressure from GM enhanced fallback when standard `01 33`
  fails.
- Show actual and desired fuel rail pressure in the fuel rail panel.
- Show actual and desired boost where barometer is known.
- Keep injector balance in a readable table.
- Label VGT convention clearly enough to avoid "open percent" confusion:
  - recommended label: `VGT vane %`.
  - details/help text: high value on this LLY corresponds to closed/spool side;
    lower value corresponds to more open.
- Add GM `$19` columns to module scan or replace the current scan panel with a
  GM-aware matrix when the active profile is GM Class 2.
- Keep keyboard switching deterministic. Suggested TUI keys:
  - `[` / `]`: previous/next category tab.
  - number keys `1`-`8`: direct category selection.
  - existing widget focus/edit keys continue to operate inside the active tab.

## GUI Requirements

The GUI must not own a separate GM implementation.

GUI layout must use the same category set as the TUI. The GUI can render the
tabs as a horizontal/flowing tab bar, but labels and grouping should match the
TUI so screenshots, docs, and troubleshooting steps map one-to-one.

Required changes:

- Replace GUI-local constants and decoders with calls into the shared GM layer.
- Keep the current readouts:
  - desired MAP.
  - barometer.
  - desired fuel rail.
  - actual fuel rail.
  - injector balance table.
  - GM module DTC scan.
- Settings/about panel must describe signal confidence:
  - `verified`.
  - `community`.
  - `live-probed`.
  - `candidate`.
- Desired MAP must be labeled as live-probed/candidate until externally
  cross-checked.
- Category tab summaries should show live counts or headline values, for
  example DTC/alert count, boost/MAF, fuel rail delta, and voltage/coolant.

## Recording and Replay Requirements

Add stable recording support for GM-specific data.

Required frame classes:

- GM enhanced DID reading.
- GM enhanced DID error.
- GM Class 2 DTC scan result.
- GM Class 2 raw/passive frame.

Compatibility:

- Existing recordings must still replay.
- New frames must be skippable by older readers or version-gated.
- Replay must reproduce dashboard DTCs and GM enhanced readouts without live
  hardware.

## Validation Plan

Unit tests:

- Decode `$19` positive response with service byte.
- Decode `$19` payload without service byte.
- Decode empty all-zero payload.
- Reject payload lengths not divisible by 3.
- Preserve raw GM status byte.
- Do not use UDS status byte mapping.
- Decode each LLY Mode 22 response shape:
  - one-byte kPa.
  - one-byte percent.
  - two-byte pressure.
  - signed injector balance.
- Stale Mode 01 positive response for wrong PID is suppressed.

Integration tests with mock adapter:

- GM profile is selected only for matching LLY vehicle identity.
- TUI/session and GUI backend receive the same decoded GM values.
- `$19` module scan produces coverage rows for all configured nodes.
- Unsupported and no-data module results are cached/backed off.

Hardware validation:

- Probe persistence:
  - `gm_pressure_probe` writes JSONL with raw bytes for `0x119D`, `0x1251`,
    `0x1470`, `0x1540`, `0x1542`, `0x1543`, `0x163D`, and `0x163E`.
  - A TCM probe writes JSONL with raw bytes for `0x1940` at node `0x18`.
  - An injector pulse-width probe writes JSONL with raw bytes for `0x1193`
    through `0x119A`.
  - `gm_class2_probe` writes JSONL with raw bytes for every module and service.
  - Captures include positive responses, negative responses, no-data outcomes,
    and the exact request header.
- Key-on-engine-off:
  - MAP absolute approximately equals barometer.
  - desired boost approximately zero or near ambient target.
- Warm idle:
  - VGT actual tracks desired.
  - MAF plausible near known idle range.
  - injector balance values are stable enough for display.
- Hard pull:
  - desired boost and actual boost are logged.
  - desired rail and actual rail are logged.
  - VGT actual and desired are logged.
- DTC validation:
  - If a known module DTC exists, `$19` must report it with module and status.
  - If standard `03/07/0A` is empty but `$19` returns a module DTC, dashboard
    must show the GM DTC.
- Cross-tool validation:
  - Compare `22 15 42 01` against Tech2/EFILive/HP Tuners desired boost/MAP.

## Implementation Decomposition

### Phase 0: Persist Probe Evidence

- Add the evidence writer.
- Route `gm_pressure_probe`, `gm_desired_map_probe`, `gm_class2_probe`, and
  `gm_drive_logger` through it where applicable.
- Save raw request/response bytes for successful, unsupported, no-data, and
  malformed outcomes.
- Print the evidence file path at probe startup and completion.
- Add fixture-based tests that decode saved evidence records offline.
- For `0x163D` and `0x163E`, persist enough idle and loaded samples to validate
  the ScanGauge 8-bit `RXD=3008`/`MTH=0091000A0000` effective range against
  expected Duramax rail pressure behavior.

Exit criteria:

- Running a GM probe creates a reviewable file under `raw-captures/`.
- Positive `62 ...` responses for live-probed DIDs are persisted before those
  DIDs are marked `live-probed`.
- Fuel-rail desired/actual evidence either confirms the ScanGauge selected-byte
  scaling for the dashboard display range or records a separate full-range
  decoder path.
- `$19` module scan evidence is persisted before the dashboard claims
  module-specific GM DTC support is proven on the truck.

### Phase 1: Normalize GM Core and Registry

- Move GM constants, node list, `$19` decoder, and Mode 22 helpers into one
  shared module.
- Remove duplicate GUI-local GM DTC decode logic.
- Add profile metadata, confidence fields, evidence IDs, and rejected-DID
  entries.
- Add ScanGauge source fields: TXD, RXF, RXD, RXD width, raw MTH, decoded MTH
  transform, display unit, and notes.
- Move `0x1251`, `0x1542`, `0x163D`, and `0x163E` out of probe-only constants
  and into the registry with evidence-pending confidence.
- Add ScanGauge-published `0x1470`, `0x1940` at TCM node `0x18`, and
  `0x1193` through `0x119A`.
- Add tests for the shared layer.
- Clean up `vehicle-specs/chevy_duramax_2004_turbo.yaml` to match the registry.

Exit criteria:

- `cargo test -p obd2-dash` passes.
- GUI builds while importing the shared GM layer.
- No duplicated `$19` decoder remains in the Tauri backend.
- The embedded spec and registry contain the same DID confidence state.

### Phase 2: Wire GM Enhanced Values Into Session State

- Add typed GM enhanced readings to the domain model or map them through the
  existing enhanced reading path without losing metadata.
- Add desired MAP, barometer fallback, desired fuel rail, and actual fuel rail
  to the shared state used by TUI and GUI.
- Keep raw enhanced readings visible in debug details.
- Keep evidence status visible so the UI can label candidate values.

Exit criteria:

- TUI and GUI both display the same values from replay/mock/live paths.
- TUI Desired MAP is no longer hardcoded to missing.
- Desired MAP remains visually marked as candidate until cross-tool verified.

### Phase 3: GM `$19` DTC Integration

- Add `$19` scan to `session_runner`.
- Merge decoded GM DTCs with standard DTCs.
- Add module scan coverage for `03`, `19 FF`, and `19 92`.
- Cache repeated unsupported/no-data states.
- Persist every scan cycle to evidence when capture is enabled.

Exit criteria:

- Normal DTC panel can show GM module DTCs.
- Module scan panel shows coverage state for each configured node.
- Standard OBD DTC behavior remains unchanged on non-GM profiles.
- The dashboard distinguishes "empty positive response" from "never answered."

### Phase 4: Recording and Replay

- Add versioned GM frame types.
- Record GM enhanced readings and GM DTC scan results.
- Replay GM values into the same message/domain path used by live sessions.
- Replay evidence-derived fixtures through the shared decoders.

Exit criteria:

- A live GM session can be recorded and replayed with DTCs and enhanced values.
- Old recordings still replay.

### Phase 5: Passive Monitor

- Add session-owned bounded monitor mode.
- Emit typed raw Class 2 frames.
- Record/replay monitor frames.
- Add UI/debug view for recent frames.

Exit criteria:

- Monitor capture can run without leaving the adapter stuck in monitor mode.
- Active polling resumes cleanly after monitor window.
- Unknown frames are preserved, not guessed.

### Phase 6: Documentation and User-Facing Polish

- Clean up LLY YAML spec and mark stale DIDs as rejected or unverified.
- Add a GM/LLY diagnostics page to user docs.
- Add a probe evidence page explaining where captures are written and how to
  review a DID claim.
- Add troubleshooting notes for:
  - underboost diagnosis.
  - VGT vane percentage convention.
  - desired vs actual rail pressure.
  - standard OBD empty vs GM module DTCs.

Exit criteria:

- Docs do not state candidate DIDs as factory-verified.
- User can understand why a module says `unsup`, `no data`, or `empty`.

## Risks

- Some GM module addresses vary by equipment and year.
- Cheap ELM327 clones may mishandle headers, monitor mode, or fast polling.
- J1850 VPW latency can make high-frequency dashboard polling sticky.
- Desired MAP `22 15 42 01` behaves correctly in live data but still needs
  cross-tool confirmation.
- GM status byte interpretation is community/research-backed; preserve raw byte
  so future corrections do not lose data.
- ScanGauge MTH numeric decoding uses the community-standard signed-word
  interpretation; preserve raw MTH, RXF, RXD, raw response bytes, and display
  units so any correction is reversible.
- ScanGauge fuel-rail DIDs are published, but their X-Gauge selected-byte range
  may not equal a full-range rail-pressure decoder. Prefer standard PID `01 23`
  for actual rail when available and validate desired rail separately.
- Passive monitor payload meanings are mostly unknown until captured and
  decoded from real traffic.

## Technical Writer Handoff

Audience:

- Primary: developers implementing GM diagnostics in this repo.
- Secondary: technically capable vehicle owners using the dashboard for LLY
  diagnostics.

Writer deliverables:

1. Developer guide: `docs/gm-class2-diagnostics.md`.
2. User guide section: "GM / Duramax enhanced diagnostics".
3. Troubleshooting page: "LLY underboost data: how to read the dashboard".
4. Evidence guide: "How GM probe evidence files prove a DID or service".
5. Category-tab UI guide: "Where to find each GM diagnostic category".
6. Glossary entries:
   - GM Class 2.
   - J1850 VPW.
   - Mode 22 DID.
   - GM `$19` DTC.
   - probe evidence.
   - VGT vane position.
   - MAP absolute vs boost gauge.
   - desired vs actual fuel rail pressure.

Tone:

- Factual and conservative.
- Do not imply OEM certification.
- Separate verified behavior, live-probed behavior, and candidate behavior.
- Avoid repair certainty from a single PID. Explain what a data pattern supports
  and what it does not prove.

Do not publish as fact:

- That `22 15 42 01` is factory-defined Desired MAP until cross-tool confirmed.
- That `22 12 51 01` is the final LLY V8 barometer DID until comparative
  `0x1251`/`0x119D` evidence is saved.
- That ScanGauge-published fuel pressure DIDs `22 16 3D 01` and `22 16 3E 01`
  are locally live-proven until persisted positive response bytes and sanity
  checks are attached.
- That the ScanGauge fuel-rail MTH is a full-range Duramax rail-pressure
  decoder until the 8-bit selected-byte range is validated from persisted truck
  captures.
- That a clean standard Mode `03` scan means all GM modules are free of codes.
- That GM `$19` module DTC retrieval is proven on the truck until saved `59`,
  negative-response, empty, or no-data evidence exists per module.
- That VGT actual/desired tracking proves the turbo is mechanically healthy.
- That passive Class 2 frames are decoded unless a decoder exists.

Important wording:

- Say "GM Class 2 module DTC scan" instead of "Tech2-equivalent full scan" until
  behavior is proven across more modules.
- Say "VGT vane percent on this LLY reads high near the closed/spool side and
  lower as the vanes open" instead of "open percent".
- Say "MAP is absolute pressure; boost is MAP minus barometer".
- Say "underboost shortfall is desired MAP minus actual MAP" only while clearly
  labeling `$1542` as provisional/candidate.
- Say "candidate" or "live-observed, evidence pending" when the raw bytes are
  not persisted yet.
- Say "empty positive response" only when the capture shows a positive response
  with no DTC records; otherwise use "no data" or "unsupported".

Source material the writer should use:

- This spec.
- `crates/obd2-dash/src/gm_class2.rs`.
- `crates/obd2-dash/examples/gm_class2_probe.rs`.
- `crates/obd2-dash/examples/gm_drive_logger.rs`.
- ScanGauge LB7/LLY Duramax X-Gauge list:
  `https://www.scangauge.com/x-gauge-commands/lb7-lly-duramax/`.
- `apps/obd2-gui/src-tauri/src/main.rs` until shared GM logic is moved.
- `vehicle-specs/chevy_duramax_2004_turbo.yaml`, but only after stale entries
  are marked or removed.
- `raw-captures/gm-*.jsonl` evidence files once Phase 0 exists.

Open questions for writer/developer sync:

- What final label should the UI use for VGT percent?
- Should Desired MAP display as absolute MAP, gauge boost target, or both?
- How much raw GM status-byte detail should be shown in the user UI versus
  debug/details panels?
- Should passive monitor documentation be hidden until the feature is
  implemented?
- Should `0x1251` or `0x119D` be the final barometer DID for this LLY after
  persisted comparative capture?
- Should `22 15 42 01` remain visible in normal UI while candidate, or only in
  evidence/debug views until cross-tool verified?
