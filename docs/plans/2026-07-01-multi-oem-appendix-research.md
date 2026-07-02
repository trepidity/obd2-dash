# Appendix — Multi-OEM Diagnostics Research (US, MY2001–2026)

Companion to `2026-07-01-multi-oem-multi-protocol-architecture-design.md`. Verified,
adversarially fact-checked reference data. Confidence tags are per-row; per-model-year
boundaries are ranges (probe-and-detect at runtime, never assume a hard cutover).

> **Provenance note:** OEM matrices were produced by independent researchers and then
> adversarially re-checked by a second researcher using different sources. Where the checker
> corrected the original, the corrected value is shown. Source URLs are retained in the session
> research set; representative sources are cited inline.

---

## A. Standards & regulatory timeline (the architectural backbone)

| Era | What changed | Scan-tool consequence |
|-----|--------------|----------------------|
| 1996–2007 | Five legacy DLC protocols, OEM-specific | Must auto-detect J1850 VPW/PWM, ISO 9141-2, ISO 14230 KWP2000, ISO 15765-4 CAN |
| MY2003 | CAN first *allowed* for OBD | Early-CAN vehicles appear (per-model) |
| **MY2008** | **ISO 15765-4 CAN mandatory, sole legislated OBD protocol** | 2008+ guarantees CAN on pins 6/14 |
| 2010–2016 | J1979-DA digital annex becomes the PID/MID registry (revised ~annually) | Track DA revisions, not base J1979 |
| 2020+ | CAN FD at the DLC (GM Global B, then Ford/Stellantis/Toyota) | Enhanced/module scans need CAN-FD hardware |
| **MY2023–2026 → MY2027** | **J1979-2 OBDonUDS**: CARB-permitted, then **mandatory 2027** (LD/MD/HD) | UDS SIDs replace classic modes; 3-byte DTCs (code + FTB) |
| MY2026+ | J1979-3 ZEVonUDS for ZEV/PHEV | One UDS engine covers ICE/hybrid/EV generic OBD |

**J1979-2 mode→UDS mapping** (a tool must speak both classic and OBDonUDS through ~2040):
`$01→$22` DIDs `0xF4xx`; `$06→$22` `0xF6xx`; `$09→$22` `0xF8xx`; `$03/$07→$19 42`; `$0A→$19 55`;
`$02→$19 04`; `$04→$14`; `$08→$31 01` RID `0xE0xx`. Protocol detection per ISO 15765-4:2021 §6:
functional probe `$22 F810` (OBDonUDS) vs `$01 00` (classic), 11-bit **and** 29-bit, 500k then 250k,
33 ms N_As timeout.

**UDS services a scan tool needs** (ISO 14229): `$10` session, `$22` read DID, `$19` read DTC
(subfns `01/02/04/06/42/55`), `$2E` write DID, `$31` routine, `$27` seed-key, `$29` authentication
(2020+), `$3E` tester-present, `$85` control-DTC-setting, `$14` clear, `$2F`/`$11` actuator/reset.
**KWP2000 (ISO 14230) equivalents** for 2001–2008 K-line: `$10`, `$21`/`$22` (local/common id),
`$18`/`$17` DTC, `$14` clear, `$27`, `$31`, `$3E` — 1-byte local ids, 2-byte DTCs, K-line init
(5-baud or fast).

**ISO-TP (ISO 15765-2):** SF/FF/CF/FC; STmin `0x00–0x7F`=0–127 ms, `0xF1–0xF9`=100–900 µs; BS=0 =
send all; 4095-byte limit (2016 edition raised to 2³²−1, added CAN FD). Linux kernel ships
`can-isotp` — reference for host-side implementation.

**DTC format (SAE J2012):** 5-char B/C/P/U + control digit + 3 hex; ISO/SAE ranges (P0/P2/U0…) vs
manufacturer ranges (**P1xxx, U1/U2, B1/B2/C1/C2 are OEM-specific** → descriptions must be
OEM-scoped). J1979-2 wire format = 3 bytes (2-byte code + 1-byte Failure Type Byte via J2012-DA).

**Interface APIs:** J2534-1 v04.04 (legacy protocols + CAN + Chrysler SCI); v05.00 (2022) + J2534-2
add CAN FD + DoIP. RP1210 (heavy truck: J1939 + J1708/J1587), Windows DLL only.

---

## B. Per-OEM protocol matrices (verified)

### B.1 General Motors (Chevrolet / GMC / Cadillac / Buick / Pontiac / Saturn / Hummer)

