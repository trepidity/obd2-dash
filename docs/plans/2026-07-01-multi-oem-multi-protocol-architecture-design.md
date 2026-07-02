# Multi-OEM, Multi-Protocol Diagnostics — Architecture & Strategy

**Date:** 2026-07-01
**Status:** Draft for review
**Scope:** `obd2-core` (library) + `obd2-dash` (TUI/GUI app) — full US-market vehicle support
**Author:** Design session (Claude) with Jared

---

## 0. Purpose and reading order

This is the **umbrella architecture** for supporting all major US-market manufacturers,
vehicle classes, and diagnostic protocols from model year 2001 to present, commercial and
non-commercial. It sits *above* — and depends on — the manufacturer-profile work already in
flight:

- `docs/plans/2026-06-29-manufacturer-profile-migration-plan.md` (9 invariants)
- `docs/plans/2026-06-29-manufacturer-profile-implementation-waves.md` (Wave 0–8)
- `docs/plans/2026-06-29-module-support-architecture.md` (ModuleKey/AddressState/evidence)
- `docs/plans/2026-06-29-gm-specific-diagnostics-spec.md` (GM/LLY)
- `docs/plans/2026-07-01-obd2-gui-capability-driven-ui-spec.md` (capability-driven UI)

Those documents describe *how the GM profile is being migrated and how one OEM is expressed*.
This document describes *how the stack generalizes to seven OEMs and every US protocol without
one OEM's support breaking another*, and what must change in `obd2-core` (protocol/transport)
to make that real. Where this document and the in-flight waves overlap, the waves are the
implementation-of-record for the GM/profile pieces; this document adds the protocol-core
generalization, the hardware-tier plan, the security/authorization model, and the commercial
(J1939) plan, and it defines the cross-OEM isolation contract that all OEM work must satisfy.

---

## 1. Goals and standing rules

### 1.1 Product goals

1. **Coverage:** Ford, GM (incl. Chevrolet/GMC/Cadillac/Buick), Toyota, Honda, Nissan, RAM/Stellantis — MY2001→present, USA market.
2. **Depth (tiered):** Legislated OBD everywhere on day one; OEM-enhanced depth per-OEM in validated waves. GM/LLY is the deepest reference.
3. **Classes:** Light-duty and commercial. Medium-duty pickups/chassis-cabs (RAM 4500/5500, F-450/550, GM HD) via light-duty OBD immediately; heavy-duty J1939/J1587 as a first-class protocol built early.
4. **All protocols:** the five legacy DLC protocols, ISO 15765-4 CAN, UDS/KWP2000, GM GMLAN/Class-2, J1979-2 OBDonUDS, CAN FD, DoIP, and J1939-73.

### 1.2 Standing rules (restated as testable invariants)

> **RULE 1 — Supporting a new manufacturer cannot break another.**
> **RULE 2 — Full and proper protocol isolation and support.**

These become the following **enforced invariants** (see §6):

- **INV-1 (Data, not enums):** Adding an OEM adds `&'static` data + one registry line + (optionally) one decoder module. It never edits a shared closed enum, never edits `session_runner`/`domain`, never touches another OEM's module.
- **INV-2 (Typed seams):** OEM profiles and protocol backends meet only at trait boundaries (`DiagnosticProfile`, `Transport`, `ProtocolClient`). Neither reaches into the other's internals.
- **INV-3 (No cross-OEM reference):** Architecture tests forbid any OEM module referencing another OEM's symbols, and forbid live/UI/session code referencing OEM-specific symbols. Extended per OEM.
- **INV-4 (Frozen decode):** Every OEM/vehicle has a golden corpus that pins decode output bit-exact. A shared-decoder change that perturbs any other OEM's golden output fails CI.
- **INV-5 (Protocol purity):** `protocol/` decoders are pure (bytes→values); no OEM data and no transport/ELM text coupling. Enforced by a source-scan test.

---

## 2. Current architecture assessment

### 2.1 What is strong and must be preserved

- **`DiagnosticProfile` trait** (`obd2-dash/src/profiles/model.rs:569`): object-safe, all capabilities are pure `&'static` const tables (signals, DTC services, module map, active tests); only two methods run code (`decode_signal`, `decode_dtc_response`). This is the correct plugin shape and is rarer/better than most OSS diagnostic stacks.
- **Evidence/provenance:** every dispatch produces a `ProfileEvidenceRecord` with confidence, provenance, source fields (ScanGauge TXD/RXF/RXD), physical address, raw bytes (`profiles/evidence.rs`, `profiles/runtime.rs:99`).
- **Sealed selection token:** `SelectedProfile::seal` is `pub(in crate::profiles)`, generation-bound; `tests/wave1_architecture.rs` pins the only mint sites.
- **Architecture tests as a firewall:** `tests/architecture.rs`, `architectural_import.rs`, `wave1_architecture.rs` are source-scan tests (GM symbols only in GM files; fixture profile GM-free; live/GUI code cannot reach raw GM helpers; `raw_request` occurrence caps).
- **Golden corpus (additive-only):** protocol payload tier, signal decode tier, DTC decode tier, selection cases — `tests/corpus/`.
- **Data-driven enhanced PIDs in core:** `EnhancedPid`/`Formula` are `serde`-deserializable and `#[non_exhaustive]` (`obd2-core/src/protocol/enhanced.rs:16`).
- **Real J1939 decoders:** correct SPN scaling + DM1 SPN/FMI for 7 PGNs (`obd2-core/src/protocol/j1939.rs`).
- **`#[non_exhaustive]` `Protocol` and `PhysicalAddress`** in core already exist — additive protocol growth is anticipated.

