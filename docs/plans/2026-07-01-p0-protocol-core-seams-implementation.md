# Phase P0 — Protocol-Core Seams & Safety Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Establish the protocol-isolation seams in `obd2-core` (rename the byte-level trait, introduce the framed `Transport` + `ProtocolClient` traits with ELM behind them, evict ELM text-parsing from `protocol/`, and fix the cross-spec VIN/DTC bleed) with **zero behavior change** — LLY path and golden corpus stay green throughout.

**Architecture:** `obd2-core` today is one ELM327 text-mode `Adapter` over a byte-level `Transport`, with ELM ASCII parsing living inside `protocol/codec.rs`. P0 renames the byte trait to `Link`, adds a new framed-PDU `Transport` trait and a `ProtocolClient` trait (with the ELM path as the first backend/impl), moves all ELM ASCII handling out of `protocol/` into an `adapter/elm_codec.rs` so `protocol/` becomes wire-dialect-pure (invariant INV-5), and makes spec lookup ambiguity-aware so one OEM's matcher can't shadow another's (RULE 1). No Session rewiring — that is P1.

**Tech Stack:** Rust 2021, edition per workspace (`obd2-core` rust-version 1.75), `tokio`, `async-trait`, `thiserror`, `serde`. Two separate Cargo workspaces: `obd2-core` (library, at `/Users/jared/Projects/HaulLogic/obd2-core`) and `obd2-dash` (app, at `/Users/jared/Projects/HaulLogic/obd2-dash`); `obd2-dash` consumes `obd2-core` as a dependency.

## Global Constraints

- **No behavior change.** Every existing test in both workspaces must still pass unchanged after each task, except tests renamed purely because a symbol was renamed. The golden corpus (`obd2-dash/crates/obd2-dash/tests/corpus/`) and the LLY path must produce bit-identical output.
- **Conservative dependencies.** Do not add new crates. (User policy: minimal deps; standard dev crates like `tempfile` are acceptable, but P0 needs none.)
- **Respect the hardware boundary and message-driven architecture** (per `AGENTS.md`): no framework churn, prefer explicit control flow, keep hot paths allocation-light.
- **Commit after every task** with a conventional-commit message. Work on the current branch `docs/multi-oem-protocol-architecture` or a dedicated `feat/p0-protocol-seams` branch — do **not** commit to `master`.
- **Test commands run per-workspace:** obd2-core tests via `cargo test` run from `/Users/jared/Projects/HaulLogic/obd2-core`; obd2-dash tests from `/Users/jared/Projects/HaulLogic/obd2-dash`. After any obd2-core change, run BOTH.
- **No `unsafe`.** None of P0 requires it.

---

## File structure (what P0 creates or changes)

**obd2-core:**
- `crates/obd2-core/src/vehicle/mod.rs` — MODIFY: `match_vin` ambiguity-aware; harden/scope `lookup_dtc`.
- `crates/obd2-core/src/transport/mod.rs` — MODIFY: rename trait `Transport` → `Link` (+ doc example, module docs).
- `crates/obd2-core/src/transport/{serial,ble,mock,logging}.rs` — MODIFY: `impl Transport for X` → `impl Link for X`.
- `crates/obd2-core/src/adapter/mod.rs` — MODIFY: `transport_mut(&mut self) -> Option<&mut dyn Link>`.
- `crates/obd2-core/src/adapter/elm327.rs` — MODIFY: `Box<dyn Link>` fields/ctor; call `elm_codec::…` instead of `codec::decode_elm_response_payload*`.
- `crates/obd2-core/src/adapter/elm_codec.rs` — CREATE: the ELM ASCII→bytes layer moved out of `protocol/codec.rs`.
- `crates/obd2-core/src/protocol/codec.rs` — MODIFY: delete the ELM-text functions (keep pure frame decoders `decode_can_headers_on`, `decode_j1850_headers_on`, `decode_iso_kline_headers_on`, `decode_frame`, `decode_can_headers_off`).
- `crates/obd2-core/src/protocol/client.rs` — CREATE: `ProtocolClient` trait + `RequestKind`/`DiagResponse` types + `J1979Client` shell wrapping the ELM path.
- `crates/obd2-core/src/transport/framed.rs` — CREATE: framed-PDU `Transport` trait (distinct from the renamed `Link`).
- `crates/obd2-core/src/lib.rs` — MODIFY: `pub mod`/`pub use` wiring for the new modules.
- `crates/obd2-core/tests/architecture.rs` — CREATE: source-scan test that `protocol/` is ELM-free (INV-5).
- `crates/obd2-hw-test/src/main.rs` — MODIFY: `Box<dyn Transport>` → `Box<dyn Link>` at line 216.