| Years | Physical | Legislated OBD | Enhanced | Secondary buses | Conf. |
|-------|----------|----------------|----------|-----------------|-------|
| 2001–2003 | J1850 VPW ("Class 2"), pin 2, 10.4 kbps | J1979 over VPW | Class-2/J2190 (`$22` data-by-PID, dynamic data, device control); 3-byte J2178 headers (priority + target node + source) | Single shared Class-2 bus, addressed by node ID; GM medium-duty (Kodiak/TopKick) use J1708 instead | High |
| 2004–2007 (transition; first GMLAN = Saturn Ion / 2004 Cadillac XLR) | Mixed: carryover VPW pin 2; new platforms ISO 15765-4 CAN 500k 11-bit pins 6/14 | J1979 over VPW or CAN | VPW: Class-2/J2190. GMLAN: **GMW3110** over ISO-TP — `$10/$1A/$22/$27/$28/$2C/$3E/$A9`(DTCs)`/$AA/$AE`(device control) | Class-2 pin 2 for body on early GMLAN cars; **LS-GMLAN SW-CAN (J2411) pin 1, 33.3k** (83.3k reprogram); HS-GMLAN 500k pins 6/14 | High |
| 2008–2009 | ISO 15765-4 CAN 500k 11-bit pins 6/14 (VPW retired) | J1979 over CAN | GMLAN GMW3110 (not full UDS); diag IDs 0x101 functional, 0x241–0x25F/0x641–0x65F physical, 0x7E0/0x7E8 OBD | LS-GMLAN SW-CAN pin 1 33.3k; MS-GMLAN ~95k pins 3/11 (some) | High |
| 2010–2019 (Global A; overlaps to ~2023) | ISO 15765-4 CAN 500k 11-bit | J1979 over CAN | GMLAN GMW3110 (`$22` 2-byte DIDs, `$A9` DTCs, `$AA` packet, `$AE` device control, `$27` seed/key) | LS-GMLAN pin 1; HS pins 6/14; MS pins 3/11; **late Global A (2019+): Gen-3 SDGM isolates buses from DLC (120Ω across 6/14)** | High |
| 2020–2026 (Global B / VIP; from 2020 Corvette C8, 2021 full-size SUVs, 2022.5 trucks) | **CAN FD 29-bit ~5 Mbps** pins 6/14 + classic CAN for OBD | J1979 over classic CAN (J1979-2 permitted MY2023+) | **UDS ISO 14229 over CAN FD**; rolling seed/key + CAN message auth; secured fns need SDAC keys via Techline/SPS2 | **No LS-GMLAN**; internal multi-CAN + Ethernet backbone (not at DLC); **K56 SDGM isolates all secure networks** | Med |

**Module map (Class-2 nodes):** ECM `0x10`, FICM `0x11`, TCM `0x18`, EBCM `0x28`/`0x29`, BCM `0x40`,
SDM/airbag `0x58`, IPC. **Gateway:** Global B has no AutoAuth broker — sanctioned access via GM
SPS2/Techline Connect. Reads/many actuator tests work on Global B with CAN-FD hardware; programming
needs the GM security-key server.

### B.2 Ford (Ford / Lincoln / Mercury)

| Years | Physical | Legislated OBD | Enhanced | Secondary buses | Conf. |
|-------|----------|----------------|----------|-----------------|-------|
| 2001–2003 (carryover to MY2007) | **J1850 PWM/SCP 41.6k pins 2/10**; ISO 9141-2 pin 7; **UBP pin 3 (~9600 bps)**; FEPS 18V pin 13 | J1979 over PWM | SCP enhanced (J2178 headers, In-Frame-Response); `$22` 2-byte PIDs, KOEO/KOER self-tests | SCP 2/10 (PCM); ISO 9141 pin 7 (ABS/SRS/GEM); UBP pin 3 (body: GEM/cluster/EATC) | High (exact SCP addr bytes: Low) |
| 2004–2007 (per-model CAN phase-in: 2004 F-150 first) | ISO 15765-4 CAN 500k 11-bit pins 6/14; **MS-CAN 125k pins 3/11** (~2003–04+); SCP holdouts to MY2007 | J1979 over CAN or PWM | Transitional KWP/early-UDS over ISO-TP; 0x7E0 PCM + 0x7xx | HS-CAN 6/14 (powertrain); **MS-CAN 3/11 (body) — NOT reachable by stock ELM327** | Med |
| 2008–2019 | ISO 15765-4 HS-CAN 500k 11-bit pins 6/14; MS-CAN 125k pins 3/11; hybrids use HS2-CAN 500k on 3/11 | J1979 over CAN | **Ford UDS (ISO 14229)**, 16-bit DIDs; PCM 0x7E0, ABS 0x760, BCM 0x726, RCM 0x737, IPC 0x720, PSCM 0x730 (resp = req+8) | HS-CAN 6/14; MS-CAN 3/11 (BCM/SYNC/HVAC/doors) — needs STN or HS/MS switch | High |
| 2020–2026 (FNV2/FNV3; GWM-gated from ~2018) | Classic 500k CAN at tester; behind **Gateway Module (GWM)** routing to HS1-4/MS/FD-CAN; 2021+ dedicated DIAG1 bus | J1979 over CAN (J1979-2 permitted MY2023+) | Ford UDS via GWM; FDRS factory tool | Internal HS-CAN1-4, MS-CAN, FD-CAN, Ethernet — only via GWM; no DoIP at DLC through MY2026 | Med |
| Medium duty F-650/750 2004–2015 (Navistar-built) | **J1939 + J1708/J1587 at 9-pin Deutsch**; J1962 for Ford body only | J1939 DM1/DM2 | Cummins INSITE / Cat ET; Ford "MD Truck" for body | 9-pin: J1939 (C/D) + J1708 (F/G) | Med |