### 2.2 What blocks the multi-OEM / multi-protocol goal

**In `obd2-core` (protocol/transport axis):**

- **ELM-text coupling is load-bearing and mis-layered.** `Transport::read` implementations hardcode the ELM `>` prompt as end-of-frame (`serial.rs`, `ble.rs`); `codec.rs::decode_elm_response_payload_for_command` parses ELM ASCII lines; the positive-response check assumes `SID+0x40` echo (`codec.rs:369`). A J2534/SocketCAN/DoIP backend cannot reuse `Transport` or the read-until-prompt contract.
- **Single ELM adapter, single active protocol, single ECU.** `ATSP0` auto-detect + `ATTP` fallback; header targeting is `ATSH` only (no `ATCRA` receive filter, no `ATST` timeout tuning); ISO-TP reassembly delegated to the chip (`ATCAF1`/`ATCFC1`). Broadcast multi-ECU responses concatenate with no per-source demux.
- **STN detected but never used.** `Chipset::Stn` is classified with capability flags, but zero `ST` commands are issued anywhere → MS-CAN (Ford body) and SW-CAN GMLAN (GM body) are unreachable.
- **No CAN FD, no DoIP, no real J1939 transmit.** `Protocol::from_elm_code` covers ELM codes 1–9 only. `read_j1939_pgn` fakes a "service 0xEA" through the OBD path — no 29-bit request-PGN header, no `ATSP A`, no TP.BAM/TP.CM reassembly (so any DM1 with >1–2 DTCs cannot be received). `PhysicalAddress::J1939` errors in the adapter (`elm327.rs:258`).
- **Hardcoded GM-flavored ELM policy in shared code:** K-line wakeup `ATWM686AF10100`/`ATWMC133F13E`, broadcast headers `686AF1`/`7DF`/`18DB33F1`, per-service skip-byte table — all in `Elm327Adapter`, all shared, all editing-risk for a non-GM vehicle.
- **Cross-spec bleed:** `SpecRegistry::match_vin` returns first hit (no ambiguity detection); `lookup_dtc` scans *all* loaded specs and returns first hit — a GM P1xxx meaning can be applied to a Ford DTC.

**In `obd2-dash` (OEM-knowledge axis):**

- **Closed enums force shared edits per OEM:** `Manufacturer` (Gm/Ford/ChryslerRam/Fixture/Generic), `ModuleKey` (8 GM-flavored variants). Toyota/Honda/Nissan need enum edits + exhaustive-match test edits.
- **Only 3 address templates:** `AddressTemplate::{J1850, Can11, Can29}`. No K-line, no UDS 29-bit `18DAxxF1` (Honda *requires* it), no J1939 PGN/SPN, no DoIP, no CAN FD.
- **Request shape assumes 2-byte Mode-$22 DIDs** (`signal_did` reads `request_data[0..2]` big-endian). Breaks Toyota/Honda `$21` locals, Ford 3-byte DIDs, J1939 PGN/SPN.
- **GM leakage past the profile boundary:** `session_runner::profile_dtc_service_for_key` hardcodes `lly.class2.dtc.*`; `build_enhanced_targets` calls LLY gating; `should_force_standard_poll` hardcodes the LLY forced-PID list globally; `DiagnosticCommand::GmActiveTest` / `Message::ActiveTestResult` are GM-typed; `domain::DtcService` mixes SAE + `GmClass2*`.
- **One profile per session is axiomatic** (`SelectedProfile`) — fine for one vehicle, but the gateway/multi-bus reality (DoIP, GM SDGM) has no model yet.

> Much of the `obd2-dash` list is *already being addressed* by the Wave 0–8 profile migration
> (Waves 4–6 in flight). This document's job is to (a) make sure those waves land the *general*
> shapes (open registries, N address templates) not GM-only ones, and (b) add the obd2-core
> protocol-core work the waves assume but don't themselves build.

---

## 3. Research foundation (US diagnostics 2001–2026)