**obd2-dash:** none expected (verified: zero bare-`Transport` references). Task 3 includes a build/test gate to confirm.

---

## Task 1: VIN spec-match ambiguity detection (RULE 1, live path)

**Files:**
- Modify: `crates/obd2-core/src/vehicle/mod.rs:505-512` (`match_vin`)
- Test: `crates/obd2-core/src/vehicle/mod.rs` (inline `#[cfg(test)] mod tests`, near existing `match_vin` tests at ~930-945)

**Interfaces:**
- Consumes: existing `VehicleSpec`, `VinMatcher::matches`.
- Produces: `SpecRegistry::match_vin(&self, vin: &str) -> Option<&VehicleSpec>` (unchanged signature, changed semantics: returns `Some` only for an unambiguous single match) **and** new `SpecRegistry::match_vin_all(&self, vin: &str) -> Vec<&VehicleSpec>` used to detect ambiguity. `session/mod.rs:488` keeps calling `match_vin` and transparently benefits.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/obd2-core/src/vehicle/mod.rs`:

```rust
#[test]
fn match_vin_returns_none_when_two_specs_match_same_vin() {
    // Two specs whose VIN matchers both accept the same VIN must NOT
    // silently shadow each other (RULE 1): ambiguity => no match.
    let mut registry = SpecRegistry::new();
    registry.register(spec_with_vin_prefix("gm-a", "1GC"));
    registry.register(spec_with_vin_prefix("gm-b", "1GC"));
    assert!(registry.match_vin("1GCHK23224F000001").is_none());
    assert_eq!(registry.match_vin_all("1GCHK23224F000001").len(), 2);
}

#[test]
fn match_vin_returns_single_unambiguous_match() {
    let mut registry = SpecRegistry::new();
    registry.register(spec_with_vin_prefix("gm-a", "1GC"));
    registry.register(spec_with_vin_prefix("honda", "JH4"));
    let matched = registry.match_vin("1GCHK23224F000001");
    assert_eq!(matched.map(|s| s.identity.id.as_str()), Some("gm-a"));
}
```

If a `spec_with_vin_prefix` test helper and `SpecRegistry::register`/`new` do not already exist in this module, add minimal versions in the `tests` module (adapt field names to the real `VehicleSpec`/`VinMatcher`/`VehicleIdentity` structs — read them at `vehicle/mod.rs` before writing; do not invent fields).

- [ ] **Step 2: Run the test to verify it fails**

Run (from `/Users/jared/Projects/HaulLogic/obd2-core`): `cargo test -p obd2-core match_vin_returns -- --nocapture`
Expected: FAIL — `match_vin_all` not found and/or the first test fails because current `match_vin` returns the first of two matches.

- [ ] **Step 3: Implement ambiguity-aware matching**

Replace `match_vin` (lines 505-512) and add `match_vin_all`:

```rust
/// All specs whose VIN matcher accepts this VIN (for ambiguity handling).
pub fn match_vin_all(&self, vin: &str) -> Vec<&VehicleSpec> {
    self.specs
        .iter()
        .filter(|s| {
            s.identity
                .vin_match
                .as_ref()
                .is_some_and(|m| m.matches(vin))
        })
        .collect()
}