**2016+ F-650/750 are Ford-built → standard light-duty J1962 OBD-II.** E-series follow light-duty
rows. **Ford gateway (2020/21+ "Security Link")** blocks bidirectional; FDRS account required (no
AutoAuth). Reads/DTC scans still work.

### B.3 Stellantis / FCA (RAM / Dodge / Chrysler / Jeep)

| Years | Physical | Legislated OBD | Enhanced | Secondary buses | Conf. |
|-------|----------|----------------|----------|-----------------|-------|
| 2001–2003 | J1850 VPW ("PCI") pin 2 10.4k | J1979 over VPW | Chrysler proprietary over PCI/CCD (DRB III); **SCI** = separate serial for PCM reflash/enhanced PCM | PCI (pin 2); CCD (twisted pair ~7812 baud, pins 3/11, phasing out); SCI (discretionary pins) | Med |
| 2004–2007 (transition; 2004 Durango first CAN) | Mostly VPW/PCI; early CAN (2005 LX cars, Grand Cherokee) 11-bit 500k pins 6/14 | J1979 over VPW or CAN | DRB III / early StarSCAN; KWP2000 + emerging UDS on CAN; SCI reflash on legacy PCMs | Legacy PCI+CCD+SCI; early-CAN: CAN-C + CAN-B/IHS | Med |
| 2008–2017 | ISO 15765-4 CAN 11-bit 500k pins 6/14 | J1979 over CAN | **wiTECH UDS** (`$22` DIDs, `$19`/`$18` DTCs, `$2F`/`$31` routines, `$27`); | Diagnostic CAN-C → TIPM/Central Gateway → CAN-C 500k + CAN-B 83.3k / CAN-IHS 125k + LIN | High |
| 2018–2020 | CAN 11-bit 500k pins 6/14 behind **Secure Gateway (SGW/SGM)** | J1979 over CAN (**reads open through SGW**) | UDS via wiTECH 2.0; **writes/actuation/clear gated behind SGW → AutoAuth (~$50/yr)** | SGW-fronted; internal CAN-C/CAN-IHS/CAN-B | High |
| 2021–2026 | Legacy: CAN behind SGW. New (Atlantis High WL Grand Cherokee 2021+, STLA Wagoneer): **CAN FD + DoIP/Ethernet, Mopar Diagnostic Pod (MDP)** | J1979 over CAN; newer → J1979-2 over CAN FD/DoIP | UDS over CAN FD/DoIP via wiTECH+MDP; **SFD** (Secure Feature Delivery) per-session online auth | SGW/SFD-fronted; CAN FD backbone + Ethernet (DoIP) | Med |

**SGW split (the key design fact):** reads (ID, DTC read, live PIDs, readiness) **open**; writes/
clears/actuator tests **gated** → AutoAuth. RAM HD (Cummins) = OBD-II at J1962 for the truck +
J1939 for engine on chassis-cabs (4500/5500).

### B.4 Toyota / Lexus / Scion