Full per-OEM matrices, hardware capability matrix, standards timeline, security-gateway
analysis, and commercial/J1939 findings are archived in the session research set. Load-bearing
conclusions:

### 3.1 Protocol timeline (the architectural backbone)

- **1996–2007:** five legacy DLC protocols, OEM-specific — **J1850 VPW** (GM "Class 2", 10.4 kbps, pin 2), **J1850 PWM** (Ford "SCP", 41.6 kbps, pins 2/10), **ISO 9141-2** K-line (Chrysler/Toyota/Honda/Nissan, pin 7), **ISO 14230-4 KWP2000** (later K-line), **ISO 15765-4 CAN** (allowed from MY2003).
- **MY2008:** ISO 15765-4 CAN becomes the *only* permitted legislated OBD protocol (pins 6/14). 1996–2007 needs legacy auto-detect; 2008+ guarantees CAN.
- **Enhanced application protocols:** GM **GMW3110/GMLAN** (`$1A`/`$22`/`$A9` DTCs/`$AE` device control — *not* full UDS); Ford SCP→KWP-ish→**Ford UDS** (ISO 14229, `0x7xx` addressing); Chrysler PCI/SCI→KWP→**UDS (wiTECH)**; Toyota **M-OBD** `$21` locals→**UDS** on TNGA; Honda **HDS** (KWP-ish over K-line, then **UDS at 29-bit `18DAxxF1`** from ~2016); Nissan **Consult II/III** (K-line→UDS-flavored).
- **J1979-2 OBDonUDS (SAE, Apr 2021):** CARB-permitted MY2023–2026, **mandatory MY2027** (light/medium/heavy). UDS SIDs replace classic modes (`$22` DIDs `0xF4xx`, `$19` subfunctions for DTCs, 3-byte DTCs = 2-byte code + Failure Type Byte). Protocol detection: functional `$22 F810` (OBDonUDS) vs `$01 00` (classic) per ISO 15765-4:2021 §6. **One UDS engine serves both J1979-2 generic OBD and OEM-enhanced diagnostics.**
- **CAN FD at the DLC:** GM Global B/VIP (2020+, 29-bit, ~5 Mbps, behind the K56 SDGM gateway), Ford ~2021, Stellantis 2021+ (Atlantis/STLA), Toyota TNGA 2023+. Legislated OBD stays classical CAN, but *enhanced/module scans need CAN FD hardware*.
- **DoIP (ISO 13400):** OEM-tooling-driven, not emissions-mandated in the US through 2026. Primarily Euro brands + newest Toyota/Stellantis. Pure TCP/UDP:13400 → cheap to add once UDS exists.

### 3.2 Per-OEM protocol summary (verified, adversarially fact-checked)

| OEM | 2001–07 legislated | Enhanced | Secondary DLC buses | 2008+ | Gateway (2018+) |
|-----|-------------------|----------|--------------------|-------|-----------------|
| **GM** | J1850 VPW (pin 2); GMLAN CAN phases in MY2004 (Ion/XLR) | Class-2/J2190 (`$22`,`$19`,`$AE`,`$27`); GMLAN GMW3110 | LS-GMLAN SW-CAN 33.3k (pin 1); MS-GMLAN ~95k (3/11) | ISO 15765-4 | Global B/VIP 2020+: CAN FD, SDGM isolation, 12-byte `$27`; no AutoAuth (GM SPS2/Techline) |
| **Ford** | J1850 PWM/SCP (2/10); ISO 9141 (7); UBP (3) | SCP→KWP→Ford UDS | MS-CAN 125k (3/11) — **not** reachable by stock ELM | ISO 15765-4 | "Security Link" 2020/21+: blocks bidirectional; FDRS account, no AutoAuth |
| **Stellantis/RAM** | J1850 VPW/PCI (2); CCD; SCI (reflash) | DRB→KWP→UDS (wiTECH) | CAN-C 500k / CAN-IHS 125k / CAN-B 83.3k behind TIPM/CGW | ISO 15765-4 | **SGW 2018+** (some 2017): blocks writes/clears/actuation, reads open; **AutoAuth ~$50/yr**; SFD on newest |
| **Toyota** | ISO 9141-2 / ISO 14230 K-line (7) | M-OBD `$21` locals (Techstream)→UDS | K-line only; internal buses behind gateway | ISO 15765-4 (CAN FD/DoIP 2023+) | SecOC (msg auth, ~2020+) + NASTF VSP for key/security; not an FCA-style read firewall |
| **Honda** | ISO 9141-2 K-line (7); SCS pin 9 | HDS (KWP-ish); **UDS 29-bit `18DAxxF1`** from ~2016 | B-CAN 33.3k gatewayed via MICU over K-line — never at DLC | CAN OBD from MY2006 Civic (**29-bit!**), universal MY2008 | No FCA-style gateway; NASTF VSP for keys; CAN-FD gateway drops some OBD queries (serialize) |
| **Nissan** | ISO 9141-2 K-line (legislated; 5-baud init) | Consult II = **KWP2000 on K-line** (`$21` locals); Consult III/III+ = **KWP-over-CAN** (`$10 C0` session), UDS piecemeal | K-line; CAN legislated MY2007 per-model, universal 2008; Leaf EV-CAN 12/13 + AV-CAN 3/11 | ISO 15765-4 | Central gateway select 2019/20+ (Sentra 20+/Rogue 21+/Pathfinder 22+); **AutoAuth-brokered** |

