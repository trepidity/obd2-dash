# Phase P0 — Protocol-Core Seams & Safety Implementation Plan

> **For agentic workers:** If your environment provides them, use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to drive this plan task-by-task. If those skills are unavailable, execute the checklist steps directly in order — each task is a self-contained TDD cycle. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the protocol-isolation seams in `obd2-core` (rename the byte-level trait, introduce the framed `Transport` + `ProtocolClient` traits with ELM behind them, evict ELM text-parsing from `protocol/`, and fix the cross-spec VIN/DTC bleed) with **zero behavior change** — LLY path and golden corpus stay green throughout.

**Architecture:** `obd2-core` today is one ELM327 text-mode `Adapter` over a byte-level `Transport`, with ELM ASCII parsing living inside `protocol/codec.rs`. P0 renames the byte trait to `Link`, adds a new framed-PDU `Transport` trait (transport-layer-neutral — no dependency on `adapter`) and a `ProtocolClient` trait (generic over `Transport`), moves all ELM ASCII handling out of `protocol/` into `adapter/elm_codec.rs` so `protocol/` becomes wire-dialect-pure (invariant INV-5), and makes spec lookup ambiguity-aware/spec-scoped so one OEM can't shadow another (RULE 1). No Session rewiring — that is P1.

**Tech Stack:** Rust 2021 (`obd2-core` rust-version 1.75), `tokio`, `async-trait`, `thiserror`, `serde`, `serde_yaml`. Two separate Cargo workspaces: `obd2-core` (library, at `/Users/jared/Projects/HaulLogic/obd2-core`) and `obd2-dash` (app, at `/Users/jared/Projects/HaulLogic/obd2-dash`); `obd2-dash` consumes `obd2-core` as a dependency.

## Global Constraints

- **No behavior change.** Every existing test in both workspaces must still pass after each task, except tests edited only because a symbol was renamed. The golden corpus (`obd2-dash/crates/obd2-dash/tests/corpus/`) and the LLY path must produce bit-identical output.
- **Conservative dependencies.** Add no new crates. (`serde_yaml` is already a workspace dep.)
- **Respect the hardware boundary and message-driven architecture** (`AGENTS.md`): no framework churn, explicit control flow, allocation-light hot paths.
- **Commit after every task** with a conventional-commit message on branch `docs/multi-oem-protocol-architecture` (or a dedicated `feat/p0-protocol-seams` branch). Do **not** commit to `master`.
- **Test commands run per-workspace.** obd2-core: run from `/Users/jared/Projects/HaulLogic/obd2-core`. obd2-dash: run from `/Users/jared/Projects/HaulLogic/obd2-dash`. After any obd2-core change, run BOTH.
- **No `unsafe`.**

---

## Verified API facts (read before writing any test)

These were confirmed against the current source; the plan's code uses them literally:

- `SpecIdentity` fields: `name: String`, `model_years`, `makes`, `models`, `engine: EngineSpec`, `transmission`, `vin_match: Option<VinMatcher>`. **There is no `id` field.** (`vehicle/mod.rs:108`)
- `EngineSpec` has a `code: String` (e.g. `"LLY"`) — the existing tests assert on `identity.engine.code`.
- `DtcEntry` fields: `code`, **`meaning: String`** (not `description`), `severity`, `notes`, `related_pids`, `category`. (`vehicle/mod.rs:432`)
- `DtcLibrary { ecm, tcm, bcm, network: Vec<DtcEntry> }` with `pub fn lookup(&self, code) -> Option<&DtcEntry>`. (`vehicle/mod.rs:420-451`)
- `SpecRegistry { specs: Vec<VehicleSpec> }` (private field), `pub fn new()`, `pub fn with_defaults()` (loads embedded Duramax spec), `pub fn specs(&self) -> &[VehicleSpec]`. **No public `register`/`push`.** Inline tests live in the same module, so a `SpecRegistry { specs: vec![...] }` struct literal is allowed in tests; specs are `Clone`, so tests clone the embedded spec rather than fabricating one.
- Embedded Duramax spec matches VIN `1GCHK23224F000001` with `identity.engine.code == "LLY"`. (`vehicle/mod.rs:930-934`)
- `MockTransport::new()` + `expect(&mut self, command: &str, response: &str)`. (`transport/mock.rs:20,29`)
- `Elm327Adapter::new(transport: Box<dyn Transport>)` (becomes `Box<dyn Link>` after Task 3); `fn protocol_family(&self) -> BusFamily` is currently private (`elm327.rs:206`) — Task 5 makes it `pub(crate)`.