| Years | Physical | Legislated OBD | Enhanced | Notes | Conf. |
|-------|----------|----------------|----------|-------|-------|
| 2001–2003 | **ISO 9141-2 K-line pin 7** (some ~9.6k, init addr 0x8A); late-2003 ISO 14230. **No J1850.** | J1979 over K-line | M-OBD (Techstream), KWP/Toyota `$21` local-id reads; blink-code via TC/TS terminals | DLC3 terminals TC(13)/TS(12); no OEM CAN at DLC | Med |
| 2004–2007 (transition; 2004 Prius NHW20 CAN) | K-line **or** ISO 15765-4 CAN 11-bit 500k pins 6/14 | J1979 over K-line or CAN | `$21` local-id reads (e.g. `21 07`); KWP semantics over K-line or ISO-TP | Auto-detect per VIN | Med |
| 2008–2013 | ISO 15765-4 CAN 11-bit 500k pins 6/14 | J1979 over CAN (0x7E0/0x7E8, 0x7DF) | M-OBD over ISO-TP: `$21` locals + actuator tests | Internal buses behind central gateway; no MS/SW-CAN at DLC | High |
| 2014–2019 (TNGA rollout) | ISO 15765-4 CAN | J1979 over CAN | Techstream; legacy `$21` **+ increasing UDS** `$22`/`$19` per module | Central gateway ECU | Med |
| 2020–2022 | Classic CAN for OBD; **CAN FD** on newer TNGA/EV (bZ4X) | J1979 over classic CAN | Techstream/GTS+ UDS (`$22`/`$19`/`$2F`/`$31`); **SecOC** msg auth ~2020+ | SecOC blocks actuation/module-join, **not reads** | Med |
| 2023–2026 | CAN FD + **DoIP** (ADAS vehicles) | J1979 over CAN (J1979-2 trend) | UDS over DoIP/CAN FD via GTS+; ECU Security Key server-side for module replacement | Zonal/domain buses; GTS+ needed (Techstream lacks DoIP) | Low |

Security concentrated on immobilizer/key/module-replacement via NASTF VSP + GTS+; routine reads
and most enhanced reads remain open.

### B.5 Honda / Acura

| Years | Physical | Legislated OBD | Enhanced | Notes | Conf. |
|-------|----------|----------------|----------|-------|-------|
| 2001–2005 | **ISO 9141-2 K-line pin 7**; SCS service line pin 9 (short to gnd = blink mode); immobilizer pin 13 | J1979 over K-line | **HDS** proprietary over K-line (KWP-ish); MIL blink fallback | Internal F-CAN(500k)+B-CAN(33.3k) from 2003 Accord but **NOT the DLC OBD interface** | Med |
| 2006–2007 (CAN OBD phase-in: Civic MY2006 first) | CAN cars: ISO 15765-4 **29-BIT** 500k pins 6/14; others still K-line | J1979 over 29-bit CAN or K-line | HDS; on CAN: **29-bit `0x18DAxxF1` addressing**; K-line still used for body via gateway | Per-VIN one or the other — probe 11-bit + 29-bit + ISO 9141-2 | Med |
| 2008–2015 | ISO 15765-4 CAN 500k pins 6/14 (11-bit **and** 29-bit across fleet); **K-line pin 7 persists** for HDS→MICU body diag | J1979 over CAN | HDS/i-HDS; **UDS 29-bit `18DAxxF1`** (ECM `18DA10F1`, meter `18DA60F1`); K-line HDS for MICU/B-CAN | **B-CAN gatewayed via MICU over K-line — never at DLC**; B-CAN DTCs invisible to generic tools | High |
| 2016–2021 | ISO 15765-4 classic CAN 500k (no CAN FD in this window) | J1979 over CAN | **UDS ISO 14229** on modern modules (`$22` DID `F181`/`F112` at `18DAxxF1`); carryover HDS on older; i-HDS/DST-i/MVCI/J2534 | Classic CAN only (per opendbc fingerprints) | Med |
| 2022–2026 | **CAN FD from MY2023** (Accord/CR-V/Pilot); Civic 11g stays classic CAN | J1979 over CAN (J1979-2 from MY2027) | UDS predominant (`$10/$22/$19/$2E/$2F/$31/$27`) at `18DAxxF1`; keys via NASTF VSP + Honda subscription | **CAN-FD gateway drops/serializes some OBD queries to camera/radar — serialize tester-present**; no AutoAuth | Med |

**Critical:** Honda uses **29-bit `18DAxxF1`** (not 11-bit) for enhanced/UDS. **Acura ZDX / Honda
Prologue (2024+) are GM Ultium → handle under a GM Global-B profile, not Honda.**

### B.6 Nissan / Infiniti