Key correctness traps the research caught (must be honored in code):

- **Honda uses 29-bit `18DAxxF1` addressing** for enhanced/UDS *and* MY2006 Civic legislated OBD is 29-bit. A compliant stack must probe 11-bit **and** 29-bit ISO 15765-4 **and** ISO 9141-2.
- **Honda K-line persists post-2008** as the HDS path to the MICU/B-CAN — "K-line gone at MY2008" is false.
- **GM CAN migration starts MY2004, not 2008**; VPW persists through MY2007 (GMT800 "Classic").
- **Ford MS-CAN and GM SW-CAN GMLAN need STN hardware** (stock ELM327 cannot reach them).
- **Acura ZDX / Honda Prologue (2024+) are GM Ultium** — handle under a **GM Global-B profile**, not Honda.
- **Cross-spec DTC bleed** is a live bug: P1xxx meanings are OEM-specific; the DTC-description layer must be OEM-scoped.

### 3.3 Hardware tiers (build order)

1. **ELM327 AT serial (Tier 1, exists):** all five legacy protocols + CAN 11/29-bit @ 250/500k + basic J1939 (protocol A). Request/response only; 512-byte buffer overflows on monitor; **cannot** do MS-CAN/SW-CAN/CAN FD/DoIP. Clones frozen at v1.4 behavior → fingerprint, don't trust the version string; keep a quirks table.
2. **STN/OBDLink ST extensions (Tier 1+):** strict ELM superset. Unlocks **MS-CAN** (STP 51–54), **SW-CAN GMLAN 33.3k** (STP 61–64), J1939 presets, `STPX` 4KB transfers, RAM filter lists, 1-ms `STPTO`. *This is the tier that makes GM + Ford enhanced/module scans real.* One CAN peripheral muxed → only one CAN channel active at a time.
3. **Native CAN (Tier 2):** SocketCAN + kernel `can-isotp` + `can-j1939` (Linux), gs_usb/candleLight (WinUSB/libusb elsewhere). Frame-level control, sub-ms latency, parallel multi-ECU ISO-TP, and the **only realistic path to CAN FD** (GM Global B). No K-line/J1850.
4. **DoIP/HSFZ (Tier 3):** pure `std::net` + tokio, zero FFI, cross-platform. UDS over TCP:13400 (BMW F-series HSFZ on 6801). Near-free once UDS exists.
5. **J2534 (Tier 4, Windows-first, optional):** pro VCIs; adds **Chrysler SCI** (pre-CAN engine data), CAN FD/DoIP on v05.00. Windows DLL ecosystem (registry discovery) — `libloading` backend. Open option: Macchina-J2534 (Rust).
6. **RP1210 (Tier 4, heavy trucks, optional):** J1939 + **J1708/J1587**. Windows DLL. On Linux, `can-j1939` substitutes; J1708 needs RP1210 hardware or a serial transceiver.

### 3.4 Security/authorization model

The dominant pattern across all OEMs: **reads are open, writes are gated.** Identification,
DTC read, live PIDs, and readiness work with **no authentication** on virtually every covered
vehicle. Only DTC-clear, actuator/bidirectional tests, coding, and resets are gated. Therefore
a **read-only diagnostic product is fully viable today across all seven OEMs** with zero
authentication integration — highest value, lowest risk, ship first.

Authorization mechanisms differ and must be pluggable:

- **AutoAuth** (FCA/Stellantis, Nissan/Infiniti, Mercedes): federated broker, ~$50–60/brand/yr, tool must be vendor-certified. One integration, three OEMs.
- **OEM subscription** (Ford FDRS, GM SPS2/Techline, Toyota GTS+, Honda i-HDS): user brings their own paid OEM session.
- **NASTF SDRM/VSP:** cross-OEM identity path for key/immobilizer/security-code functions.
- **UDS `$27` seed-key** (per-ECU) vs **`$29` Authentication** (certificate/PKI, ISO 14229:2020) — GM Global B uses 12-byte `$27` + server validation; UN R155 / ISO 21434 (NHTSA CSMS guide, July 2026 US alignment) push everyone toward `$29`, and eventually toward authenticated *reads*.