---

## File structure (what P0 creates or changes)

**obd2-core:**
- `crates/obd2-core/src/vehicle/mod.rs` — MODIFY: `match_vin` one-pass ambiguity check + new `match_vin_all`; add `VehicleSpec::lookup_dtc`; deprecate `SpecRegistry::lookup_dtc`; update one existing test.
- `crates/obd2-core/src/transport/mod.rs` — MODIFY: rename trait `Transport` → `Link`; `pub mod framed;`.
- `crates/obd2-core/src/transport/{serial,ble,mock,logging}.rs` — MODIFY: `impl Transport for X` → `impl Link for X`.
- `crates/obd2-core/src/transport/framed.rs` — CREATE: transport-neutral framed-PDU `Transport` trait + `TransportRequest` (no `adapter` dependency).
- `crates/obd2-core/src/adapter/mod.rs` — MODIFY: `transport_mut -> Option<&mut dyn Link>`; `pub mod elm_codec;`; `pub mod elm_transport;`.
- `crates/obd2-core/src/adapter/elm327.rs` — MODIFY: `Box<dyn Link>`; call `elm_codec::…`; make `protocol_family` `pub(crate)`.
- `crates/obd2-core/src/adapter/elm_codec.rs` — CREATE: ELM ASCII→bytes layer moved out of `protocol/codec.rs`.
- `crates/obd2-core/src/adapter/elm_transport.rs` — CREATE: `ElmTransport` (impl of framed `Transport` over `Elm327Adapter`) — the adapter→transport bridge lives in the adapter layer, not `transport/`.
- `crates/obd2-core/src/protocol/codec.rs` — MODIFY: delete the ELM-text functions/tests (keep pure frame decoders).
- `crates/obd2-core/src/protocol/client.rs` — CREATE: `ProtocolClient` trait + `RequestKind`/`DiagResponse` + `J1979Client<T: Transport>` (generic, **no ELM reference**).
- `crates/obd2-core/src/protocol/mod.rs` / `lib.rs` — MODIFY: module wiring + `pub use`.
- `crates/obd2-core/tests/architecture.rs` — CREATE: recursive source-scan test that `protocol/` is ELM-free (INV-5).
- `crates/obd2-hw-test/src/main.rs:216` — MODIFY: `Box<dyn Transport>` → `Box<dyn Link>`.

**obd2-dash (NOT none — corrected):**
- `crates/obd2-dash/tests/protocol_payload_corpus.rs:4` and `crates/obd2-dash/tests/seed_corpus.rs:11` — MODIFY (Task 4): change the import of `decode_elm_response_payload_for_command` from `obd2_core::protocol::codec` to `obd2_core::adapter::elm_codec`. (`BusFamily` stays in `protocol::codec` and is unaffected.)

---

## Task 1: VIN spec-match ambiguity detection (RULE 1, live path)

**Files:**
- Modify: `crates/obd2-core/src/vehicle/mod.rs:505-512` (`match_vin`)
- Test: inline `tests` module in `crates/obd2-core/src/vehicle/mod.rs` (near existing `match_vin` tests ~929-942)

**Interfaces:**
- Consumes: `VehicleSpec`, `VinMatcher::matches`, `SpecRegistry::{with_defaults, specs}`.
- Produces: `SpecRegistry::match_vin(&self, vin) -> Option<&VehicleSpec>` (same signature; new semantics: `Some` only for a single unambiguous match, implemented one-pass with no allocation) and `SpecRegistry::match_vin_all(&self, vin) -> Vec<&VehicleSpec>` for callers that want the full set. `session/mod.rs:488` keeps calling `match_vin` unchanged.

- [ ] **Step 1: Write the failing tests** (clone the embedded spec — no fabricated `VehicleSpec`, no nonexistent `register`)

Add to the `tests` module in `vehicle/mod.rs`:

```rust
#[test]
fn match_vin_returns_none_when_two_specs_match_same_vin() {
    // Two specs whose matchers both accept the VIN must not silently shadow (RULE 1).
    let base = SpecRegistry::with_defaults();
    let spec = base.specs()[0].clone();
    let registry = SpecRegistry { specs: vec![spec.clone(), spec] }; // in-module: private field OK
    assert!(registry.match_vin("1GCHK23224F000001").is_none());
    assert_eq!(registry.match_vin_all("1GCHK23224F000001").len(), 2);
}

#[test]
fn match_vin_returns_single_unambiguous_match() {
    let registry = SpecRegistry::with_defaults(); // one embedded Duramax spec
    let matched = registry.match_vin("1GCHK23224F000001");
    assert_eq!(matched.map(|s| s.identity.engine.code.as_str()), Some("LLY"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p obd2-core match_vin_returns -- --nocapture`
Expected: FAIL — `match_vin_all` undefined; ambiguity test fails because current `match_vin` returns the first of two matches.

- [ ] **Step 3: Implement one-pass ambiguity check + `match_vin_all`**

Replace `match_vin` (505-512) with:

```rust
/// All specs whose VIN matcher accepts this VIN.
pub fn match_vin_all(&self, vin: &str) -> Vec<&VehicleSpec> {
    self.specs
        .iter()
        .filter(|s| s.identity.vin_match.as_ref().is_some_and(|m| m.matches(vin)))
        .collect()
}

/// Match a VIN to a single spec. Returns `None` for zero or >1 matches —
/// ambiguity is never silently resolved (RULE 1). One pass, no allocation.
pub fn match_vin(&self, vin: &str) -> Option<&VehicleSpec> {
    let mut found = None;
    for s in &self.specs {
        if s.identity.vin_match.as_ref().is_some_and(|m| m.matches(vin)) {
            if found.is_some() {
                return None; // ambiguous
            }
            found = Some(s);
        }
    }
    found
}
```

- [ ] **Step 4: Run tests + full obd2-core suite**

Run: `cargo test -p obd2-core match_vin` then `cargo test`
Expected: PASS. The pre-existing `test_registry_match_vin_duramax` / `test_registry_no_match` use the single embedded spec, so they are unaffected.

- [ ] **Step 5: Commit**

```bash
git add crates/obd2-core/src/vehicle/mod.rs
git commit -m "fix(core): make VIN spec matching ambiguity-aware (one-pass, no silent shadowing)"
```

---

## Task 2: Spec-scoped DTC lookup (RULE 1, remove the cross-spec footgun)

**Files:**
- Modify: `crates/obd2-core/src/vehicle/mod.rs` — add `impl VehicleSpec { fn lookup_dtc }`; deprecate `SpecRegistry::lookup_dtc` (538-547); update existing test `test_embedded_duramax_has_turbo_dtc_enrichment` (914-927).
- Test: inline `tests` module.

**Interfaces:**
- Consumes: `VehicleSpec::dtc_library`, `DtcLibrary::lookup`, `DtcEntry::meaning`.
- Produces: `VehicleSpec::lookup_dtc(&self, code) -> Option<&DtcEntry>` (inherently per-spec — no `spec_id` key needed). `SpecRegistry::lookup_dtc` becomes `#[deprecated]`. The live enrich path (`session/diagnostics.rs:25 enrich_dtcs(dtcs, spec: Option<&VehicleSpec>)`) is already single-spec-scoped and is unchanged; this task removes the unscoped public API so no future caller reintroduces P1xxx cross-OEM bleed.

- [ ] **Step 1: Write the failing test** (clone + mutate the embedded spec to prove per-spec scoping with real data)

```rust
#[test]
fn vehicle_spec_lookup_dtc_is_scoped_per_spec() {
    // P1xxx meanings are OEM-specific: each spec must see only its own library.
    let base = SpecRegistry::with_defaults();
    let a = base.specs()[0].clone();
    let mut b = a.clone();
    // find a real code in the embedded library, then give spec B a different meaning for it
    let code = a.dtc_library.as_ref().unwrap().ecm[0].code.clone();
    b.dtc_library.as_mut().unwrap().ecm[0].meaning = "DIFFERENT-OEM-MEANING".to_string();

    assert_ne!(
        a.lookup_dtc(&code).unwrap().meaning,
        b.lookup_dtc(&code).unwrap().meaning
    );
    assert_eq!(b.lookup_dtc(&code).unwrap().meaning, "DIFFERENT-OEM-MEANING");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p obd2-core vehicle_spec_lookup_dtc_is_scoped`