| Era | Years | Physical | Legislated OBD | Enhanced (tool era / SIDs / addressing) | Conf. |
|-----|-------|----------|----------------|-----------------------------------------|-------|
| Pre-OBD Consult | ~1989–1995 | 14-pin round DDL | OBD-I | **Consult gen 1**: proprietary 3-wire serial 9600 baud, init `FF FF EF`→`0x10`, inverted-cmd echo; opcodes `5A` read / `D0` info / `D1` DTC / `C1` clear / `F0` terminate | High |
| Consult-II K-line | ~1996–2006 | J1962; K-line pin 7, L-line pin 15 | ISO 9141-2 / ISO 14230-4 | **Consult-II** = **KWP2000 on K-line 10.4k fast-init, tester `0xFC` / ECU `0x10`**; `$1A` ident, `$21` read-local-id (enhanced PIDs), `$18` DTC, `$14` clear, `$30` I/O control | High |
| CAN transition | ~2005–2007 | Mixed CAN (6/14) + K-line | ISO 15765-4 11-bit 500k phasing in | **Consult-III** (2007); KWP2000-on-CAN | Med |
| Universal CAN / III+ | ~2008–2019 | ISO 15765-4 CAN 11-bit 500k pins 6/14 | J1979 over CAN | **Consult-III+**; **hybrid `$21` (legacy) + `$22`/`$19`/`$2E`/`$31`/`$27` (UDS)**; **Nissan proprietary session `$10 C0`** for some modules; IDs: ECM 7E0/7E8, BCM 745/765, IPDM 74D, EPS 742, IPC 743 (platform-dependent) | High (CAN); Med (per-module SID) |
| Secure Gateway | ~2020–2023 | CAN (+ CAN FD newest) behind **SGW** (Sentra 2020+, Rogue 2021+, Pathfinder 2022+) | ISO 15765-4 | Consult-III+/4; reads open, writes/tests gated → **AutoAuth**; NATS gates key/start not reads | High |
| DoIP / CAN FD | ~2023–2026 | **DoIP (ISO 13400) + CAN FD** behind central gateway | ISO 15765-4 via gateway (J1979-2 trend) | **Consult 4 + VI3**; full UDS; Ariya 23+, Rogue 24+, others 25+ | High (platform); Med (UDS detail) |

Leaf exposes multiple CAN at the DLC (Car-CAN 6/14, EV-CAN 12/13, AV-CAN 3/11). NATS immobilizer:
BCM/IPDM stores encrypted out-code → 4-digit PIN (rolling 20-digit post-2013); separate from
diagnostic reads.

---

## C. Interface hardware capability matrix

| Tier | Hardware | Covers | Cannot do | Latency |
|------|----------|--------|-----------|---------|
| 1 | ELM327 AT serial (USB/BT/BLE/WiFi) | all 5 legacy + CAN 11/29-bit 250/500k + basic J1939 (proto A) | MS-CAN, SW-CAN, CAN FD, DoIP; 512B buffer overflow on monitor; clones frozen at v1.4 | BT-Classic ~15 PID/s; WiFi ~115ms ping (worst) |
| 1+ | STN/OBDLink (SX/EX/MX+/CX) | ELM superset + **MS-CAN (STP 51-54)** + **SW-CAN GMLAN 33.3k (STP 61-64)** + J1939 presets + STPX 4KB + RAM filters | CAN FD (no OBDLink does FD); one CAN channel active at a time | USB fastest, 2 Mbps UART |
| 2 | SocketCAN + `can-isotp`/`can-j1939` (Linux); gs_usb/candleLight (WinUSB/libusb) | frame-level CAN, parallel ISO-TP, **CAN FD** (CANable 2.0) | no K-line/J1850 (2008+ CAN only) | sub-ms |
| 3 | DoIP/HSFZ (ENET cable + NIC) | UDS over TCP:13400 (BMW HSFZ 6801) | non-Ethernet vehicles | Ethernet-class |
| 4 | J2534 pass-thru (Tactrix, CarDAQ, Macchina-Rust) | legacy + CAN + **Chrysler SCI**; v05.00 adds CAN FD/DoIP | Windows DLL ecosystem (registry discovery) | device-dependent |
| 4 | RP1210 (NEXIQ, DG, Noregon) | J1939 + **J1708/J1587** | Windows DLL only; no Linux | device-dependent |

**Clone defense:** fingerprint via `ATZ`/`ATI`/`AT@1` + behavior probes (`ATCRA` support); keep a
quirks table; gate bidirectional/high-rate on genuine-v1.4+/STN/native tiers. **Multi-ECU on ELM:**
`ATH1` + `ATCRA` filter + physical `ATSH` per ECU; 29-bit needs `ATCP`+`ATSH`.

---

## D. Security gateways & authorization (2018–2026)

**Dominant pattern: reads open, writes gated.** A read-only product needs zero auth across all
seven OEMs.