Design consequence: model authorization as a **per-session, per-ECU capability negotiation**
(`AuthProvider` trait) between transport and write-capable services, defaulting to read-only
when unauthenticated. Never depend on hardware bypass (12+8 cables) for a legitimate product.

### 3.5 Commercial / J1939

Three worlds: (1) **Heavy-duty J1939** — 29-bit CAN 250k→500k (green Type-II 9-pin from ~MY2016);
needs J1939-81 address claiming, J1939-21 transport (TP.BAM broadcast + TP.CM RTS/CTS, ≤1785 B),
J1939-73 diagnostics (DM1/DM2/DM3/DM11/DM5/DM7/DM8/DM30/DM24/DM25; 4-byte SPN+FMI+OC, CM bit for
pre-2010). (2) **CARB HD-OBD (2010/2013)** permits *either* J1939 *or* ISO 15765-4 — Cummins/Detroit/Paccar/Navistar
stayed J1939 (9-pin); Volvo/Mack moved to a 16-pin carrying J1939+J1587+ISO 15765/UDS. (3) **Medium-duty
pickups/chassis-cabs** (RAM 4500/5500, F-450/550, GM HD, E-series) are standard light-duty ISO 15765-4
OBD-II — *these work with the light-duty stack immediately*.

- **J1939DA licensing:** the SPN/PGN database is paid, per-revision. Ship a hand-curated public SPN/PGN subset (FMS standard + public fault-code lists) as built-in data; support importing a user-licensed DA-derived JSON/DBC overlay (the `pretty_j1939` model). Keeps the open tool legally clean.
- **J1708/J1587** (≈1985–2013 tail): MID/PID/SID+FMI model, separate decode tables; needs RP1210 hardware — a later, optional tier.
- **Allison TCM:** GM-pickup LCT1000 = GM Class-2/GMLAN over OBD-II; commercial-chassis WTEC = J1587 (pre-2006) → J1939 (4th-gen+).

---

## 4. Target architecture

### 4.1 Layered stack (the whole system)

```
┌──────────────────────────────────────────────────────────────────────┐
│ UI: TUI (ratatui) + GUI (Tauri) — render from profile capabilities     │
├──────────────────────────────────────────────────────────────────────┤
│ obd2-dash: SessionRunner (integration boundary)                        │
│   ├─ ProfileRegistry / DiagnosticProfile (OEM knowledge, pure data)    │  Axis 2
│   ├─ ProfileRuntime (route → address → request → decode → evidence)    │  (OEM)
│   ├─ Scheduler (cadence) · Selection (VIN/probe) · Evidence sink       │
│   └─ AuthProvider (per-OEM authorization for write-capable services)   │
├──────────────────────────────────────────────────────────────────────┤
│ obd2-core: Session (orchestration, discovery, single-flight, capture)  │
├──────────────────────────────────────────────────────────────────────┤
│   ProtocolClient  (J1979 · J1979-2/UDS · KWP2000 · GMLAN · J1939-73)   │  ◀── NEW
├──────────────────────────────────────────────────────────────────────┤  Axis 1
│   Transport  (ISO-TP · J1850 · K-line · J1939-TP · DoIP · raw-frame)   │  ◀── NEW  (protocol)
├──────────────────────────────────────────────────────────────────────┤
│   Link/Backend (ELM/STN AT · J2534 FFI · SocketCAN/gs_usb · DoIP TCP)  │  ◀── generalized Adapter
├──────────────────────────────────────────────────────────────────────┤
│   Physical (serial · BLE · USB · TCP)  — existing Transport→"Link"      │
└──────────────────────────────────────────────────────────────────────┘
```

The **surgical change** (recommended approach) is inserting the two NEW layers cleanly:

- **`Transport`** (rename current byte-`Transport` → **`Link`**): exchanges *framed diagnostic PDUs* for one addressing scheme. Implementations: `IsoTpTransport`, `J1850Transport`, `KLineTransport`, `J1939Transport`, `DoIpTransport`, `RawCanTransport`. Each owns *its* framing/timing/flow-control — no ELM-text assumptions leak up.
- **`ProtocolClient`**: turns high-level requests (read DID, read DTCs, routine, security access) into service PDUs for one *application* protocol and parses responses. Implementations: `J1979Client`, `UdsClient` (serves both J1979-2 and OEM-enhanced), `Kwp2000Client`, `GmlanClient`, `J1939Client`. Response validation is per-protocol (no shared `SID+0x40` assumption).

A **backend** provides `(Link, Transport)` pairs it can realize. ELM/STN realizes ISO-TP (via
chip `ATCAF1`/ST filters), J1850, K-line, and basic J1939 through AT/ST text. SocketCAN realizes
ISO-TP (kernel), raw CAN, CAN FD, J1939 (kernel). DoIP realizes DoIP transport directly. J2534
realizes all of them via channels. This is where "protocol isolation" becomes *structural*:
each `(backend, transport, protocol-client)` triple is independently testable and cannot corrupt
another.