Expected: FAIL — `VehicleSpec::lookup_dtc` undefined.

- [ ] **Step 3: Add `VehicleSpec::lookup_dtc` and deprecate the registry-wide one**

Add an `impl VehicleSpec` block (near the `VehicleSpec` struct):

```rust
impl VehicleSpec {
    /// Look up a DTC within THIS spec's library only. OEM-scoped by
    /// construction — manufacturer-range codes (P1xxx, etc.) never bleed
    /// across specs because you resolve through the vehicle's own spec.
    pub fn lookup_dtc(&self, code: &str) -> Option<&DtcEntry> {
        self.dtc_library.as_ref().and_then(|lib| lib.lookup(code))
    }
}
```

Replace `SpecRegistry::lookup_dtc` (538-547) with a deprecated shim:

```rust
/// Deprecated: scans ALL specs and returns the first hit, bleeding
/// OEM-specific DTC meanings across manufacturers. Resolve through the
/// vehicle's own spec via `VehicleSpec::lookup_dtc`.
#[deprecated(note = "cross-spec bleed; use VehicleSpec::lookup_dtc")]
pub fn lookup_dtc(&self, code: &str) -> Option<&DtcEntry> {
    self.specs.iter().find_map(|s| s.lookup_dtc(code))
}
```

- [ ] **Step 4: Migrate the one existing caller-test to the scoped API**

In `test_embedded_duramax_has_turbo_dtc_enrichment` (914-927), replace `registry.lookup_dtc("P2563")` / `registry.lookup_dtc("P003A")` with `registry.specs()[0].lookup_dtc("P2563")` / `...("P003A")`. Assertions (`.meaning`, `.severity`) are unchanged.

- [ ] **Step 5: Run tests + full suite**

Run: `cargo test -p obd2-core lookup_dtc` then `cargo test`
Expected: PASS with no `deprecated` warnings (the only remaining caller of the deprecated fn is gone; if `cargo build` warns about the deprecated item being unused, that is acceptable, or delete the shim entirely if nothing references it — confirm with `grep -rn 'SpecRegistry' | grep lookup_dtc` first).

- [ ] **Step 6: Commit**

```bash
git add crates/obd2-core/src/vehicle/mod.rs
git commit -m "fix(core): add per-spec VehicleSpec::lookup_dtc; deprecate cross-spec SpecRegistry::lookup_dtc"
```

---

## Task 3: Rename the byte-level `Transport` trait to `Link`

**Files (exact, verified):**
- Modify: `crates/obd2-core/src/transport/mod.rs:52` (`pub trait Transport` → `pub trait Link`) + module docs (1-5) + doc-example (35-50).
- Modify: `crates/obd2-core/src/transport/serial.rs`, `ble.rs`, `mock.rs` (`impl Transport for …` → `impl Link for …`).
- Modify: `crates/obd2-core/src/transport/logging.rs` (`impl Transport for LoggingTransport` and test `impl Transport for FragmentedTransport` at 631).
- Modify: `crates/obd2-core/src/adapter/mod.rs:145` (`Option<&mut dyn Link>` + import).
- Modify: `crates/obd2-core/src/adapter/elm327.rs:34,43,54,701` (`Box<dyn Link>`, ctor arg, return type, import).
- Modify: `crates/obd2-hw-test/src/main.rs:216` (`Box<dyn Link>` + import).
- Modify: `crates/obd2-core/src/lib.rs` (any `pub use transport::Transport` → `Link`).

**Interfaces:**
- Produces: byte-level trait is now `Link`; the name `Transport` is freed for Task 5. **No `pub use Link as Transport` alias** — it would collide with Task 5's new `Transport`.

- [ ] **Step 1: Rename the trait and its docs** — in `transport/mod.rs`, `pub trait Transport: Send + Sync` → `pub trait Link: Send + Sync`; update module doc (1-5) and `# Example` (35-50) to `Link` / `impl Link for MyTransport`. Keep `ChunkObserver` and all method signatures identical.