| OEM | Mechanism | Blocks | Path |
|-----|-----------|--------|------|
| Stellantis/FCA | SGW (2018+, some 2017) | writes, clears, actuation, coding | **AutoAuth ~$50/yr** (federated) |
| Nissan/Infiniti | Central SGW (2020+ select) | writes/tests | **AutoAuth** |
| Mercedes | SGW (2018+) | writes/tests | AutoAuth |
| VW/Audi | OEM token (2018+) | writes/tests | ~$120 token (not AutoAuth) |
| GM | Global B/VIP (2020+): CAN FD + SDGM isolation + 12-byte `$27` | programming (needs key server); many reads/actuator tests work | GM SPS2/Techline (no broker) |
| Ford | "Security Link" (2020/21+) | bidirectional output controls | FDRS account (no broker) |
| Toyota | SecOC + NASTF VSP (2020+) | key/immobilizer/module-replace | GTS+ / NASTF VSP |
| Honda | NASTF VSP (2023+) | key/immobilizer/secure reprog | i-HDS + NASTF |

**Cross-OEM identity:** NASTF SDRM/VSP (keys/immobilizer, ~$425/2yr). **UDS `$27` seed-key** →
**`$29` certificate auth** trend under UN R155 / ISO 21434 (NHTSA CSMS guide, July 2026 US
alignment). Right-to-repair (MA/ME 2025–26) = telematics data mandates, **not** gateway-unlock
mandates. **Do not depend on 12+8 hardware bypass** for a legitimate product.

**Design:** `AuthProvider` trait, per-session/per-ECU capability negotiation (supports both `$27`
and `$29`), default read-only when unauthenticated. Providers: `AutoAuth` (FCA/Nissan/Mercedes),
`Token` (VW/Audi), `OemSession` (Ford/GM/Toyota/Honda), `NastfVsp` (keys).

---

## E. Commercial vehicles / J1939

Three worlds: **(1) Heavy-duty J1939** — 29-bit CAN 250k→500k (green Type-II 9-pin from ~MY2016);
needs J1939-81 address claiming, J1939-21 transport (TP.BAM + TP.CM RTS/CTS ≤1785 B), J1939-73
diagnostics (DM1/DM2 DTCs, DM3/DM11 clear, DM5 readiness, DM7/DM8/DM30 tests, DM24/DM25 supported
SPNs/freeze; 4-byte SPN+FMI+OC, **CM bit for pre-2010** SPN byte-order). **(2) CARB HD-OBD (2010/2013)**
permits *either* J1939 *or* ISO 15765-4 → Cummins/Detroit/Paccar/Navistar = J1939 (9-pin);
Volvo/Mack = 16-pin (J1939+J1587+ISO 15765/UDS). **(3) Medium-duty** (RAM 4500/5500, F-450/550,
GM HD, E-series) = **standard light-duty ISO 15765-4 OBD-II — works with the LD stack now.**

- **J1939DA licensing:** database is paid, per-revision. Ship curated public SPN/PGN subset (FMS
  standard + public fault lists); support user-licensed DA→JSON/DBC overlay import (`pretty_j1939`
  model). Never embed a paid DA.
- **J1708/J1587** (~1985–2013 tail): MID/PID/SID+FMI, separate tables; needs RP1210 hardware.
- **Allison:** GM-pickup LCT1000 = GM Class-2/GMLAN over OBD-II; commercial-chassis WTEC = J1587
  (pre-2006) → J1939 (4th-gen+). Generic tools read Allison DTCs over J1939; shift-adapt/flash need
  Allison DOC.
- **Adapters:** SocketCAN `can-j1939` (Linux) or vendor CAN; ELM/STN do basic J1939 read only.

---

## F. GM LLY Duramax deep-dive (reference implementation)

Source: existing repo implementation (`crates/obd2-dash/src/profiles/gm/lly.rs`,
`gm_class2.rs`, `gm_enhanced.rs`) + the `reference_lly_gm_class2_dids.md` project memory. The LLY
profile is the **deepest reference** and the pattern every other OEM's enhanced tier follows. (The
supplementary public-DID research agent hit a transient API error; the data below is already
encoded in the repo and memory and is sufficient — a fuller public-DID sweep can be re-run on
request.)

**Platform:** 2004.5–2005 GMT800 Chevrolet Silverado / GMC Sierra 2500HD/3500HD, 6.6L LLY Duramax.
**Critical fact:** ECM and TCM live on **GM Class 2 (SAE J1850 VPW, 10.4 kbps) — NOT CAN** on this
platform. This is why the profile declares `ALLOWED_PROTOCOLS = [Protocol::J1850Vpw]` only.