### 4.2 Backend capability negotiation

Replace the "ELM auto-detect one protocol" model with explicit capability discovery:

```rust
struct BackendCaps {
    links: &'static [LinkKind],          // Serial, Ble, Usb, Tcp
    transports: &'static [TransportKind],// IsoTp, J1850Vpw, J1850Pwm, KLine, J1939, DoIp, RawCan, RawCanFd
    can_fd: bool,
    channels: u8,                        // 1 for ELM, N for J2534/SocketCAN
    secondary_can: &'static [SecondaryBus], // MsCan125, SwCanGmlan33, MsGmlan95
    max_pdu: usize,
    quirks: QuirkFlags,                  // clone bugs, buffer limits
}
```

Vehicle protocol detection follows ISO 15765-4:2021 §6 (probe 500k then 250k, 11-bit and 29-bit,
`$01 00` vs `$22 F810`), falling back to legacy (J1850, then K-line) — driven by the profile's
`allowed_protocols` + the backend's `transports`.

### 4.3 Addressing generalization

`AddressTemplate` grows (additively; it is already matched exhaustively so add variants + tests):

```rust
enum AddressTemplate {
    J1850 { node: u8 },                                  // exists (GM Class-2, Ford SCP w/ convention)
    Can11 { request_id: u16, response_id: u16 },         // exists
    Can29 { request_id: u32, response_id: u32 },         // exists
    KLine { init: KLineInit, addr: u8 },                 // NEW  Toyota/Honda/Nissan/Ford ISO9141/KWP
    UdsCan29Fixed { target: u8 },                        // NEW  Honda 18DAxxF1 normal-fixed addressing
    J1939 { source_address: u8, pgn: u32 },              // NEW  commercial
    DoIp { logical_address: u16 },                       // NEW  Euro/newest
}
```

Request shape generalizes beyond 2-byte DIDs: a `RequestKind` covering `Mode01Pid(u8)`,
`Did16(u16)` (UDS/GMLAN), `LocalId8(u8)` (KWP/`$21`), `Did24` (Ford), `Pgn(u32)` (J1939) — so
`signal_did`/UI keying no longer assume `request_data[0..2]`.

### 4.4 OEM knowledge: from closed enums to open registries

To satisfy **INV-1**, the identity/module vocabulary moves from closed enums to string-keyed
newtypes (the pattern `ProfileId` and `BusKey` already use):

- `Manufacturer` enum → `ManufacturerId(&'static str)` (or keep the enum but make it *non-load-bearing* — display/grouping only, never matched exhaustively in shared logic).
- `ModuleKey` enum → `ModuleKey(&'static str)` with a registry of well-known keys (`ecm`, `tcm`, `bcm`, `abs`, `srs`, `ipc`, `hvac`, plus OEM-specific like `honda.micu`, `ford.gwm`). `session_runner::profile_module_key` string duplication is deleted.
- `domain::DtcService` GM variants → generic `DtcServiceId(&'static str)` carried by the profile; `session_runner::profile_dtc_service_for_key` hardcoded map is deleted (the profile *is* the map).
- Active tests: replace `DiagnosticCommand::GmActiveTest`/`Message::ActiveTestResult` with a generic `ActiveTestCommand { profile, capability, params }` / `ActiveTestOutcome`, routed through `ProfileRuntime` (unlocking the currently hard-locked active-test path uniformly).

This is largely the destination the Wave 4–6 migration is already heading toward; this document
makes "generalize, don't GM-shape" the explicit acceptance criterion.

---

## 5. Per-OEM support model (tiered depth)

Every OEM progresses through the same capability tiers. "Support" is declared at the tier a
validated corpus exists for.

- **Tier 0 — Generic OBD (all vehicles, day one):** J1979 (or J1979-2) live data, DTCs (`$03/$07/$0A` or `$19`), readiness, freeze frame, VIN. No profile required; works on the generic path.
- **Tier 1 — Identity + module map:** VIN→profile selection, OEM DTC descriptions (OEM-scoped, fixing the P1xxx bleed), module discovery/addressing per era.
- **Tier 2 — Enhanced live data:** OEM DIDs/PIDs with provenance/confidence (GM Class-2, Ford UDS DIDs, Toyota `$21`/UDS, Honda 29-bit UDS, Nissan Consult, Stellantis UDS).
- **Tier 3 — All-module scans + enhanced DTCs:** per-module DTC reads across the era's buses (incl. secondary buses via STN).
- **Tier 4 — Bidirectional/actuator tests:** UDS `$2F`/`$31` or GM `$AE`, gated by `AuthProvider`; safety-classed and preconditioned (leveraging existing `ActiveTestDefinition`/`SafetyClass`).