- [ ] **Step 2: Rename every implementer/reference** (only where the token names the trait — never `SerialTransport`/`MockTransport`/`LoggingTransport`/`BleTransport`/`transport_mut`/module path `transport::`):
- `serial.rs`, `ble.rs`, `mock.rs`: `impl Transport for` → `impl Link for`.
- `logging.rs`: both impls (incl. `FragmentedTransport` at 631).
- `adapter/mod.rs:145`: `Option<&mut dyn Transport>` → `Option<&mut dyn Link>` + `use`.
- `elm327.rs:34/43/54/701`: field/ctor/return/impl → `Link` + `use crate::transport::Link`.
- `obd2-hw-test/src/main.rs:216`: `Box<dyn Transport>` → `Box<dyn Link>` + import.
- `lib.rs`: any re-export.

- [ ] **Step 3: Build obd2-core** — Run: `cargo build`. Expected: clean. Any error naming `Transport` is a missed site.

- [ ] **Step 4: Test obd2-core** — Run: `cargo test`. Expected: PASS, identical to before.

- [ ] **Step 5: Build + test obd2-dash** — Run (from obd2-dash): `cargo build && cargo test`. Expected: PASS (obd2-dash has no bare-`Transport` references — verified).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(core): rename byte-level Transport trait to Link (frees Transport for framed layer)"
```

---

## Task 4: Move ELM ASCII parsing to `adapter/elm_codec.rs` (INV-5) + fix obd2-dash imports

**Files:**
- Create: `crates/obd2-core/src/adapter/elm_codec.rs`
- Modify: `crates/obd2-core/src/protocol/codec.rs` (delete ELM-text fns + their tests; keep pure frame decoders)
- Modify: `crates/obd2-core/src/adapter/mod.rs` (`pub mod elm_codec;`)
- Modify: `crates/obd2-core/src/adapter/elm327.rs:318,661` (call `elm_codec::…`)
- Modify: `crates/obd2-dash/tests/protocol_payload_corpus.rs:4` and `tests/seed_corpus.rs:11` (import path)

**Interfaces:**
- Consumes: `protocol::codec::{BusFamily}` (frame decoders stay in codec).
- Produces: `adapter::elm_codec::{decode_elm_response_payload, decode_elm_response_payload_for_command}` — identical signatures to today's `codec` versions.

- [ ] **Step 1: Move the ELM-text functions verbatim** — into new `crates/obd2-core/src/adapter/elm_codec.rs`, unchanged: `decode_elm_response_payload` (226-232), `decode_elm_response_payload_for_command` (234-329), `parse_hex_line`, `parse_hex_tokens`, `hex_nibble`, `invalid_hex_byte`, `expected_response_prefix`, `parse_compact_hex`, `line_matches_command_echo` (331-423). Header:

```rust
//! ELM327 ASCII-response parsing (moved out of `protocol/` so protocol
//! decoders stay wire-dialect-pure — see INV-5).
use crate::error::Obd2Error;
use crate::protocol::codec::BusFamily;
```

If any moved fn calls a `codec`-private helper that stays in `codec` (e.g. a frame decoder), make that helper `pub(crate)` rather than duplicating it.

- [ ] **Step 2: Move the ELM-text unit tests** — the `test_decode_elm_response_payload*` tests (476-527) into an inline `#[cfg(test)] mod tests` in `elm_codec.rs`. Leave pure-frame tests (`test_decode_can_*`, `test_decode_j1850_*`, `test_decode_iso_kline_*`, `test_decode_generic_frame`, `test_decode_can_headers_off_payload`, 425-474) in `codec.rs`.

- [ ] **Step 3: Register module + fix obd2-core call sites** — add `pub mod elm_codec;` to `adapter/mod.rs`; in `elm327.rs:318` and `:661` change `codec::decode_elm_response_payload_for_command(` → `elm_codec::decode_elm_response_payload_for_command(` (args unchanged); add `use crate::adapter::elm_codec;`, keep `use crate::protocol::codec;` only if still used for frame types.