**Access convention:** Mode `$22` DID read to ECM header `6C 10 F1` (priority `0x6C`, target node,
tool source `0xF1`) with a trailing `01` selector byte, e.g. `6C10F1 22 1543 01`. Enhanced DTCs via
Mode `$19 FF FF 00` → `$59` reply (decoder in `gm_class2.rs`).

**Module map (Class-2 nodes):** ECM `0x10`, FICM `0x11`, TCM `0x18` (Allison 1000; header `6C 18 F1`;
trans temp DID `0x1940`), EBCM `0x29`, BCM `0x40`, SDM/airbag `0x58`.

**Public DID list** (ScanGauge X-Gauge / EFILive, each confidence + provenance tagged in the profile):

| DID | Signal | Scaling (ScanGauge MTH, vendor-confirmed) | Confidence |
|-----|--------|-------------------------------------------|------------|
| `0x1540`/`0x1543` | VGT vane desired/actual (%) | ×100/255 | High (ScanGauge) |
| `0x163D`/`0x163E` | Fuel rail pressure desired/actual | ×14.5 (⚠ 8-bit RXD caps ~3700 psi; prefer std PID `0123` for actual) | High |
| `0x1193`–`0x119A` | Injector pulse width 1–8 (ms) | ×1.53 | High |
| `0x162F`–`0x1636` | Injector balance rate 1–8 (mm³) | ×0.15625 − 5120 | High |
| `0x1470` | Oil pressure (psi) | ×0.58 | High |
| `0x1940` | Transmission temp (°F, **TCM** node `0x18`) | ×1.8 − 40 | High |
| `0x1542` | Desired MAP / boost target (kPa) | provisional — live-returns data, scaling unconfirmed | **Low (provisional)** |
| `0x1251`/`0x119D` | Barometer (V6 vs V8 variant) | generic GM-VPW | Med |

Registry must store both **RXF** (receive filter) and **RXD** (bit-width: `3008`=8-bit, `3010`=16-bit).
Underboost analysis = desired MAP (`0x1542`) − actual MAP (std PID `0x0B`); baro cancels.

**Actuator tests / security:** the profile's VGT active test is currently `Locked` (evidence-only) —
the generalized `ActiveTestCommand` path (design §4.4) will unlock GM `$AE` device control (VGT sweep,
injector balance/cutout) behind the `$27` seed-key + `AuthProvider` gate, safety-classed and
preconditioned. Reference behavior: Tech2 + TIS2000, EFILive. Class-2 timing quirks: P3 inter-message
gap, tester-present `$3E` cadence, 4x high-speed mode entry.

---

## G. Open-source prior-art (architecture lessons)

Maintenance state verified via GitHub API 2026-07-01. **The most important finding: the trait
stack this design recommends is the same one `ecu_diagnostics` converged on independently** — strong
evidence the layering is right.

### G.1 `ecu_diagnostics` (rnd-ash) — the closest analog

- **State:** v0.107.2, edition 2024, **GPL-3.0**, actively maintained (2026-06), 224★.
- **Layering (adopt this):** Hardware API (discovery + channel factory) → Channel traits (`PayloadChannel` → `IsoTPChannel`; `PacketChannel<CanFrame>` → `CanChannel`) → `DynamicDiagSession` generic over `DiagProtocol<NRC>` + `EcuNRC` (UDS/KWP2000/OBD2 chosen **at runtime**).
- **Key move:** the diag server depends on the `IsoTPChannel` **trait**, never a concrete adapter. Any backend that presents an ISO-TP channel (hardware or a **software ISO-TP shim** for raw-only backends) plugs in identically. Channels are **RAII (close on Drop)**. `HardwareCapabilities { iso_tp, can, kline, kline_kwp, sae_j1850, sci, ip }` is a runtime bitfield; discovery is a `HardwareScanner` trait.
- **Backends:** J2534, SocketCAN, SLCAN, PCAN (+ sw-ISO-TP), VW TP2.0. **D-PDU is a doc-comment stub only** (lesson: keep capability flags honest/test-enforced). **DoIP WIP. ELM327: zero code — deliberately unsupported.**
- **Why ELM327 is rejected:** it exposes the bus only through an AT-command text REPL that hides raw IDs, timing, CAN error frames, and does its own ISO-TP — fine for PID polling, unusable for UDS/KWP depth. *This is exactly why our design pushes ELM to a backend and owns ISO-TP above it.*
- **`automotive_diag`** (Apache-2.0, `no_std`, active): forked from `ecu_diagnostics` at its last MIT commit so protocol tables stay permissive. Its **`ByteWrapper<T> = Standard(T) | Extended(u8)`** is the single best OEM-isolation primitive: every protocol byte decodes to a known variant *or* preserves the OEM-specific raw value losslessly, so standard tables never need editing per OEM.