**Rollout priority:** GM (deepest, reference — extends the LLY work) → Ford (best aftermarket
docs, no gateway pre-2020, MS-CAN via STN) → Stellantis/RAM (AutoAuth model + Cummins/commercial
bridge) → Toyota → Honda (29-bit UDS + K-line gateway quirks) → Nissan. Each OEM lands as: one
profile module + registry line + corpus + extended architecture test — **no shared-file edits
beyond the single registry line.**

---

## 6. Isolation contract (how RULE 1 & RULE 2 are enforced)

| Invariant | Mechanism | Test |
|-----------|-----------|------|
| INV-1 Data not enums | String-keyed IDs; registry of `&'static dyn DiagnosticProfile` | New test: "adding OEM touches only its module + 1 registry line" (diff-scoped) |
| INV-2 Typed seams | `DiagnosticProfile`, `Transport`, `ProtocolClient`, `AuthProvider` traits | Object-safety + trait-boundary tests (extend `wave1_architecture.rs`) |
| INV-3 No cross-OEM ref | Source-scan allowlists per OEM | Extend `architecture.rs`/`architectural_import.rs`: `<oem>` symbols only in `profiles/<oem>/`; `session_runner`/`domain`/GUI reference no OEM symbols |
| INV-4 Frozen decode | Per-OEM/vehicle golden corpus (additive-only) | `corpus_*` tests; bit-exact `f64::to_bits` on signals |
| INV-5 Protocol purity | `protocol/` has no OEM data, no ELM text | New source-scan: `protocol/` imports no `elm`/OEM symbols; move ELM parsing into the ELM backend |

Additional structural guards:

- **Cross-spec bleed fix:** `SpecRegistry::lookup_dtc` and `match_vin` must be OEM/profile-scoped (return ambiguity instead of first-hit); DTC descriptions resolved through the *selected* profile only.
- **Selection ambiguity:** two profiles returning `Exact` already yields no-selection (good) — add specificity ordering so a loose new-OEM matcher can't disable a precise existing one.
- **Registry single-source:** stop reconstructing `with_builtins()` at ~5 call sites; build once and inject (removes the "registration diverges silently" risk).

---

## 7. Testing & validation strategy

1. **Golden corpus per OEM/vehicle** (extends existing tiers): protocol payload (raw wire→bytes), signal decode (bytes→value, bit-exact), DTC decode, selection cases. Seeded from `raw-captures/` via the existing dev-only seeder.
2. **Protocol conformance fixtures:** per transport (ISO-TP multi-frame with flow control, J1850 3-byte header + IFR, K-line init timing, J1939 TP.BAM/RTS-CTS reassembly, DoIP routing activation) — decode/encode round-trips independent of any OEM.
3. **Mock backends per tier:** extend `MockAdapter`/`MockTransport` into per-transport mocks so protocol clients are testable with no hardware.
4. **hw-test matrix expansion:** add `ExpectedVehicle` entries + `TestGroup`s per OEM/protocol; gate by capability flags (`has_j1939`, `has_ms_can`, `has_can_fd`). This is the real-vehicle regression net.
5. **Architecture tests extended per OEM** (INV-3) — required in the same commit as each OEM.
6. **CI cross-OEM guard:** run the *full* corpus on every change; a shared-decoder edit that changes any OEM's golden output fails (INV-4).

---

## 8. Phased roadmap (waves)

Waves are ordered by dependency; each ends with green tests + (where relevant) a real-vehicle
check. Waves P0–P3 are the new obd2-core protocol work; O-waves are OEM rollout; they interleave.

**Phase P — Protocol core (obd2-core)**

- **P0 — Seams & safety (no behavior change):** rename byte-`Transport`→`Link`; introduce `Transport`/`ProtocolClient` traits with the ELM path refactored *behind* them (ELM becomes `ElmBackend` realizing IsoTp/J1850/KLine). Fix cross-spec DTC/VIN bleed (OEM-scoped lookup). Move ELM text parsing out of `protocol/codec.rs` into the ELM backend (INV-5). *Everything still runs on ELM; corpus unchanged.*
- **P1 — UDS + ISO-TP first-class:** real host-side ISO-TP (flow control/BS/STmin) as a `Transport`; `UdsClient` (`$10/$22/$19/$2E/$31/$27/$29/$3E/$85/$14/$2F/$11`). J1979-2 OBDonUDS detection (`$22 F810`) + 3-byte DTC (code+FTB) model. This unlocks all modern OEMs at once.
- **P2 — STN + native CAN backends:** issue `ST` commands (MS-CAN, SW-CAN GMLAN, filters, `STPTO`); add SocketCAN/gs_usb backend (kernel ISO-TP, CAN FD). Backend capability negotiation (§4.2). Unlocks Ford body + GM body + CAN FD (Global B reads).
- **P3 — J1939 + DoIP:** real `J1939Transport` (address claiming, TP.BAM/TP.CM) + `J1939Client` (DM1/DM2/DM3/DM11/DM5/DM24/DM25); curated public SPN/PGN table + optional DA overlay import. `DoIpTransport` (TCP:13400 + HSFZ). Optional J2534/RP1210 FFI backends last.