- [ ] **Step 4: Fix obd2-dash test imports** — in `obd2-dash/crates/obd2-dash/tests/protocol_payload_corpus.rs:4` and `tests/seed_corpus.rs:11`, change `use obd2_core::protocol::codec::decode_elm_response_payload_for_command;` → `use obd2_core::adapter::elm_codec::decode_elm_response_payload_for_command;`. (Leave `obd2_core::protocol::codec::BusFamily` imports elsewhere untouched.)

- [ ] **Step 5: Build + test both workspaces** — obd2-core `cargo build && cargo test`, then obd2-dash `cargo build && cargo test`. Expected: PASS in both — `protocol_payload_corpus` and `seed_corpus` compile against the new path and stay green (proves behavior unchanged).

- [ ] **Step 6: Commit**

```bash
git add -A   # both repos, or commit per-repo if they are separate git roots
git commit -m "refactor(core): move ELM ASCII parsing from protocol/codec to adapter/elm_codec; update obd2-dash imports"
```

> If obd2-core and obd2-dash are separate git repositories, commit each repo's changes separately with the same message; the obd2-dash change (import paths) must land together with the obd2-core move so obd2-dash keeps building.

---

## Task 5: Framed `Transport` + `ProtocolClient` traits, ELM bridge in the adapter layer

**Files:**
- Create: `crates/obd2-core/src/transport/framed.rs` (neutral framed-PDU trait)
- Create: `crates/obd2-core/src/protocol/client.rs` (`ProtocolClient` + `J1979Client`, generic, no ELM)
- Create: `crates/obd2-core/src/adapter/elm_transport.rs` (`ElmTransport` bridge + round-trip test)
- Modify: `transport/mod.rs` (`pub mod framed;`), `protocol/mod.rs` (`pub mod client;`), `adapter/mod.rs` (`pub mod elm_transport;`), `elm327.rs:206` (`protocol_family` → `pub(crate)`)

**Interfaces:**
- `transport::framed`: `Transport` trait — depends only on `error` and `protocol::codec::BusFamily`, **not on `adapter`**:
  ```rust
  #[async_trait::async_trait]
  pub trait Transport: Send {
      async fn exchange(&mut self, req: &TransportRequest) -> Result<Vec<u8>, crate::error::Obd2Error>;
      fn family(&self) -> crate::protocol::codec::BusFamily;
  }
  pub struct TransportRequest { pub service_id: u8, pub data: Vec<u8> } // P0: broadcast-only; routing/targeting in P1
  ```
- `protocol::client`: generic client, **no ELM/adapter reference** (keeps `protocol/` INV-5-clean):
  ```rust
  #[async_trait::async_trait]
  pub trait ProtocolClient: Send {
      fn name(&self) -> &'static str;
      async fn request(&mut self, kind: RequestKind) -> Result<DiagResponse, crate::error::Obd2Error>;
  }
  pub enum RequestKind { Mode01Pid(u8), Did16 { service: u8, did: u16 }, Raw { service: u8, data: Vec<u8> } }
  pub struct DiagResponse { pub expected_positive_service: u8, pub payload: Vec<u8> }
  pub struct J1979Client<T: crate::transport::framed::Transport> { transport: T }
  ```
- `adapter::elm_transport`: `ElmTransport` wraps `Elm327Adapter` and implements `transport::framed::Transport` — the adapter→transport bridge lives here (adapter may depend on transport; transport may not depend on adapter).