/// Match a VIN to a single spec. Returns `None` if zero or more-than-one
/// spec matches (ambiguity is never silently resolved — see RULE 1).
pub fn match_vin(&self, vin: &str) -> Option<&VehicleSpec> {
    let mut matches = self.match_vin_all(vin);
    match matches.len() {
        1 => matches.pop(),
        _ => None,
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p obd2-core match_vin`
Expected: PASS (both new tests + the pre-existing `match_vin` tests at ~930-945; those use single non-overlapping specs so they still return `Some`).

- [ ] **Step 5: Run the full obd2-core suite (no regressions)**

Run: `cargo test` (from `/Users/jared/Projects/HaulLogic/obd2-core`)
Expected: PASS. If any test relied on first-hit shadowing with overlapping VIN matchers, that is a latent bug surfacing — fix the test fixture to be unambiguous, do not revert the behavior.

- [ ] **Step 6: Commit**

```bash
git add crates/obd2-core/src/vehicle/mod.rs
git commit -m "fix(core): make VIN spec matching ambiguity-aware (no silent shadowing)"
```

---

## Task 2: Scope the cross-spec DTC lookup footgun (RULE 1, latent path)

**Files:**
- Modify: `crates/obd2-core/src/vehicle/mod.rs:538-547` (`lookup_dtc`)
- Test: `crates/obd2-core/src/vehicle/mod.rs` tests module (near existing `lookup_dtc` tests at ~915-930)

**Interfaces:**
- Consumes: `VehicleSpec::dtc_library`, `DtcLibrary::lookup`, `DtcEntry`.
- Produces: `SpecRegistry::lookup_dtc_in(&self, spec_id: &str, code: &str) -> Option<&DtcEntry>` (spec-scoped) and a deprecated shim `lookup_dtc` retained for the existing tests. The live enrich path (`session/diagnostics.rs:25 enrich_dtcs`) already takes a single `spec` and is unchanged — this task removes the unscoped public footgun so no future caller reintroduces P1xxx cross-OEM bleed.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn lookup_dtc_in_does_not_bleed_across_specs() {
    // P1xxx meanings are OEM-specific; a spec-scoped lookup must only see
    // its own DTC library, never another loaded spec's.
    let mut registry = SpecRegistry::new();
    registry.register(spec_with_dtc("gm", "P1133", "GM: HO2S insufficient switching"));
    registry.register(spec_with_dtc("ford", "P1133", "Ford: HO2S-11 lack of switching"));

    let gm = registry.lookup_dtc_in("gm", "P1133").map(|e| e.description.as_str());
    let ford = registry.lookup_dtc_in("ford", "P1133").map(|e| e.description.as_str());
    assert_eq!(gm, Some("GM: HO2S insufficient switching"));
    assert_eq!(ford, Some("Ford: HO2S-11 lack of switching"));
}
```

Add a `spec_with_dtc(id, code, desc)` helper in the tests module matching the real `DtcLibrary`/`DtcEntry` field names (read them first).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p obd2-core lookup_dtc_in`
Expected: FAIL — `lookup_dtc_in` not found.

- [ ] **Step 3: Add the spec-scoped lookup and deprecate the unscoped one**

Replace `lookup_dtc` (538-547) with:

```rust
/// Look up a DTC within a single named spec's library (OEM-scoped).
/// P1xxx and other manufacturer-range codes have OEM-specific meanings,
/// so cross-spec lookup is forbidden — resolve through the vehicle's own spec.
pub fn lookup_dtc_in(&self, spec_id: &str, code: &str) -> Option<&DtcEntry> {
    self.specs
        .iter()
        .find(|s| s.identity.id.as_str() == spec_id)
        .and_then(|s| s.dtc_library.as_ref())
        .and_then(|lib| lib.lookup(code))
}

/// Deprecated: scans ALL specs and returns the first hit, which bleeds
/// OEM-specific meanings across manufacturers. Use `lookup_dtc_in`.
#[deprecated(note = "cross-spec bleed; use lookup_dtc_in(spec_id, code)")]
pub fn lookup_dtc(&self, code: &str) -> Option<&DtcEntry> {
    self.specs
        .iter()
        .filter_map(|s| s.dtc_library.as_ref())
        .find_map(|lib| lib.lookup(code))
}
```

(Adapt `s.identity.id.as_str()` to the real identity id accessor.)

- [ ] **Step 4: Silence the deprecation in the pre-existing tests**

At the top of the existing `lookup_dtc` tests (~915-930), add `#[allow(deprecated)]` to those `#[test]` fns so the suite stays clean while the shim remains for coverage.

- [ ] **Step 5: Run to verify pass + no regressions**

Run: `cargo test -p obd2-core lookup_dtc` then `cargo test` (whole obd2-core).
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/obd2-core/src/vehicle/mod.rs
git commit -m "fix(core): add spec-scoped DTC lookup; deprecate cross-spec lookup_dtc"
```

---

## Task 3: Rename the byte-level `Transport` trait to `Link`

**Files (exact sites, verified):**
- Modify: `crates/obd2-core/src/transport/mod.rs:52` (`pub trait Transport` → `pub trait Link`), plus module docs (1-5) and the doc-example (35-50).
- Modify: `crates/obd2-core/src/transport/serial.rs` (`impl Transport for SerialTransport` → `impl Link for SerialTransport`)
- Modify: `crates/obd2-core/src/transport/ble.rs` (`impl Transport for BleTransport`)
- Modify: `crates/obd2-core/src/transport/mock.rs` (`impl Transport for MockTransport`)
- Modify: `crates/obd2-core/src/transport/logging.rs` (`impl Transport for LoggingTransport` and the test `impl Transport for FragmentedTransport` at 631)
- Modify: `crates/obd2-core/src/adapter/mod.rs:145` (`transport_mut(&mut self) -> Option<&mut dyn Link>`)
- Modify: `crates/obd2-core/src/adapter/elm327.rs:34,43,54,701` (`Box<dyn Link>`, ctor arg, `transport_mut`)
- Modify: `crates/obd2-hw-test/src/main.rs:216` (`Box<dyn Transport>` → `Box<dyn Link>`)
- Modify: `crates/obd2-core/src/lib.rs` (any `pub use transport::Transport` → `pub use transport::Link`)

**Interfaces:**
- Consumes: nothing new.
- Produces: the byte-level trait is now `Link`; the name `Transport` is freed for Task 5's framed-PDU trait. **No deprecated alias** — a `pub use Link as Transport` would collide with the new `Transport` trait in Task 5, so rename cleanly.

- [ ] **Step 1: Rename the trait definition and its docs**

In `transport/mod.rs`: rename `pub trait Transport: Send + Sync` to `pub trait Link: Send + Sync`; update the module doc comment (lines 1-5) and the `# Example` doc block (35-50) to say `Link`/`impl Link for MyTransport`. Keep `ChunkObserver` and all method signatures identical.

- [ ] **Step 2: Rename every implementer and reference**

Apply these exact edits (each is `Transport` → `Link` only where it names the trait, never where it is part of `SerialTransport`/`MockTransport`/`LoggingTransport`/`BleTransport`/`transport_mut`/module path `transport::`):
- `transport/serial.rs`: `impl Transport for` → `impl Link for`
- `transport/ble.rs`: `impl Transport for` → `impl Link for`
- `transport/mock.rs`: `impl Transport for` → `impl Link for`
- `transport/logging.rs`: both `impl Transport for LoggingTransport` and `impl Transport for FragmentedTransport` (line 631)
- `adapter/mod.rs:145`: `Option<&mut dyn Transport>` → `Option<&mut dyn Link>` (and its `use` import)
- `adapter/elm327.rs:34`: `transport: Box<dyn Transport>` → `Box<dyn Link>`; `:43` ctor `transport: Box<dyn Transport>` → `Box<dyn Link>`; `:54` `-> &mut dyn Transport` → `-> &mut dyn Link`; `:701` `Option<&mut dyn Transport>` → `Option<&mut dyn Link>`; update the `use crate::transport::Transport` import to `Link`.
- `obd2-hw-test/src/main.rs:216`: `Box<dyn Transport>` → `Box<dyn Link>`; update its import.
- `lib.rs`: update any `pub use`/`use` of `transport::Transport`.

- [ ] **Step 3: Compile obd2-core**

Run: `cargo build` (from `/Users/jared/Projects/HaulLogic/obd2-core`)
Expected: builds clean. Any remaining error naming `Transport` points to a missed site — fix it. Do NOT rename `SerialTransport`, `MockTransport`, `LoggingTransport`, `BleTransport`, `transport_mut`, or the `transport` module.

- [ ] **Step 4: Run obd2-core + obd2-hw-test tests**

Run: `cargo test` (obd2-core workspace)
Expected: PASS, identical results to before the rename.

- [ ] **Step 5: Build + test obd2-dash (confirm zero blast radius)**

Run (from `/Users/jared/Projects/HaulLogic/obd2-dash`): `cargo build && cargo test`
Expected: PASS. (Verified pre-emptively: obd2-dash has no bare-`Transport` references. If the build breaks, a missed obd2-dash reference exists — rename it `Link`.)

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(core): rename byte-level Transport trait to Link (frees Transport for framed layer)"
```

---

## Task 4: Move ELM ASCII parsing out of `protocol/codec.rs` into `adapter/elm_codec.rs`

**Files:**
- Create: `crates/obd2-core/src/adapter/elm_codec.rs`
- Modify: `crates/obd2-core/src/protocol/codec.rs` (delete the ELM-text functions + their unit tests, keep pure frame decoders)
- Modify: `crates/obd2-core/src/adapter/mod.rs` (`pub mod elm_codec;`)
- Modify: `crates/obd2-core/src/adapter/elm327.rs:318,661` (call `elm_codec::…` instead of `codec::…`)

**Interfaces:**
- Consumes: `protocol::codec::{BusFamily, decode_frame, DecodedFrame}` (the pure decoders stay in codec and `elm_codec` may call them).
- Produces: `adapter::elm_codec::{decode_elm_response_payload, decode_elm_response_payload_for_command}` with identical signatures to the current `codec` functions:
  - `decode_elm_response_payload(response: &str, family: BusFamily, skip_bytes: usize) -> Result<Vec<u8>, Obd2Error>`
  - `decode_elm_response_payload_for_command(response: &str, family: BusFamily, skip_bytes: usize, echo_command: Option<&str>) -> Result<Vec<u8>, Obd2Error>`

- [ ] **Step 1: Create `elm_codec.rs` by moving the ELM-text functions verbatim**

Move these items from `protocol/codec.rs` into the new `crates/obd2-core/src/adapter/elm_codec.rs`, unchanged in body: `decode_elm_response_payload` (226-232), `decode_elm_response_payload_for_command` (234-329), `parse_hex_line` (331), `parse_hex_tokens` (336), `hex_nibble` (356), `invalid_hex_byte` (365), `expected_response_prefix` (369-382), `parse_compact_hex` (384), `line_matches_command_echo` (403-423). Add the needed `use` at the top:

```rust
//! ELM327 ASCII-response parsing (moved out of `protocol/` so protocol
//! decoders stay wire-dialect-pure — see INV-5).
use crate::error::Obd2Error;
use crate::protocol::codec::BusFamily;
```

- [ ] **Step 2: Move the ELM-text unit tests too**

Move the ELM-specific tests from `codec.rs` (the `test_decode_elm_response_payload*` tests at 476-527) into an inline `#[cfg(test)] mod tests` in `elm_codec.rs`. Leave the pure-frame tests (`test_decode_can_*`, `test_decode_j1850_*`, `test_decode_iso_kline_*`, `test_decode_generic_frame`, `test_decode_can_headers_off_payload`, 425-474) in `codec.rs`.

- [ ] **Step 3: Register the module and fix call sites**

- In `adapter/mod.rs`, add `pub mod elm_codec;`.
- In `elm327.rs:318`, change `codec::decode_elm_response_payload_for_command(` → `elm_codec::decode_elm_response_payload_for_command(` (args unchanged: `&response, self.protocol_family(), skip, Some(&cmd)`).
- In `elm327.rs:661`, change the same call to `elm_codec::…`.
- Update `elm327.rs` imports: keep `use crate::protocol::codec;` only if still used for frame types; add `use crate::adapter::elm_codec;`.

- [ ] **Step 4: Compile**

Run: `cargo build` (obd2-core)
Expected: clean. Errors will name any missed call site or a `codec` helper that `elm_codec` still needs — if a moved fn used a `codec`-private helper that stays in codec, make that helper `pub(crate)` rather than duplicating it.

- [ ] **Step 5: Run tests (byte-identical behavior)**

Run: `cargo test` (obd2-core), then from obd2-dash: `cargo test` (the protocol payload corpus `protocol_payload_corpus.rs` exercises this exact path — it must stay green, proving no behavior change).
Expected: PASS in both.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor(core): move ELM ASCII parsing from protocol/codec to adapter/elm_codec"
```

---

## Task 5: Introduce the `Transport` (framed-PDU) and `ProtocolClient` traits with an ELM-backed impl

**Files:**
- Create: `crates/obd2-core/src/transport/framed.rs` (the framed-PDU `Transport` trait)
- Create: `crates/obd2-core/src/protocol/client.rs` (`ProtocolClient` trait + `RequestKind`/`DiagResponse` + `J1979Client`)
- Modify: `crates/obd2-core/src/lib.rs` / `transport/mod.rs` / `protocol/mod.rs` (module wiring + `pub use`)
- Test: inline tests in `client.rs` + a round-trip test driving a `MockTransport`-backed ELM path through `J1979Client`.

**Interfaces:**
- Consumes: `adapter::Adapter` (existing), `adapter::elm_codec`, `protocol::codec::BusFamily`, `adapter::elm327::Elm327Adapter`, `transport::mock::MockTransport`.
- Produces:
  - `transport::framed::Transport` — a framed-PDU exchange trait:
    ```rust
    #[async_trait::async_trait]
    pub trait Transport: Send {
        /// Exchange one diagnostic PDU (request bytes -> response payload bytes).
        async fn exchange(&mut self, req: &TransportRequest) -> Result<Vec<u8>, crate::error::Obd2Error>;
        /// The bus family this transport speaks (for decode/framing).
        fn family(&self) -> crate::protocol::codec::BusFamily;
    }
    pub struct TransportRequest { pub service_id: u8, pub data: Vec<u8>, pub target: crate::adapter::PhysicalTarget }
    ```
  - `protocol::client::ProtocolClient` — application-protocol trait:
    ```rust
    #[async_trait::async_trait]
    pub trait ProtocolClient: Send {
        fn name(&self) -> &'static str;
        async fn request(&mut self, kind: RequestKind) -> Result<DiagResponse, crate::error::Obd2Error>;
    }
    pub enum RequestKind { Mode01Pid(u8), Did16 { service: u8, did: u16 }, Raw { service: u8, data: Vec<u8> } }
    pub struct DiagResponse { pub service: u8, pub payload: Vec<u8> }
    pub struct J1979Client<T: crate::transport::framed::Transport> { transport: T }
    ```

- [ ] **Step 1: Write the failing round-trip test**

Create `protocol/client.rs` with a test that a Mode-01 PID request flows through the ELM path and returns the decoded payload bytes, using the existing `MockTransport` primed with an ELM response:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::elm327::Elm327Adapter;
    use crate::transport::mock::MockTransport;

    #[tokio::test]
    async fn j1979_client_reads_pid_over_elm_backed_transport() {
        // MockTransport queues (command, elm_response) pairs; prime a coolant-temp read.
        let mock = MockTransport::with_exchanges(vec![("0105".into(), "41 05 7B\r\r>".into())]);
        let mut client = J1979Client::over_elm(Elm327Adapter::new(Box::new(mock)));
        let resp = client.request(RequestKind::Mode01Pid(0x05)).await.unwrap();
        assert_eq!(resp.service, 0x41);
        assert_eq!(resp.payload, vec![0x7B]); // 0x7B raw; SAE decode is a higher layer
    }
}
```

Adapt `MockTransport::with_exchanges` to the real MockTransport constructor (read `transport/mock.rs`; it queues command→response string pairs — use whatever the real API is, do not invent it). If a convenience like `J1979Client::over_elm` is cleaner as a free fn, keep the name used here consistent across steps.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p obd2-core j1979_client_reads_pid`
Expected: FAIL — `J1979Client`, `RequestKind`, framed `Transport` not defined.

- [ ] **Step 3: Define the framed `Transport` trait**

Create `transport/framed.rs` with the `Transport`/`TransportRequest` definitions from the Interfaces block above. Add `pub mod framed;` to `transport/mod.rs`. Implement it for the ELM path via an adapter shim:

```rust
pub struct ElmTransport { adapter: crate::adapter::elm327::Elm327Adapter }
impl ElmTransport { pub fn new(adapter: crate::adapter::elm327::Elm327Adapter) -> Self { Self { adapter } } }

#[async_trait::async_trait]
impl Transport for ElmTransport {
    async fn exchange(&mut self, req: &TransportRequest) -> Result<Vec<u8>, crate::error::Obd2Error> {
        use crate::protocol::service::{ServiceRequest, Target};
        self.adapter.request(&ServiceRequest {
            service_id: req.service_id,
            data: req.data.clone(),
            target: Target::Broadcast, // P0: broadcast only; routing lands in P1
        }).await
    }
    fn family(&self) -> crate::protocol::codec::BusFamily { self.adapter.protocol_family() }
}
```

(If `Elm327Adapter::protocol_family` is private, expose a `pub(crate)` accessor rather than duplicating logic. `Adapter::request` already returns echo-stripped payload bytes, so `exchange` reuses the exact existing decode path — this is why P0 is behavior-neutral.)

- [ ] **Step 4: Define `ProtocolClient` + `J1979Client`**

Create `protocol/client.rs` with the trait/types from the Interfaces block. Implement `J1979Client` over any framed `Transport`:

```rust
impl<T: crate::transport::framed::Transport> J1979Client<T> {
    pub fn new(transport: T) -> Self { Self { transport } }
}
impl J1979Client<crate::transport::framed::ElmTransport> {
    pub fn over_elm(adapter: crate::adapter::elm327::Elm327Adapter) -> Self {
        Self::new(crate::transport::framed::ElmTransport::new(adapter))
    }
}

#[async_trait::async_trait]
impl<T: crate::transport::framed::Transport> ProtocolClient for J1979Client<T> {
    fn name(&self) -> &'static str { "J1979" }
    async fn request(&mut self, kind: RequestKind) -> Result<DiagResponse, crate::error::Obd2Error> {
        let req = match kind {
            RequestKind::Mode01Pid(pid) => TransportRequest { service_id: 0x01, data: vec![pid], target: PhysicalTarget::Broadcast },
            RequestKind::Did16 { service, did } => TransportRequest { service_id: service, data: did.to_be_bytes().to_vec(), target: PhysicalTarget::Broadcast },
            RequestKind::Raw { service, data } => TransportRequest { service_id: service, data, target: PhysicalTarget::Broadcast },
        };
        let payload = self.transport.exchange(&req).await?;
        Ok(DiagResponse { service: req.service_id + 0x40, payload })
    }
}
```

Add `pub mod client;` to `protocol/mod.rs`. Import `TransportRequest`/`PhysicalTarget` as needed.

- [ ] **Step 5: Run the round-trip test + full suites**

Run: `cargo test -p obd2-core j1979_client_reads_pid`, then `cargo test` (obd2-core), then obd2-dash `cargo test`.
Expected: PASS everywhere. The new traits are additive — no existing path is rewired, so behavior is unchanged.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat(core): add framed Transport + ProtocolClient traits with ELM-backed J1979 client (seam only)"
```

---

## Task 6: Architecture test — `protocol/` is ELM-free (INV-5)

**Files:**
- Create: `crates/obd2-core/tests/architecture.rs`

**Interfaces:**
- Consumes: nothing (source-scan test reading files under `crates/obd2-core/src/protocol/`).
- Produces: a CI guard that fails if ELM/AT text handling re-enters `protocol/`.

- [ ] **Step 1: Write the failing (then passing) source-scan test**

```rust
//! Architecture invariants for obd2-core (source-text scans).
use std::fs;
use std::path::Path;

/// INV-5: the protocol/ layer must stay wire-dialect-pure — no ELM327/AT
/// text-mode parsing. ELM ASCII handling lives in adapter/elm_codec.rs.
#[test]
fn protocol_module_is_elm_free() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/protocol");
    let mut offenders = Vec::new();
    for entry in fs::read_dir(&dir).expect("protocol dir") {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") { continue; }
        let src = fs::read_to_string(&path).unwrap();
        for needle in ["ELM327", "decode_elm_response_payload", "SEARCHING...", "AT SH", "\\r\\r>"] {
            if src.contains(needle) {
                offenders.push(format!("{}: contains {:?}", path.display(), needle));
            }
        }
    }
    assert!(offenders.is_empty(), "protocol/ must be ELM-free (INV-5):\n{}", offenders.join("\n"));
}
```

- [ ] **Step 2: Run it**

Run: `cargo test -p obd2-core --test architecture`
Expected: PASS (Task 4 already moved the ELM parsing out). If it FAILS, a listed needle still lives in `protocol/` — finish moving it to `elm_codec.rs`. (Note: `BusFamily`/frame decoders may mention "CAN"/"J1850" — those are fine; only ELM/AT text needles are forbidden.)

- [ ] **Step 3: Commit**

```bash
git add crates/obd2-core/tests/architecture.rs
git commit -m "test(core): assert protocol/ stays ELM-free (INV-5)"
```

---

## Task 7: P0 exit gate — full green + LLY/corpus verification

**Files:** none (verification only).

- [ ] **Step 1: obd2-core full suite**

Run (from `/Users/jared/Projects/HaulLogic/obd2-core`): `cargo test`
Expected: PASS.

- [ ] **Step 2: obd2-dash full suite incl. corpus + LLY**

Run (from `/Users/jared/Projects/HaulLogic/obd2-dash`): `cargo test`
Expected: PASS — specifically `protocol_payload_corpus`, `lly_signal_corpus`, `lly_dtc_corpus`, `corpus_selection`, `corpus_support` all green (proves zero behavior change through the seam extraction).

- [ ] **Step 3: Clippy (both workspaces)**

Run: `cargo clippy --all-targets` in each workspace.
Expected: no new warnings introduced by P0 (deprecation of `lookup_dtc` is intentional and allow-annotated at its test call sites).

- [ ] **Step 4: Confirm P0 invariants**

Manually confirm: `protocol/` compiles with no `adapter`/ELM import except the shared `codec` frame types; the byte trait is `Link`; the framed trait is `Transport`; `ProtocolClient`/`J1979Client` exist with an ELM-backed round-trip test; `match_vin` rejects ambiguity; `lookup_dtc_in` is spec-scoped.

- [ ] **Step 5: Final commit / branch note**

```bash
git status   # ensure clean
git log --oneline -8
```

No further commit needed if Tasks 1-6 each committed. Do not merge to `master` — leave the branch for review.

---

## Self-review (spec coverage)

- **Spec P0 item "rename byte-Transport → Link"** → Task 3. ✓
- **Spec P0 item "introduce Transport/ProtocolClient with ELM behind them"** → Tasks 5. ✓ (seam only; Session migration deferred to P1, as the spec's P0 states "everything still runs on ELM").
- **Spec P0 item "move ELM text parsing out of protocol/codec.rs (INV-5)"** → Tasks 4 + 6. ✓
- **Spec P0 item "fix cross-spec DTC/VIN bleed"** → Tasks 1 (VIN, live) + 2 (DTC, latent footgun). ✓
- **Global constraint "no behavior change / corpus + LLY green"** → enforced at every task's test step and the Task 7 exit gate. ✓
- **Global constraint "no new deps"** → nothing in P0 adds a crate. ✓

Types are consistent across tasks: `Link` (byte), `Transport`/`TransportRequest` (framed), `ProtocolClient`/`RequestKind`/`DiagResponse`/`J1979Client`, `match_vin`/`match_vin_all`, `lookup_dtc_in`. `J1979Client::over_elm` and `ElmTransport::new` names are used identically in Task 5 steps 1, 3, and 4.

**Note for the implementer:** several code blocks say "adapt to the real struct/field names — read the file first." That is deliberate: `VehicleSpec`/`VinMatcher`/`DtcLibrary`/`MockTransport` field and constructor names must be read from source before writing the test helpers, because inventing them is the most likely way to break this plan. Everything else is literal.