**Phase O — OEM rollout (obd2-dash), each on top of the profile-migration waves**

- **O0 — Generalize profile model (with Wave 4–6):** open the closed enums (§4.4), add address templates (§4.3), generalize request shape, evict GM leakage from `session_runner`/`domain`. Acceptance: the fixture profile + a second real OEM stub both plug in with zero shared-file edits beyond one registry line.
- **O1 — GM breadth:** extend beyond LLY to GMLAN (2004+ CAN) + Global A + Global B (CAN FD reads). Reuse LLY corpus patterns.
- **O2 — Ford:** SCP (legacy) + Ford UDS (2008+), MS-CAN via STN, `0x7xx` module table.
- **O3 — Stellantis/RAM:** PCI/SCI (legacy, SCI via J2534) + UDS; AutoAuth `AuthProvider`; RAM HD Cummins bridge to J1939.
- **O4 — Toyota / O5 — Honda / O6 — Nissan:** K-line + UDS; Honda 29-bit `18DAxxF1` + K-line/MICU gateway + query serialization; Nissan Consult + AutoAuth.

**Phase A — Authorization**

- **A0 — Read-only everywhere (implicit, done first):** ship the full read surface with no auth.
- **A1 — `AuthProvider` trait + AutoAuth (FCA/Nissan/Mercedes):** gate write-capable services.
- **A2 — `$27`/`$29` per-ECU security state machine; OEM-subscription + NASTF VSP providers.**

**Phase C — Commercial**

- Rides on P3 (J1939) + medium-duty on the light-duty stack (immediate). HD J1939 DM diagnostics, 9-pin adapter support, Allison/Cummins profiles. J1708/J1587 (RP1210) optional/last.

Dependency spine: **P0 → P1 → (P2, P3 parallel)**; **O0 depends on P0**; each O-wave depends on
the protocol clients it uses (O2 Ford UDS ← P1; O2 MS-CAN ← P2; O3 SCI ← J2534 in P3; C ← P3).

---

## 9. Risks & open questions

- **ELM buffer/clone reality:** multi-frame + monitor on clones is unreliable; gate bidirectional + high-rate features on fingerprinted-genuine/STN/native tiers. (Mitigation: capability flags + quirks table.)
- **CAN FD reach:** GM Global B / newest Stellantis/Toyota *reads* need CAN FD hardware — ELM/STN cannot. Communicate to users; SocketCAN/J2534 required for those vehicles.
- **J1939DA licensing:** ship curated public subset + optional user-licensed overlay; never embed a paid DA.
- **Corpus churn on shared-decoder refactors:** generalizing `decode_class2_dtcs` etc. must not perturb LLY golden bits — refactor behind the corpus, add new goldens, never edit existing.
- **DoIP/Euro-brand scope:** out of the seven named OEMs mostly, but the DoIP transport is cheap and future-proofs Toyota/Stellantis 2023+ — build the transport, defer brand profiles.
- **`$29` authenticated reads (2025+):** the "reads always open" assumption erodes; `AuthProvider` must support per-session/per-ECU negotiation, not just a one-time gateway unlock.
- **Open decisions (defaults chosen, revisit):** (1) keep `Manufacturer` enum as display-only vs full string ID; (2) exact split of ISO-TP host-side vs chip-delegated on ELM; (3) whether O0 lands inside the existing Wave 4–6 or as a distinct wave.

---

## 10. Success criteria

1. A 2003 Silverado (VPW), a 2015 F-150 (HS+MS-CAN), a 2012 Camry (CAN), a 2018 Accord (29-bit UDS), a 2020 RAM 2500 (SGW), and a 2022 Silverado HD (Global B/CAN FD) all connect, identify, and return live data + DTCs through the *same* code paths with different profiles/transports.
2. Adding the 7th OEM requires: 1 new module + 1 registry line + 1 corpus dir + 1 architecture-test extension — and the full existing corpus stays green (proving RULE 1).
3. A heavy-duty J1939 truck (9-pin) returns DM1/DM2 DTCs and live SPNs.
4. Read-only diagnostics work with zero authentication on all seven OEMs; write/actuator tests are gated behind `AuthProvider` and refuse cleanly when unauthorized.
5. `protocol/` contains no ELM text handling and no OEM data (INV-5 test passes).

---

*Appendices (per-OEM verified matrices, hardware capability matrix, standards timeline with
sources, security-gateway analysis, commercial/J1939 findings, and open-source prior-art) are
archived in the session research set and will be attached as `appendix-*.md` alongside this spec.*