- [ ] **Step 1: Write the failing round-trip test** (in `adapter/elm_transport.rs`, where using `Elm327Adapter`/`MockTransport` is layer-correct)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::elm327::Elm327Adapter;
    use crate::protocol::client::{J1979Client, ProtocolClient, RequestKind};
    use crate::transport::mock::MockTransport;

    #[tokio::test]
    async fn j1979_client_reads_pid_over_elm_backed_transport() {
        let mut mock = MockTransport::new();
        mock.expect("0105", "41 05 7B\r\r>"); // coolant temp
        let adapter = Elm327Adapter::new(Box::new(mock));
        let mut client = J1979Client::new(ElmTransport::new(adapter));
        let resp = client.request(RequestKind::Mode01Pid(0x05)).await.unwrap();
        assert_eq!(resp.expected_positive_service, 0x41);
        assert_eq!(resp.payload, vec![0x7B]); // raw byte; SAE scaling is a higher layer
    }
}
```

(Adapter init/`expect` details: if `Elm327Adapter` requires an `initialize()` call or additional `expect` primes before a Mode-01 request in mock mode, add them per the existing MockTransport-based adapter tests in `elm327.rs`'s test module — read them first and mirror the setup.)

- [ ] **Step 2: Run to verify failure** — Run: `cargo test -p obd2-core j1979_client_reads_pid`. Expected: FAIL (types undefined).

- [ ] **Step 3: Define the neutral framed `Transport`** — create `transport/framed.rs` with the `Transport`/`TransportRequest` from Interfaces; add `pub mod framed;` to `transport/mod.rs`.

- [ ] **Step 4: Define the ELM bridge** — make `Elm327Adapter::protocol_family` `pub(crate)` (`elm327.rs:206`). Create `adapter/elm_transport.rs`:

```rust
use crate::adapter::{elm327::Elm327Adapter, Adapter};
use crate::protocol::codec::BusFamily;
use crate::protocol::service::{ServiceRequest, Target};
use crate::transport::framed::{Transport, TransportRequest};

pub struct ElmTransport { adapter: Elm327Adapter }
impl ElmTransport { pub fn new(adapter: Elm327Adapter) -> Self { Self { adapter } } }

#[async_trait::async_trait]
impl Transport for ElmTransport {
    async fn exchange(&mut self, req: &TransportRequest) -> Result<Vec<u8>, crate::error::Obd2Error> {
        self.adapter.request(&ServiceRequest {
            service_id: req.service_id,
            data: req.data.clone(),
            target: Target::Broadcast, // P0: broadcast only
        }).await
    }
    fn family(&self) -> BusFamily { self.adapter.protocol_family() }
}
```

Add `pub mod elm_transport;` to `adapter/mod.rs`. (`Adapter::request` already returns echo-stripped payload bytes via the existing `elm_codec` decode path — so `exchange` reuses the exact current decode logic; that is why P0 is behavior-neutral.)

- [ ] **Step 5: Define `ProtocolClient` + `J1979Client`** — create `protocol/client.rs` with the types from Interfaces (generic over `transport::framed::Transport`, no ELM reference):

```rust
use crate::transport::framed::{Transport, TransportRequest};

impl<T: Transport> J1979Client<T> {
    pub fn new(transport: T) -> Self { Self { transport } }
}

#[async_trait::async_trait]
impl<T: Transport> ProtocolClient for J1979Client<T> {
    fn name(&self) -> &'static str { "J1979" }
    async fn request(&mut self, kind: RequestKind) -> Result<DiagResponse, crate::error::Obd2Error> {
        let (service_id, data) = match kind {
            RequestKind::Mode01Pid(pid) => (0x01u8, vec![pid]),
            RequestKind::Did16 { service, did } => (service, did.to_be_bytes().to_vec()),
            RequestKind::Raw { service, data } => (service, data),
        };
        let expected_positive_service = service_id
            .checked_add(0x40)
            .ok_or_else(|| crate::error::Obd2Error::ParseError(format!("service id 0x{service_id:02X} cannot form a positive-response id")))?;
        let payload = self.transport.exchange(&TransportRequest { service_id, data }).await?;
        Ok(DiagResponse { expected_positive_service, payload })
    }
}
```

Add `pub mod client;` to `protocol/mod.rs`.

- [ ] **Step 6: Run round-trip test + both suites** — `cargo test -p obd2-core j1979_client_reads_pid`, then obd2-core `cargo test`, then obd2-dash `cargo test`. Expected: PASS everywhere (new traits are additive; nothing existing is rewired).

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat(core): add neutral framed Transport + generic ProtocolClient with ELM bridge in adapter layer (seam only)"
```

---

## Task 6: Architecture test — `protocol/` is ELM-free, recursively (INV-5)

**Files:** Create: `crates/obd2-core/tests/architecture.rs`

- [ ] **Step 1: Write the recursive source-scan test**