### G.2 `automotive` crate (I-CAN-hack) — async done right

MIT, active (2026-03). `CanAdapter` (blocking) → `AsyncCanAdapter` (async wrapper, matches TX ACKs) →
ISO-TP → `UDSClient` generic over a `TransportLayer` trait. **Blocking hardware, async surface** keeps
backend authors simple while the client gets parallel multi-ECU for free. Backends are **feature-gated,
default-off** (SocketCAN, panda, J2534, Vector) — never compile a backend you don't use.

### G.3 Python ecosystem — cleanest separation-of-concerns

- **`python-can`** (LGPL, very active): `BusABC` + a **registry of entry-point backends** = "adding a backend can't break another" at the transport tier.
- **`udsoncan`** (MIT, active) — **the cleanest OEM-isolation model in the survey:** the protocol state machine is fixed/shared; each OEM/ECU is a *bundle of injected `DidCodec`s + a `security_algo` callable + `ClientConfig`* handed to the same engine. Two OEMs are two values; neither can alter the other. **Adopt this injection model directly.**
- **`python-OBD`** (GPL): PIDs are **data (command table + decoder fn)**, not code branches; `watch(cmd, callback)` live model distinct from blocking query.
- **Caring Caribou** (GPL): each technique is a drop-in module in a registry — new capability = new file, not a core-dispatch edit.

### G.4 Definition formats — what they buy, and the sprawl traps

- **opendbc** (comma.ai, MIT, most active in survey): **structural per-brand isolation** — everything for a car in `opendbc/car/<brand>/`; a DBC preprocessor composes brand+model to dedup; **safety is a separately-compiled, independently-tested C tier (panda, MISRA + 100% coverage + mutation + per-car tests).** This is the real-world proof of "adding a car cannot break another," enforced by directory isolation + an independent critical tier. Also the primary source confirming Honda 29-bit `18DAxxF1`.
- **ODX (ISO 22901) / MVCI:** buys a complete machine-readable whole-lifecycle ECU description; **costs** heavy complexity and — critically — *"the flexibility of ODX led to different ODX dialects in practice."* → adopt the *shape* (variant → services → params-with-scaling → connection/protocol) as a **small, strict, versioned, self-owned schema** (OVD's JSON approach), not full ODX.
- **ddt4all** (Renault): the canonical **external-DB failure mode** — its entire capability was bound to Renault's DDT2000 XML DB, discontinued 2022, so it "will never be updated again." → never source OEM definitions from a single external proprietary DB.

### G.5 J1939 open stacks + the DA licensing problem

- **Linux kernel `can-j1939`** (mainline since 4.19): implements J1939-21 transport (≤1785 B, transparent fragmentation) + J1939-81 network management; **userspace sends payloads, not frames.** → J1939 addressing/TP belong in a transport layer decoupled from PGN/SPN meaning.
- **Rust:** `yorickdewid/j1939` (GPL, `no_std`, hardcoded defs, no DA dependency), `voltage-j1939` (Apache-2.0, ~60 SPN built-in), `canparse` (DBC-driven but stale 2023).
- **J1939DA licensing:** the Digital Annex is paid and covers only standardized PGNs/SPNs. → ship the open transport engine + a curated public subset; make the PGN/SPN dictionary a **swappable user-supplied DBC/JSON overlay** (never bake the licensed DA into the repo).

### G.6 Synthesis — lessons applied to this design

1. **Trait stack** (Hardware/backend → Channel traits with sw-ISO-TP shim → runtime-selected DiagProtocol → OEM profile) — exactly the design's §4.1 layering, validated by `ecu_diagnostics` + `automotive`.
2. **OEM = additive data + injected callables** (`ByteWrapper` Standard/Extended + udsoncan-style injected codecs/security-algo + opendbc per-directory isolation) — this is the concrete mechanism behind INV-1/INV-2.
3. **Definitions in a tight self-owned versioned schema**, serde-validated so a malformed OEM file fails at load *in isolation* (never corrupting a shared table). J1939 dict = user-supplied overlay.
4. **Independent testability = non-interference:** each backend gets channel-trait contract tests; each OEM profile gets fixture-driven tests (recorded req/resp; `ecu_diagnostics` ships an ECU simulator) so profiles are testable without hardware *and without each other* — this is INV-3/INV-4 operationalized.
5. **Avoid:** ELM327 as primary transport; dishonest capability flags; copyleft on the reusable definitions layer (GPL forced the `automotive_diag` fork — consider splitting a permissive `no_std` definitions crate); single-external-DB dependence; timing/security/DID logic inside the shared engine (inject it).