```rust
//! Architecture invariants for obd2-core (source-text scans).
use std::fs;
use std::path::{Path, PathBuf};

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).expect("read dir") {
        let p = entry.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// INV-5: the protocol/ layer must stay wire-dialect-pure — no ELM327/AT
/// text-mode parsing anywhere under protocol/, including future submodules.
#[test]
fn protocol_module_is_elm_free() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/protocol");
    let mut files = Vec::new();
    rs_files(&dir, &mut files);
    assert!(!files.is_empty(), "protocol/ dir not found or empty");

    let mut offenders = Vec::new();
    for path in files {
        let src = fs::read_to_string(&path).unwrap();
        for needle in ["ELM327", "decode_elm_response_payload", "SEARCHING...", "AT SH"] {
            if src.contains(needle) {
                offenders.push(format!("{}: contains {:?}", path.display(), needle));
            }
        }
    }
    assert!(offenders.is_empty(), "protocol/ must be ELM-free (INV-5):\n{}", offenders.join("\n"));
}
```

(`BusFamily`/frame decoders mentioning "CAN"/"J1850" are fine — only ELM/AT text needles are forbidden.)

- [ ] **Step 2: Run** — `cargo test -p obd2-core --test architecture`. Expected: PASS (Task 4 moved the ELM parsing out). If FAIL, finish moving the named needle to `elm_codec.rs`.

- [ ] **Step 3: Commit**

```bash
git add crates/obd2-core/tests/architecture.rs
git commit -m "test(core): assert protocol/ stays ELM-free recursively (INV-5)"
```

---

## Task 7: P0 exit gate — full green + LLY/corpus verification

- [ ] **Step 1: obd2-core full suite** — `cargo test` (obd2-core). Expected: PASS.
- [ ] **Step 2: obd2-dash full suite** — `cargo test` (obd2-dash). Expected: PASS, specifically `protocol_payload_corpus`, `seed_corpus`, `lly_signal_corpus`, `lly_dtc_corpus`, `corpus_selection`, `corpus_support` all green (proves zero behavior change).
- [ ] **Step 3: Clippy** — `cargo clippy --all-targets` in each workspace. Expected: no new warnings (deprecation of `SpecRegistry::lookup_dtc` is intentional; confirm nothing still calls it).
- [ ] **Step 4: Confirm invariants** — `protocol/` imports no `adapter`/ELM symbols (arch test green); byte trait is `Link`; framed trait is `Transport` (neutral); `ProtocolClient`/`J1979Client` exist with an ELM-backed round-trip test in the adapter layer; `match_vin` rejects ambiguity; `VehicleSpec::lookup_dtc` is per-spec.
- [ ] **Step 5: Branch hygiene** — `git status` clean; `git log --oneline -8`. Do not merge to `master`.

---

## Self-review (spec coverage)

- **"rename byte-Transport → Link"** → Task 3. ✓
- **"introduce Transport/ProtocolClient with ELM behind them"** → Task 5, with the ELM bridge kept in the adapter layer so transport/ and protocol/ stay adapter-independent (fixes the layer-inversion review finding). ✓
- **"move ELM text parsing out of protocol/codec.rs (INV-5)"** → Tasks 4 + 6, incl. the obd2-dash import updates that the move forces (corrected review finding) and a recursive scan. ✓
- **"fix cross-spec DTC/VIN bleed"** → Task 1 (VIN, live, one-pass no-alloc) + Task 2 (DTC via per-spec `VehicleSpec::lookup_dtc`, deprecating the registry-wide footgun). ✓
- **"no behavior change / corpus + LLY green"** → every task's test step + Task 7. ✓
- **"no new deps"** → none added. ✓

Type/name consistency across tasks: `Link` (byte); `Transport`/`TransportRequest{service_id,data}` (framed, neutral); `ProtocolClient`/`RequestKind`/`DiagResponse`/`J1979Client<T: Transport>`; `ElmTransport::new`; `match_vin`/`match_vin_all`; `VehicleSpec::lookup_dtc`. All field/method names (`identity.name`, `identity.engine.code`, `DtcEntry.meaning`, `MockTransport::{new,expect}`, `SpecRegistry::{with_defaults,specs}`) are the verified real names, listed in the "Verified API facts" section.

**Implementer note:** the only remaining "read the source first" items are the `Elm327Adapter` mock-init details in Task 5 Step 1 (mirror the existing `elm327.rs` test setup) — everything else is literal and API-verified.
