# Manufacturer Profile Implementation Waves

Status: draft implementation waves
Date: 2026-06-29
Companion: this is the coder-facing companion to `2026-06-29-manufacturer-profile-migration-plan.md`; that plan owns the invariants, terminology, and Regression Firewall, and this document sequences them into shippable waves.

## Purpose

This document breaks the manufacturer profile migration into 10 implementation waves sized for a coder to execute one at a time. The governing constraints for every wave are:

- Each wave is independently shippable and independently reversible. A wave merges on its own, and reverting a single wave never strands the tree in a non-building or behavior-changed state.
- The regression firewall and golden corpus stay green across every wave. No wave is allowed to change the decoded output of an already-supported profile or protocol. The LLY golden corpus is the frozen baseline from Wave 0 onward.
- Generic SAE OBD-II always works. At no point during the migration does a vehicle lose generic OBD-II. Manufacturer-specific behavior is only ever added behind a `SelectedProfile`; the absence of a profile means generic-only, never broken.

Waves map onto the migration plan's phases but are stated as concrete, verifiable units of coder work. Where a wave deliberately changes behavior (for example, moving the LLY forced-standard-PID workaround out of global code), that change is called out in the wave itself and is reflected by a new golden corpus entry, never by editing an existing expected output.

## Wave Dependency Graph

```text
Wave 0  (freeze + baseline golden corpus)
  |
  v
Wave 1  (neutral profile model + registry + runtime, no behavior change)
  |
  v
Wave 2  (session-owned VehicleContext + profile selection)
  |
  v
Wave 3  (central protocol-agnostic dispatcher; quarantine raw routed APIs)
  |
  v
Wave 3.5 (poll policy + scheduler: plan_poll_cycle; forced-PID / cadence /
          backoff / candidate-suppression / preferred_over OUT of global code)
  |
  +-------------------+-------------------+
  |                                       |
  v                                       v
Wave 4  (migrate LLY enhanced reads)   Wave 5  (migrate GM $19 DTC services)
  |                                       |
  +-------------------+-------------------+
                      |
                      v
                   Wave 6  (GUI/TUI unification on the shared runtime)
                      |
                      v
                   Wave 7  (evidence + recording/replay v3 profile frames)
                      |
                      v
                   Wave 8  (active tests under profiles)
                      |
                      v
                   Wave 9  (one non-GM proof profile)

Waves 4 and 5 have no dependency on each other and may be developed in
parallel once Wave 3.5 lands. Both must complete before Wave 6 begins.
All other waves are strictly sequential. (Graph labels match the wave section
titles below; the poll-policy/scheduler step is its own wave, Wave 3.5.)
```

## Global Acceptance Gate

Every wave, without exception, must leave the following CI checks green before it can merge. This is the required-green list from the migration plan's Regression Firewall, applied uniformly to each wave rather than only at the end:

```text
cargo test -p obd2-core      // protocol/codec/adapter unit + protocol golden corpus
cargo test -p obd2-dash      // profile runtime, dispatcher, decoders, profile corpus
profile selection corpus     // no false or overlapping exact matches
architectural import test    // live code cannot reach probe-only raw routed APIs
replay compatibility         // old recordings still replay identically
```

Gate rules that hold for every wave:

- Zero diffs on existing goldens. A wave adds new corpus files for new behavior; it never edits an existing expected output unless it is a deliberate, single-purpose decode correction explained in the commit message.
- Additive-only shared layers. Layer 1 (adapter/codec) and Layer 2 (dispatcher/runtime) may only grow by addition. The dispatcher gains no per-manufacturer branch in any wave. A shared-layer signature change requires the full protocol plus profile corpus to pass before merge.
- Generic OBD-II is verified green. Each wave's acceptance includes a check that a no-profile / unmatched vehicle still reads generic SAE OBD-II.
- Reversibility check. Each wave must be revertible as a unit with the gate still green on the prior commit.

Checks not yet meaningful in an early wave (for example, the profile corpus before any profile exists) are still listed and must pass trivially or be explicitly seeded by that wave; they are never skipped or disabled.

## Glossary

OWL thinking
: The adversarial review lens used in the migration plan's "Owl Review" section. It asks where safety actually lives versus where it merely appears to live (for example: a sealed token only helps if the resolver cannot mint it from weak criteria). Apply OWL thinking when sizing and reviewing each wave: assume a future caller will try to bypass the gate.

golden corpus
: A frozen set of real captured (and synthetic per-`BusFamily`) traffic, each entry being raw request/response bytes plus expected `DecodedSignal` / `DecodedDtc` / error classification. CI replays every entry through the real decoders and asserts byte-for-byte and value-for-value identical output. It is the primary regression firewall. The LLY J1850 VPW captures under `raw-captures/` seed the first frozen baseline.

SelectedProfile
: The sealed runtime token minted only by the profile resolver, never constructed by UI or call-site code. It is required by the dispatcher before any manufacturer-specific routed request executes, and it is bound to a specific context generation. No `SelectedProfile` means generic OBD-II only.

generation / epoch
: The `VehicleContext.generation` counter that scopes a `SelectedProfile`'s validity. Disconnect, reconnect, adapter change, protocol change, VIN change, or decoded-spec change increments the generation and invalidates all prior tokens. A stale-generation token must fail dispatcher validation.

profile vs manufacturer
: A manufacturer is the OEM family (`Gm`, `Ford`, `ChryslerRam`, `Generic`) and is not enough by itself to select behavior. A profile is a concrete vehicle/protocol support package (for example `gm.gmt800.lly.class2`) that owns its match rules, routed requests, decoders, DTC services, active tests, passive monitors, poll policy, evidence policy, and display metadata. "Chevy" is part of a vehicle identity, not a profile.

## Wave 0: Freeze + Inventory

### Objective

Freeze the exact current GM/LLY surface and pin today's decoded behavior as the immutable regression baseline before any profile-runtime refactor begins: enumerate every live manufacturer call site, seed a frozen golden corpus from the real LLY J1850 VPW captures under `raw-captures/`, and add an architectural test that fails the build if live dashboard code grows a NEW raw manufacturer-routed call site. The architectural allowlist is a per-symbol NON-INCREASING UPPER BOUND (`count <= frozen`), not an exact match, so the legitimate later removals in Waves 3/4/5/6 do not falsely turn it red. The frozen golden corpus uses ONE canonical directory layout and ONE shared loader so every later wave extends it purely additively. This wave changes zero runtime behavior; it only adds tests and committed fixtures.

### Depends on

- Nothing. Wave 0 is the foundation wave and the regression firewall every later wave is graded against.
- Hard ordering constraint: Wave 0 MUST land before any wave that touches `src/session_runner.rs`, `src/gm_enhanced.rs`, `src/gm_class2.rs`, or the GUI `main.rs`. The corpus and the architectural allowlist must be seeded from the pre-migration code, or they pin already-moved behavior and prove nothing. If a later wave merges first, the baseline is contaminated and must be reseeded from a pre-migration git tag.
- Hand-off contract (forward reference): Wave 0's allowlist is a non-increasing upper bound and its corpus schema/layout are frozen for ADDITIVE extension only. Waves 3/4/5 (dash) and Wave 6 (GUI) that remove call sites MUST decrement the matching Wave 0 upper bound in the SAME commit (see Tests and `docs/diagnostics/gm-lly-call-site-inventory.md`). Waves 4/5 populate `SignalGolden.signal_key` once `LLY_SIGNALS` exists and add new `signal-*.jsonl` / `dtc-*.jsonl` lines; Wave 9 adds `protocol/<family>/` tiers (e.g. `can-11bit`). None of these may rewrite a frozen file or restructure the layout -- the shared loader and schema exist precisely so growth is line/file additions only.
- OWL: the corpus deliberately captures CURRENT behavior including current bugs (the TCM signal `0x1940` labeled `"ecm"` at `session_runner.rs:890`; the `range_suspect` fuel-rail DIDs; the `0x1170/0x1171` rejected-DID UI fallbacks). Wave 0 does not fix these. It pins them so a later wave's fix shows up as exactly one intentional golden diff with a written reason, not a silent drift. Note the new `module` field records the PHYSICAL route (`request_header_hex` byte[1]: `0x10` -> `"ecm"`, `0x18` -> `"tcm"`), NOT the display label; for `0x1940` the route is TCM while the live display mislabels it `"ecm"`. `0x1940` is not in the Wave 0 real-capture set (it is a NOT-covered gap), so no Wave 0 fixture asserts it; when it is later added, `module` records the route and the README flags the route-vs-label discrepancy as the pinned bug.

### Files touched

- CREATE `crates/obd2-dash/tests/` (directory does not exist today; confirmed via `ls`).
- CREATE `crates/obd2-dash/tests/corpus_support.rs` -- shared, non-`#[test]` module compiled into each integration-test binary (declared with `mod corpus_support;` from each test file, or duplicated as a small `include!`). Holds the corpus entry structs, the JSONL loader, the hex helpers, and the decode-replay drivers. No `[test]` functions here. This module is THE single shared corpus loader/schema for the whole plan: every later wave (3/4/5/9) reuses these exact structs and `load_jsonl` rather than writing its own reader, which is what makes corpus growth additive and gives Wave 9 its one globbing runner.
- CREATE `crates/obd2-dash/tests/lly_signal_corpus.rs` -- the profile-tier replay test (Layer 3 decoder firewall).
- CREATE `crates/obd2-dash/tests/lly_dtc_corpus.rs` -- the `$19`/`$59` DTC decode replay test (synthetic-only; see Tests).
- CREATE `crates/obd2-dash/tests/protocol_payload_corpus.rs` -- the protocol-tier replay test (Layer 1 strip firewall) that pins `decode_elm_response_payload_for_command` output.
- CREATE `crates/obd2-dash/tests/architecture.rs` -- the source-scanning bypass-allowlist test.
- CREATE `crates/obd2-dash/tests/seed_corpus.rs` -- a single `#[test] #[ignore]` dev tool that reads `raw-captures/*.obd2raw`, derives candidate goldens, and writes them to `tests/corpus/.staging/` (NEVER over the frozen files). CI never runs it (`#[ignore]`).
- CREATE `crates/obd2-dash/tests/corpus/protocol/j1850-vpw/*.jsonl` -- frozen Layer 1 strip goldens (real bytes). Future waves add sibling tiers (e.g. `protocol/can-11bit/*.jsonl`) additively under `protocol/`.
- CREATE `crates/obd2-dash/tests/corpus/profile/gm.gmt800.lly.class2/signal-*.jsonl` -- frozen Layer 3 signal goldens, FLAT under the profile dir (canonical layout). Real bytes only: `0x1540`, `0x1543`, `0x162F` are confirmed-present per the capture inventory (see OWL gap note). Real-vs-synthetic is carried by the per-record `capture` field, NOT by directory.
- CREATE `crates/obd2-dash/tests/corpus/profile/gm.gmt800.lly.class2/dtc-*.jsonl` -- clearly-labeled SYNTHETIC `$19`/`$59` DTC goldens, FLAT under the same profile dir (canonical layout; the `dtc-` filename prefix lets the shared loader select them, and the per-record `source: "synthetic"` field carries the real-vs-synthetic split). Lifted from the existing unit-test vectors in `gm_class2.rs` (real captures contain NO `$19` traffic and NO stored DTCs; do not label these "real").
- CREATE `crates/obd2-dash/tests/corpus/README.md` -- documents the FROZEN canonical layout (`protocol/<family>/*.jsonl`; `profile/<profile_id>/signal-*.jsonl` + `dtc-*.jsonl`), the filename-prefix convention the shared loader keys on, the `SignalGolden`/`DtcGolden`/`PayloadGolden` schema (including `signal_key` as a Wave 4/5-populated additive field and `module` as the route pin), the real-vs-synthetic split (carried by the `capture`/`source` field, NOT by directory), the seeding/promotion workflow, and the not-covered DID gap list. It also states the additive-only rule: later waves add lines/files/tiers but never restructure the layout or rewrite a frozen file. (Fixture documentation colocated with the data, not a findings report.)
- CREATE `docs/diagnostics/gm-lly-call-site-inventory.md` -- the human-readable frozen call-site list AND the written DECREMENT CONTRACT. The machine-checked allowlist in `tests/architecture.rs` is the source of truth (the doc is its narrative mirror), and the doc states explicitly: each `(file, symbol, count)` is a NON-INCREASING UPPER BOUND; any wave that moves or removes a call site MUST lower the matching count in the same commit; the bound is never raised without explicit review. This is the contract Waves 3/4/5 (dash) and Wave 6 (GUI) follow when they migrate sites.
- MODIFY `crates/obd2-dash/Cargo.toml` -- no new dependencies. `serde`/`serde_json` are already `[dependencies]` and are available to integration tests; `tempfile` is already a dev-dependency. Add nothing unless a `[[test]]` entry is needed to disable harness sharing; default harness is fine.
- (SIBLING, separate crate) CREATE `apps/obd2-gui/src-tauri/tests/architecture.rs` -- GUI-local bypass allowlist. The GUI lives in a different crate (`apps/obd2-gui/src-tauri`), so its guard cannot live in the dash test binary without brittle cross-crate path reads. See OWL note under Tests.
- DELETE: none. Wave 0 deletes no code and modifies no `src/*.rs`.

OWL on the seeding tool writing files: a `#[test] #[ignore]` that writes is a foot-gun -- a careless `cargo test -- --ignored` could regenerate and silently overwrite a frozen golden, masking the exact regression the corpus exists to catch. Mitigation is mandatory: `seed_corpus` writes only into `tests/corpus/.staging/`, and a human diffs staging against the committed tree and promotes by hand. The replay tests read only the committed tree, never `.staging/`. Add `tests/corpus/.staging/` to `.gitignore`.

### Exact APIs

New test-support types and functions (in `tests/corpus_support.rs`). These are test-only; they introduce no public library surface.

```rust
use serde::Deserialize;

/// One frozen profile-tier signal golden: post-strip payload bytes -> decoded value.
/// `payload_hex` is what the LIVE ELM adapter delivers to the decoder AFTER the
/// service-0x22 skip of 3 bytes (elm327.rs:310). It is NOT the raw `62 ..` echo.
#[derive(Debug, Clone, Deserialize)]
pub struct SignalGolden {
    pub capture: String,            // source .obd2raw filename, or "synthetic"
    pub profile_id: String,         // "gm.gmt800.lly.class2"
    pub service_id: u8,             // 0x22
    pub did: u16,                   // Wave 0 key, e.g. 0x1540
    /// Stable signal key. Does NOT exist until Wave 4 introduces LLY_SIGNALS, so it is
    /// null/absent in every Wave 0 fixture. The field is frozen NOW so Waves 4/5 can
    /// populate it ADDITIVELY (no rewrite of the frozen files). serde(default) keeps
    /// Wave 0 lines (which omit it) valid. The Wave 0 decode test is agnostic to it.
    #[serde(default)]
    pub signal_key: Option<String>,
    /// Route module the live capture used: "ecm" or "tcm". Derived from
    /// request_header_hex byte[1] (0x10 -> ecm, 0x18 -> tcm). Pins the ecm/tcm route
    /// so the (later) poll-policy wave can assert per-module dispatch. This is the
    /// physical route, NOT the display label (see OWL note on 0x1940).
    pub module: String,
    pub request_hex: String,        // "221540" (ELM command, post AT-SH)
    pub request_header_hex: String, // observed "6C10F1" (route provenance; node = byte[1])
    pub payload_hex: String,        // post-skip bytes fed to the decoder, e.g. "E1"
    pub expected: SignalExpected,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SignalExpected {
    pub selected_raw: u32,          // GmDecodedValue.selected_raw
    pub value: f64,                 // GmDecodedValue.value (compared via to_bits)
    pub unit: String,               // GmDecodedValue.unit
}

/// One frozen DTC golden: a decode_class2_dtcs payload -> decoded records.
/// Wave 0 ships these ONLY as synthetic fixtures (no real $19 capture exists).
#[derive(Debug, Clone, Deserialize)]
pub struct DtcGolden {
    pub source: String,             // always "synthetic" in Wave 0
    pub profile_id: String,
    pub payload_hex: String,        // full $59 positive-response payload bytes
    pub expected: Vec<DtcExpected>, // ordered; pins count AND order
}

#[derive(Debug, Clone, Deserialize)]
pub struct DtcExpected {
    pub code: String,               // e.g. "P0301"
    pub gm_status_raw: u8,          // GmClass2Status.raw
    pub generic_status: String,     // GmClass2Status::generic_status() Debug form
}

/// One frozen protocol-tier golden: raw ELM response text -> stripped payload.
/// Pins the Layer 1 transform decode_elm_response_payload_for_command.
#[derive(Debug, Clone, Deserialize)]
pub struct PayloadGolden {
    pub capture: String,
    pub raw_response_text: String,  // verbatim `R` line text from the capture
    pub family: String,             // "J1850"
    pub skip_bytes: usize,          // 3 for service 0x22
    pub echo_command: String,       // the `W` command, e.g. "221540"
    pub expected_payload_hex: String,
}

/// THE single shared corpus loader. Every later wave (3/4/5/9) reuses this exact
/// function via `mod corpus_support;` -- no bespoke per-wave readers, so the corpus
/// is one globbing runner (plan Wave 9). Loads every `*.jsonl` under `dir`
/// (recursive) whose FILE NAME starts with `name_prefix`, deserializing each
/// non-empty line to T. Prefix globbing lets `signal-*.jsonl` and `dtc-*.jsonl`
/// coexist in one FLAT profile dir while still deserializing into distinct types;
/// pass `""` to load every file in a tier (e.g. all of `protocol/`). Panics with
/// the offending path+line on parse error so a malformed golden is a loud test
/// failure, not a silent skip.
pub fn load_jsonl<T: for<'de> Deserialize<'de>>(
    dir: &std::path::Path,
    name_prefix: &str,
) -> Vec<T>;

/// Resolve `tests/corpus/...` relative to CARGO_MANIFEST_DIR so tests are
/// CWD-independent.
pub fn corpus_dir() -> std::path::PathBuf; // env!("CARGO_MANIFEST_DIR")/tests/corpus

pub fn hex_to_bytes(s: &str) -> Vec<u8>;   // ignores ASCII whitespace; panics on odd/invalid
pub fn bytes_to_hex(b: &[u8]) -> String;   // uppercase, no separators
```

Decode-replay drivers (the functions under test are the CURRENT live decoders -- the test must call exactly what `session_runner.rs:417` calls so it pins the live path, not a parallel one):

```rust
// Profile-tier: mirrors the live enhanced decode path.
// obd2_dash::gm_enhanced::find_lly_did(did) -> obd2_dash::gm_enhanced::decode_did_value(def, payload)
fn decode_signal_golden(g: &SignalGolden) {
    use obd2_dash::gm_enhanced::{find_lly_did, decode_did_value};
    let def = find_lly_did(g.did).expect("LLY DID must resolve");
    let payload = hex_to_bytes(&g.payload_hex);
    let decoded = decode_did_value(def, &payload).expect("decode must succeed");
    assert_eq!(decoded.selected_raw, g.expected.selected_raw);
    assert_eq!(decoded.value.to_bits(), g.expected.value.to_bits()); // exact, NaN-safe
    assert_eq!(decoded.unit, g.expected.unit.as_str());
    // module pins the route; assert it is one of the known modules. The decode
    // assertion above is keyed on `did` only and is deliberately AGNOSTIC to
    // `signal_key`, so Wave 4 can populate signal_key without breaking this test.
    assert!(matches!(g.module.as_str(), "ecm" | "tcm"));
}

// DTC-tier: obd2_dash::gm_class2::decode_class2_dtcs(payload) -> Vec<GmClass2DtcRecord>
// Assert count, order, code, raw status, and generic_status mapping.

// Protocol-tier: obd2_core::protocol::codec::decode_elm_response_payload_for_command(...)
```

Referenced obd2-core signatures (already public; do not re-declare -- consume them):

```rust
// obd2_core::transport::parse_raw_capture  (re-exported at transport/mod.rs:15)
pub fn parse_raw_capture(path: &Path) -> std::io::Result<Vec<(String, String)>>;

// obd2_core::protocol::codec  (re-exported at protocol/mod.rs:11)
pub fn decode_elm_response_payload_for_command(
    response: &str,
    family: BusFamily,
    skip_bytes: usize,
    echo_command: Option<&str>,
) -> Result<Vec<u8>, Obd2Error>;
pub fn decode_frame(line: &str, family: BusFamily) -> Result<DecodedFrame, Obd2Error>;
pub enum BusFamily { Can, J1850, Iso9141, Kwp2000 }
```

Referenced dash library signatures (already public per `lib.rs`):

```rust
// obd2_dash::gm_enhanced
pub fn find_lly_did(did: u16) -> Option<&'static GmDidDefinition>;          // :613
pub fn decode_did_value<'a>(definition: &'a GmDidDefinition, payload: &[u8])
    -> Result<GmDecodedValue<'a>, GmEnhancedDecodeError>;                   // :666
pub struct GmDecodedValue<'a> { pub definition, pub selected_raw: u32, pub value: f64, pub unit: &'static str }

// obd2_dash::gm_class2
pub fn decode_class2_dtcs(payload: &[u8]) -> Result<Vec<GmClass2DtcRecord>, GmClass2DecodeError>; // :220
```

Seeding tool signature (in `tests/seed_corpus.rs`, behind `#[ignore]`):

```rust
// Walks raw-captures/, pairs W/R via parse_raw_capture, tracks the most-recent
// ATSH header per Mode 22 read, derives payload via the Layer 1 strip, computes
// expected via the live decoder, and writes JSONL to tests/corpus/.staging/.
#[test]
#[ignore = "dev-only corpus seeder; writes to tests/corpus/.staging, never the frozen tree"]
fn seed_lly_corpus_from_raw_captures();
```

OWL on the seeder pairing logic (`parse_raw_capture` pairs W->R strictly sequentially):
- It MUST filter `W` lines: keep only commands starting with `"22"` (Mode 22 enhanced), and separately `"0902"` (VIN anchor). Discard `AT*` and `OK`-paired lines.
- For each kept Mode 22 pair it MUST validate the `R` actually echoes the request before trusting it: run `decode_elm_response_payload_for_command(R, BusFamily::J1850, 3, Some(W))`; if it errors (negative response `7F`, `NO DATA`, wrong-DID echo) the pair is dropped and logged, never written as a positive golden. A `W` with a missing `R` will mis-pair with the next `R` -- this validation is the only thing that stops a desynced pair from poisoning the corpus.
- It MUST track the most recent `ATSH......` write and record it as `request_header_hex` so the route (node = header byte[1], e.g. `6C18F1` -> TCM `0x18`) is pinned for later waves.
- It MUST set `module` from that tracked header node (byte[1] `== 0x10` -> `"ecm"`, `0x18` -> `"tcm"`) and MUST leave `signal_key` null/absent (Wave 4 owns populating it once `LLY_SIGNALS` exists). It writes `signal-*.jsonl` and `dtc-*.jsonl` into `tests/corpus/.staging/` mirroring the canonical FLAT layout, never the frozen tree.

### Tests

Unit:
- `corpus_support::tests::hex_roundtrip` -- `bytes_to_hex(hex_to_bytes(x)) == x` for sample LLY payloads; `hex_to_bytes` panics on odd-length and on non-hex.
- `corpus_support::tests::load_jsonl_rejects_malformed` -- a malformed line panics with the path and line number (no silent skip). Exercise via `load_jsonl::<SignalGolden>(tmp_dir, "")` over a staged bad line.

Golden-corpus (the primary firewall; all read only the committed tree):
- `lly_signal_corpus::every_signal_golden_decodes_identically` -- loads `corpus/profile/gm.gmt800.lly.class2/` with prefix `"signal-"`, runs `decode_signal_golden` on each, asserts `selected_raw`, `value.to_bits()`, and `unit` match. Asserts the loaded set is non-empty AND covers exactly the confirmed-present DIDs `{0x1540, 0x1543, 0x162F}` (guards against an empty/over-claimed corpus).
- `lly_signal_corpus::corpus_dids_are_all_known_lly` -- every `did` in the corpus resolves via `find_lly_did` and is NOT in `LLY_REJECTED_DIDS` (catches a future fixture that pins a rejected DID).
- `lly_dtc_corpus::synthetic_dtc_goldens_decode_identically` -- loads `corpus/profile/gm.gmt800.lly.class2/` with prefix `"dtc-"`, runs `decode_class2_dtcs`, asserts ordered records (count + code + `gm_status_raw` + `generic_status`). Each entry's `source` MUST equal `"synthetic"` (test asserts this, so no one can sneak a fake "real" DTC golden into Wave 0).
- `protocol_payload_corpus::strip_is_byte_stable` -- loads `corpus/protocol/` with prefix `""` (recursive over `j1850-vpw`; a future `can-11bit` tier is picked up additively). For each `PayloadGolden`, `decode_elm_response_payload_for_command(raw, J1850, skip, Some(echo))` equals `expected_payload_hex` byte-for-byte. This pins the Layer 1 transform the profile decoder depends on; if obd2-core changes skip/strip behavior, this fails before the value test does, isolating the layer.

Architectural (the bypass allowlist; the wave's named deliverable):
- `architecture::live_dashboard_has_no_new_raw_routed_callers` -- reads the live file set as strings via `CARGO_MANIFEST_DIR`-relative paths and asserts each per-file, per-symbol occurrence count is `<=` its frozen UPPER BOUND (NOT `==`; see OWL below). Frozen upper bounds (verified this wave):
  - `src/session_runner.rs`: `find_lly_did(` <= 1 (currently line 417), `.raw_request(` <= 3 (currently lines 411, 512, 564).
  - `src/session_runner.rs`: `.adapter_mut(` <= 0, `class2_routed_request(` <= 0, `class2_dtc_all_request(` <= 0, `class2_dtc_active_request(` <= 0, `.routed_request(` <= 0.
  - `src/app.rs`, `src/main.rs`, `src/domain.rs`, `src/vehicle_data.rs`, `src/mock_profile.rs`, `src/tui/*.rs`, `src/widget/*.rs`: ALL of `find_lly_did(`, `.raw_request(`, `.routed_request(`, `class2_routed_request(`, `class2_dtc_all_request(`, `class2_dtc_active_request(`, `.adapter_mut(` <= 0.
  The allowlist is a `const &[(&str, &str, usize)]` of `(relative_path, needle, max_count)`; the assertion is `actual <= max_count`. ADDING a call site pushes `actual` above the bound and fails red. The failure message must say: "to add a manufacturer-routed call site you must raise this bound deliberately (review required); to MOVE/REMOVE one you must LOWER the matching bound in the same commit. This is the Wave 0 freeze; do not delete this test."

  OWL (why upper bound, not equality): the migration is real. Waves 3/4/5 DELETE `find_lly_did(`:417 and the `.raw_request(` sites as they move dispatch behind the profile runtime, and Wave 6 deletes the GUI `request_gm_node`. With exact equality, that legitimate deletion drops the count BELOW the frozen number and turns Wave 0 RED on the very change it is meant to allow. A careless implementer then "fixes" the red by deleting the architectural test -- destroying the firewall. A non-increasing upper bound (`count <= frozen`) stays GREEN on removal, so nobody is tempted to delete the guard. The cost is that a removal leaves the bound loose (over-permissive), which is why the DECREMENT CONTRACT is mandatory: the migrating wave LOWERS the matching bound (and updates `docs/diagnostics/gm-lly-call-site-inventory.md`) in the SAME commit, keeping the bound a tight upper bound that still catches re-introduction. No wave may RAISE a bound without explicit review. Hand-off owners: Wave 3/4/5 decrement the `session_runner.rs` `find_lly_did(`/`.raw_request(` bounds as they each remove their assigned site; Wave 6 decrements the GUI bounds below.
- `architecture::gm_library_modules_are_the_only_definers` -- asserts `class2_routed_request`/`class2_header`/`find_lly_did` are DEFINED only in `src/gm_class2.rs`/`src/gm_enhanced.rs` (pins the quarantine target location so a later wave can move it wholesale). (When a later wave relocates these definitions wholesale into the profile runtime, it updates this test's expected definer file in the same commit, per the same hand-off discipline as the allowlist.)
- (SIBLING crate) `apps/obd2-gui/src-tauri/tests/architecture::gui_request_gm_node_is_frozen` -- pins the GUI baseline as UPPER BOUNDS (same `count <= frozen` rule, same reason): `fn request_gm_node` defined <= 1 (currently main.rs:821), `adapter_mut().routed_request` <= 1, `find_lly_did(` and `.read_enhanced(` <= their current counts. Wave 6 removes `request_gm_node` and the GUI routed calls; it LOWERS these bounds in the same commit. This MUST live in the GUI crate; see OWL.

Integration-with-mock:
- Deliberately NONE for byte-accurate LLY replay. OWL: `MockAdapter::routed_request` (mock.rs:300) downgrades every `PhysicalTarget::Addressed` to `Broadcast` and re-dispatches through `request`, and returns `Obd2Error::NoData` for addressed services `0x03|0x07|0x0A`. It consults no response table. Replaying real LLY J1850 traffic through it would fabricate wrong bytes and produce a fake green. Wave 0 therefore drives the corpus through the decoders directly (the functions the live path calls), not through an adapter. A real adapter-level replay needs a new fixture transport seeded from `parse_raw_capture` -> `MockTransport::expect()`; that is later-wave work, explicitly out of scope here.

### Acceptance criteria

- [ ] `cargo test -p obd2-dash` passes with the new test files present and zero changes to any `src/*.rs`.
- [ ] `cargo test -p obd2-core` still passes unchanged (Wave 0 adds no obd2-core code).
- [ ] The existing LLY golden corpus stays green with zero diffs -- in Wave 0 this means: the newly committed goldens are the baseline, and a second full `cargo test` run produces identical output with no fixture rewrite. (There is no prior corpus to diff against; Wave 0 establishes it. Every later wave is graded "zero diffs against THIS corpus.")
- [ ] `tests/architecture.rs` fails if a single new `find_lly_did(`, `.raw_request(`, `.routed_request(`, `class2_*`, or `.adapter_mut(` call is ADDED to any live dashboard file (the occurrence count exceeds its frozen upper bound). Verify by adding one in a throwaway commit, seeing red, reverting.
- [ ] Removing or moving a live call site (the intended Wave 3/4/5 dash migration and Wave 6 GUI removal) does NOT turn `architecture.rs` red: the per-symbol bound is a non-increasing upper bound (`count <= frozen`), so a legitimate removal drops below the bound and stays green. The migrating wave MUST decrement the matching bound (and update the inventory doc) in the SAME commit, so the allowlist stays a tight upper bound and cannot silently re-admit a removed bypass. Raising any bound is forbidden without explicit review.
- [ ] `SignalGolden` carries `signal_key` (`Option<String>`, null/absent in every Wave 0 fixture; Waves 4/5 populate it additively once `LLY_SIGNALS` exists -- no rewrite of frozen files) and `module` (the ecm/tcm route derived from `request_header_hex` byte[1]). The Wave 0 decode test asserts `selected_raw`/`value`/`unit` only and is agnostic to `signal_key`, so Wave 4 can fill it without breaking this wave's assertions.
- [ ] The corpus uses ONE canonical, flat-per-profile layout (`protocol/<family>/*.jsonl` and `profile/<profile_id>/signal-*.jsonl` + `dtc-*.jsonl`) read by ONE shared loader (`corpus_support::load_jsonl(dir, name_prefix)`). Every later wave (3/4/5/9) declares `mod corpus_support;` and reuses that loader -- no bespoke per-wave readers, one globbing runner. Adding goldens (more lines, more files, a new `protocol/<family>/` dir such as `can-11bit`) is purely additive and never restructures or rewrites a frozen file.
- [ ] Signal corpus covers exactly the confirmed-present DIDs `{0x1540, 0x1543, 0x162F}` from real captures, and the corpus README enumerates the NOT-covered plan DIDs (`0x1251`, `0x1542`, `0x163D`, `0x163E`, `0x1470`, `0x1940`, injector pulse width `0x1193..0x119A`, injector balance `0x1630..0x1636`) as gaps to be filled when a capture with a positive `62 <did>` response or a deliberate synthetic fixture is added.
- [ ] No `$19`/`$59` golden is labeled "real"; every DTC golden has `source: "synthetic"` and the test enforces it.
- [ ] No corrupted-VIN negative golden is seeded from the `1IGTHKI...` files -- the README states their `0902` payloads are byte-identical to the clean capture and decode cleanly, so they cannot anchor a "corrupted -> not exact" negative; that belongs to Phase 1/2 with synthesized input.
- [ ] The seeder writes only to `tests/corpus/.staging/`; `.staging/` is gitignored; replay tests never read it.
- [ ] `value` comparisons use `f64::to_bits()` equality (NaN-safe, exact), not epsilon.
- [ ] Corpus paths resolve via `env!("CARGO_MANIFEST_DIR")`, so tests pass regardless of CWD and regardless of `StorageManager` FIFO trimming of live `raw-captures/` (the frozen corpus is committed under `tests/`, never read from `raw-captures/` at test time).
- [ ] `docs/diagnostics/gm-lly-call-site-inventory.md` lists the same `(file, symbol, max-count)` triples the architectural test pins, states the test is authoritative, and documents the DECREMENT CONTRACT: any wave that moves or removes a call site lowers the matching upper bound in the same commit; the bound is never raised without review.

### Rollback notes

- Fully additive and independently shippable: Wave 0 creates only `tests/`, `tests/corpus/`, two docs, and a no-dependency `Cargo.toml` touch. Reverting is `git revert` of the wave commit (or `rm -r crates/obd2-dash/tests docs/diagnostics/gm-lly-call-site-inventory.md` plus restoring `Cargo.toml`). No `src/*.rs` changes means there is nothing to roll back in the running binary.
- No feature flag needed because nothing here is conditionally compiled into the binary -- it is all `tests/` (integration test crates) that only build under `cargo test`. Because the allowlist is a non-increasing upper bound, the very next wave's legitimate removals do NOT turn it red, so the `#[ignore]` escape hatch should rarely be needed; if the architectural test ever does prove too noisy during a refactor, quarantine it with `#[ignore]` rather than deleting it, preserving the frozen allowlist data (and the decrement contract) for re-enable.
- The seeder (`#[ignore]`) is inert in CI by construction; it can be deleted independently of the corpus it produced without affecting the replay tests.
- Independent shippability check: this wave can merge to `master` on its own and provides immediate value (a regression baseline + a bypass tripwire) even if no further wave ever lands. It introduces no new public library API and no behavior change, so it cannot regress LLY behavior -- by design it only observes it.
- OWL residual risk to flag at handoff: the GUI architectural guard lives in a separate crate and is brittle if `apps/obd2-gui/src-tauri` is restructured; if the GUI crate is not part of the standard `cargo test` invocation, that guard can silently not run. The dash-side `architecture.rs` is the reliable one; the GUI guard is best-effort until Wave 6 removes `request_gm_node` outright (and decrements its bound to 0 in the same commit).

Verified against obd2-core source: `ModuleId(pub String)` derives `Debug, Clone, PartialEq, Eq, Hash` with `ModuleId::new(impl Into<String>)`; `Protocol` is `Copy + non_exhaustive`; `VehicleSpec` derives `Clone` (no Arc needed). All corrections applied below.

## Wave 1: Profile Model

### Objective

Introduce a self-contained `profiles::model` type layer (plus an empty `profiles::registry` skeleton) that defines AND OWNS THE FINAL SHAPE of every Layer-3 vocabulary type the migration needs -- `ProfileId`, `Manufacturer`, `VehicleContext`, `ProfileMatch`, the sealed `SelectedProfile`, the `DiagnosticProfile` trait, and its data types (`SignalDefinition`, `DtcServiceDefinition`, `ActiveTestDefinition`, the closed-enum `ModuleKey`, module-only `RouteDefinition`, the `ModuleMap`/`ModuleDefinition`/`BusDefinition`/`AddressTemplate` model and its `AddressState`/`ModuleEvidenceState`/`PassiveCapabilityState` enums per the module-support-architecture doc, and `SourceFields`). These are pure data and one trait with zero logic; nothing in the existing TUI/session/GM code references them, so there is provably no behavior change.

This wave is the SINGLE OWNER of the shared model. Every shared type is pinned here in its FINAL shape (see "Type ownership contract" below). Later waves (registry resolver, session-owned selection, dispatcher, GM data migration, evidence, recording v3, active tests, proof profile) must consume these types AS-IS and may only extend them additively after this section is updated first. No later wave may silently redefine a field, variant, name, or signature. This rule exists because the review found DecodedSignal, SourceFields, MatchConfidence, IdentityConfidence, EvidencePolicy, RouteSet, Provenance, and ProfileDecodeError each defined or redefined differently across Waves 1/2/4/5/8/9, with several references (`MatchConfidence::High`, `EvidencePolicy::BoundedLive`, `Provenance::LocalFixture`, `RouteSet::single`, `ProfileDecodeError::Decode`) pointing at variants no wave defined. If Wave 1 does not pin these, several later waves will not compile.

### Depends on

- Nothing. This is the first wave and a pure leaf module. It compiles against the existing `obd2_core::vehicle` types (`Protocol`, `VehicleSpec`, `ModuleId`) that already exist in the workspace. It must land before every later wave because all of them consume these types.
- VERIFIED prerequisites in `obd2_core::vehicle::mod.rs`: `ModuleId(pub String)` derives `Debug, Clone, PartialEq, Eq, Hash` and exposes `ModuleId::new(impl Into<String>)` (NOT `const`); `Protocol` derives `Copy` and is `#[non_exhaustive]`; `VehicleSpec` derives `Clone` (so `Option<VehicleSpec>` by value is fine -- no `Arc` required). `DecodedSignal` below relies on `ModuleId: Clone + Debug + PartialEq`, which is satisfied.
- Explicitly does NOT depend on the GM modules (`gm_enhanced`, `gm_class2`, `gm_active`, `gm_evidence`). Wave 1 must not import them, and they must not import Wave 1. Keeping the dependency edge empty in both directions is what guarantees the "NO behavior change" property and is asserted by an architectural test below.

### Files touched

- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/mod.rs` -- module root. Declares `pub mod model;` and `pub mod registry;`, re-exports the commonly used types (`pub use model::{...}; pub use registry::ProfileRegistry;`) INCLUDING the new owner types `RxdSource`, `RouteScope`, and `Selection`, and carries a module-level `#![allow(dead_code)]` (see Acceptance criteria -- without it, every unused type fails a `-D warnings` build). Add a `//! Wave 1: pure profile model. Single owner of all shared Layer-3 types.` doc header.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/model.rs` -- all pure types and the `DiagnosticProfile` trait. No `impl` logic except trivial accessors and `const fn`/builder-free constructors. Inline `#[cfg(test)]` unit tests.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/registry.rs` -- the `ProfileRegistry` skeleton. Storage is `Vec<&'static dyn DiagnosticProfile>` (NOT `Vec<Box<dyn ...>>` -- profiles are compile-time-static data: every `SignalDefinition`/`DtcServiceDefinition` field is `&'static`, so a profile is declared as a `static` and registered by reference. Wave 9 registers `&FIXTURE_PROFILE`, never a `Box`). `register(&mut self, profile: &'static dyn DiagnosticProfile)`. Also exposes `new`, `get`, `profiles`, `is_empty`, `len`, and the `select` signature (body `unimplemented!()`; Wave 2 fills it in). The `select` body is the ONLY non-trivial method and it is never reached in Wave 1 (asserted by the architectural test). Inline `#[cfg(test)]` tests with a local `static` stub profile to exercise `register`/`get`.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/wave1_architecture.rs` -- integration test (new `tests/` dir; none exists today) that reads the `src/` tree and asserts no live module references `profiles` and that `profiles/` does not reference the `gm_*` modules. Pure `std::fs` string scan, zero new dependencies.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/lib.rs` -- add exactly one line `pub mod profiles;` to the existing four `pub mod` declarations. This is the only edit to existing code.
- NO MODIFY anywhere else. `session_runner.rs`, `app.rs`, `main.rs`, `tui/*`, `widget/*`, `domain.rs`, `gm_*.rs`, `recording/*`, `mock_profile.rs` are untouched. If any of these files changes in this wave, the wave is out of scope and the "zero behavior change" guarantee is void.

### Exact APIs

All in `crates/obd2-dash/src/profiles/model.rs` unless noted. Paths are pinned to the inventory: `ModuleId` is `obd2_core::vehicle::ModuleId` (NOT `obd2_core::domain::ModuleId` -- the plan's Core Types snippet at lines 145-153 names a `domain` module that does NOT exist in obd2-core; `lib.rs` there declares only `adapter/error/protocol/session/specs/store/transport/vehicle`. Using the plan's path verbatim will not compile.)

#### Type ownership contract (READ FIRST -- this is the single source of truth)

Each shared type below is in its FINAL shape. The right column lists the WRONG/divergent forms the review found in later-wave drafts; those drafts must be corrected to consume the Wave-1 shape. No later wave may add a field/variant/method to these types without amending THIS section first (so a reviewer catches the redefinition). There are NO pre-approved pending additive changes; if a later wave genuinely needs to extend one of these, it updates this contract and the change is reviewed here.

- `DecodedSignal` -- FINAL: `{ key, value, unit, raw: Vec<u8>, selected_raw: Vec<u8>, module: ModuleId, confidence: Confidence }`. Carries BOTH the full raw payload (invariant 8 -- NEVER drop it) AND the Wave-4 decode metadata. Wave 4 MUST NOT "reproduce" a `{key,value,unit,selected_raw,module,confidence}` variant that drops `raw`.
- `SourceFields` -- FINAL: `txd` is non-optional, `rxd: Option<RxdSource>` (struct, preserves the `"3008"` caveat + bit width), `raw_mth` (renamed from `mth`). Wave 4 MUST NOT keep `txd: Option<...>`, a bare `rxd: Option<&str>`, or the old `mth` name.
- `MatchConfidence` -- FINAL three variants: `ProtocolPlusVinDimension`, `VinExact`, `VinPlusSpec`. The Wave-1-draft names `VinDerived`/`SpecConfirmed`/`VinAndSpec` are RETIRED. Wave 9's `MatchConfidence::High` is NOT a variant and MUST be replaced by `VinPlusSpec`.
- `IdentityConfidence` -- FINAL four variants: `Unread`, `Corrupted`, `Single`, `Confirmed`, plus `const fn is_trusted()`. The 5th draft variant `ManualConfirmed` is RETIRED; manual confirmation is now `manual_confirmed: bool` on `SelectedProfile`. Wave 2 computes the values; it MUST NOT redefine the variant set. Wave 9 consumes `is_trusted()`.
- `EvidencePolicy` -- FINAL: `None`, `OnError`, `OnDemand`, `BoundedLive`, `Always`. The `BoundedLive` variant (Phase 6 "bounded live evidence") is what Wave 7 and Wave 9 (`EvidencePolicy::BoundedLive`) require; it is a unit variant so it can be written as a value.
- `RouteSet` / `RouteScope` -- FINAL: `RouteSet { scope: RouteScope }` with `RouteScope::{ Single(RouteDefinition), Explicit(&'static [RouteDefinition]), DiscoveredOnBus { bus: BusKey } }`, plus `const fn single` / `const fn explicit` / `const fn discovered_on_bus`. `DiscoveredOnBus` is the zero-diff DTC fan-out policy; it expands against live discovered modules on that bus and does NOT put bus/address back on `RouteDefinition`. Wave 5 MUST NOT redefine `RouteSet`; Wave 9's `RouteSet::single(..)` resolves to the constructor here.
- `Provenance` -- FINAL: `ScanGaugePublished`, `LiveObserved`, `LegacySpec`, `LocalRejection`, `LocalFixture`. The `LocalFixture` arm exists for Wave 9's proof/fixture profile.
- `ProfileDecodeError` -- FINAL: `PayloadTooShort { expected, got }` (STRUCT form -- Wave 9 MUST NOT use a unit `PayloadTooShort`), `MismatchedResponse`, `UnknownDecoder(&'static str)` (decoder ids are static metadata -- lower-allocation; matches Wave 9; never `.to_string()` a `decoder_id`), `NegativeResponse { service: u8, nrc: u8 }` (Wave 5's checked `$19` decoder returns this for a leading-`0x7F` reply -- see Wave 5), `Decode(String)` (general decode failure -- Wave 5's `::Decode`), `Other(String)`.
- `SelectedProfile` -- FINAL: minted ONLY via `pub(in crate::profiles) fn seal(profile_id, context_generation, manual_confirmed)`. Wave 2 MUST NOT widen the seal to `pub(crate)` -- `registry.rs` is already inside `crate::profiles`, so `pub(crate)` is unnecessary and would let `session_runner`/`app`/`gm_*` fabricate a token, defeating resolver-only minting (plan line 87).
- `ProfileRegistry` -- FINAL storage `Vec<&'static dyn DiagnosticProfile>` (NO `Box`; profiles are static; `register(&mut self, &'static dyn DiagnosticProfile)`); FINAL method set `new/register/get/profiles/is_empty/len/select`. `SafetyClass`, `ActiveCommandProfile`, `PollCadence`, `FailurePolicy`, `Confidence`, `SignalCategory`, `BackoffPolicy` are also owned here and may not be redefined elsewhere.
- Module-map types (CANONICAL shapes in `2026-06-29-module-support-architecture.md`; OWNED here in `profiles::model`, consumed as-is by later waves): `ModuleKey` (CLOSED enum `{Ecm,Tcm,Ficm,Bcm,Ebcm,Ipc,Sdm,Hvac}`, NOT a `&'static str` newtype), `RouteDefinition { module: ModuleKey }` (module-only; NOT `{bus,address,module}`), `BusDefinition`, `ModuleDefinition`, `ModuleMap`, `AddressTemplate` (lives on `ModuleDefinition`, not on the route), and the three orthogonal state enums `AddressState`, `ModuleEvidenceState`, `PassiveCapabilityState`. Wave 2/3/5 MUST consume these; none may redefine `ModuleKey` as stringly-typed or re-add `bus`/`address` to `RouteDefinition`.

```rust
use obd2_core::vehicle::{ModuleId, Protocol, VehicleSpec};

// ---- Identity ----------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProfileId(pub &'static str);

impl ProfileId {
    pub const fn new(id: &'static str) -> Self { Self(id) }
    pub const fn as_str(&self) -> &'static str { self.0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Manufacturer {
    Gm,
    Ford,
    ChryslerRam,
    Generic,
}

// VIN/identity confidence (plan: Vehicle Identity Lifecycle, lines 280-309).
// FINAL shape owned here; Wave 2 computes the values. Manual confirmation is
// NOT a variant -- it is `manual_confirmed: bool` on SelectedProfile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityConfidence {
    Unread,     // no VIN read yet / discovery incomplete
    Corrupted,  // failed sanity check (e.g. I/1 confusion) -- MUST NOT yield a match
    Single,     // one unverified VIN read
    Confirmed,  // VIN agrees with itself / decoded spec
}

impl IdentityConfidence {
    // True when identity is strong enough to trust WITHOUT a manual override.
    // Wave 9 consumes this. A weak identity can still be polled if the user
    // sets SelectedProfile.manual_confirmed, which is intentionally separate.
    pub const fn is_trusted(&self) -> bool {
        matches!(self, IdentityConfidence::Confirmed)
    }
}

// ---- Match result ------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchConfidence {
    ProtocolPlusVinDimension, // floor: protocol + exactly one VIN-derived dimension
    VinExact,                 // VIN decodes unambiguously to this profile
    VinPlusSpec,              // VIN AND decoded VehicleSpec agree (strongest)
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProfileMatch {
    Exact { confidence: MatchConfidence },
    Partial { reason: String },
    NoMatch,
}

// ---- Vehicle context (immutable identity snapshot) ---------------------

#[derive(Clone, Debug)]
pub struct VehicleContext {
    pub generation: u64,
    pub protocol: Protocol,                // obd2_core::vehicle::Protocol (Copy)
    pub vin: Option<String>,
    pub vin_confidence: IdentityConfidence,
    pub spec: Option<VehicleSpec>,         // obd2_core::vehicle::VehicleSpec (Clone, by value)
    pub discovered_modules: Vec<ModuleId>, // obd2_core::vehicle::ModuleId
    pub active_bus: Option<String>,        // BusId.0 string (plan uses Option<String>)
}

// ---- Sealed selected-profile token -------------------------------------

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedProfile {
    profile_id: ProfileId,
    context_generation: u64,
    manual_confirmed: bool, // user deliberately chose this profile under weak identity
    _sealed: (),            // private => external crates/modules cannot fabricate one
}

impl SelectedProfile {
    // ONLY mintable from within the profiles module tree (the Wave-2 resolver in
    // registry.rs, which IS inside crate::profiles). pub(in crate::profiles) is the
    // seal. Wave 2 MUST NOT widen this to pub(crate) or pub -- doing so lets any
    // dash module fabricate a token and defeats resolver-only minting (plan line 87).
    pub(in crate::profiles) fn seal(
        profile_id: ProfileId,
        context_generation: u64,
        manual_confirmed: bool,
    ) -> Self {
        Self { profile_id, context_generation, manual_confirmed, _sealed: () }
    }
    pub fn profile_id(&self) -> ProfileId { self.profile_id }
    pub fn context_generation(&self) -> u64 { self.context_generation }
    pub fn manual_confirmed(&self) -> bool { self.manual_confirmed }
}

// Result of resolving a VehicleContext against the registry. Wave 2 computes
// these; Wave 1 pins the shape so no later wave redefines select's return type.
#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    // Exact match -> a sealed token is minted (manufacturer requests allowed).
    Matched(SelectedProfile),
    // Partial match (invariant 5): visible for display but MUST NOT poll
    // manufacturer-specific requests. NO token is minted.
    Partial { profile_id: ProfileId, reason: String },
    // No profile matched; generic OBD-II only.
    None,
}
```

Routing (Layer 3 declares intent only; Layer 2 resolver maps to `PhysicalAddress` in a later wave -- NOT here):

```rust
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BusKey(pub &'static str);

impl BusKey {
    pub const fn new(key: &'static str) -> Self { Self(key) }
    pub const fn as_str(&self) -> &'static str { self.0 }
}

// CANONICAL per 2026-06-29-module-support-architecture.md: a closed, OEM-neutral
// functional-role enum. A free-form string key is the leakage the plan warns about
// and breaks the resolver's exhaustive `match`; growth is additive (role = variant).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModuleKey { Ecm, Tcm, Ficm, Bcm, Ebcm, Ipc, Sdm, Hvac }

impl ModuleKey {
    /// THE canonical module string (obd2-core ModuleId strings; Ebcm->"abs", Sdm->"airbag").
    /// Wave 3 consumes this.
    pub const fn canonical(self) -> &'static str {
        match self {
            ModuleKey::Ecm => "ecm",  ModuleKey::Tcm => "tcm",   ModuleKey::Ficm => "ficm",
            ModuleKey::Bcm => "bcm",  ModuleKey::Ebcm => "abs",  ModuleKey::Ipc => "ipc",
            ModuleKey::Sdm => "airbag", ModuleKey::Hvac => "hvac",
        }
    }
    pub fn to_core_module_id(self) -> obd2_core::vehicle::ModuleId {
        obd2_core::vehicle::ModuleId::new(self.canonical())
    }
}

// CANONICAL per the module-support-architecture doc: module reference ONLY. Bus +
// address live once in the profile ModuleMap (ModuleDefinition), never copied onto
// each route. `gm_enhanced` today repeats the ECM node in 22 of 24 DID entries --
// exactly the duplication this kills.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RouteDefinition {
    pub module: ModuleKey, // identity always from the route; NEVER a display fallback
}

// Deliberately NOT #[non_exhaustive] and deliberately NO J1939 arm.
// Exhaustive => the future Layer-2 resolver's `match` is a hard compile error
// until a new arm is handled (the intended additive-arm firewall). J1939 is
// omitted because obd2-core's ELM apply_target errors on PhysicalAddress::J1939
// (elm327.rs:258); declaring a J1939 route would be undecodable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressTemplate {
    J1850 { node: u8 }, // header [0x6C, node, 0xF1] synthesized by the resolver, NOT here
    Can11 { request_id: u16, response_id: u16 },
    Can29 { request_id: u32, response_id: u32 },
}

// FINAL shape. Wave 5 MUST NOT redefine RouteSet; it consumes RouteScope here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteScope {
    Single(RouteDefinition),
    Explicit(&'static [RouteDefinition]), // fixed list of module-only routes
    DiscoveredOnBus { bus: BusKey },       // runtime expands through discovery
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteSet {
    pub scope: RouteScope,
}

impl RouteSet {
    pub const fn single(route: RouteDefinition) -> Self {
        Self { scope: RouteScope::Single(route) }
    }
    pub const fn explicit(routes: &'static [RouteDefinition]) -> Self {
        Self { scope: RouteScope::Explicit(routes) }
    }
    pub const fn discovered_on_bus(bus: BusKey) -> Self {
        Self { scope: RouteScope::DiscoveredOnBus { bus } }
    }
}
```

Vendor-auditable source fields (plan lines 222-226 -- preserve TXD/RXF/RXD/MTH after decoders move behind `decoder_id`; keep the fuel-rail `RXD=3008` caveat inspectable). FINAL shape: `txd` non-optional, `rxd` is the `RxdSource` struct, `mth` renamed to `raw_mth`:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxdSource {
    pub raw: &'static str,        // string-preserving (e.g. "3008" range caveat)
    pub bit_width: Option<u8>,    // decoded bit width when published
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct SourceFields {
    pub txd: &'static str,                 // non-optional (FINAL); "" when not published
    pub rxf: Option<&'static str>,
    pub rxd: Option<RxdSource>,            // None when the signal publishes no RXD
    pub raw_mth: Option<&'static str>,     // raw MTH as published (renamed from `mth`)
    pub source_ref: Option<&'static str>,  // URL / document id / "scangauge"
}
```

Capability/poll vocab. The profile model defines its OWN `Confidence`/`Provenance`/`PollCadence`/`FailurePolicy` to avoid binding Layer 3 to either existing enum. These COLLIDE BY NAME with `gm_enhanced::Confidence` and `obd2_core::protocol::enhanced::Confidence` (and with `gm_enhanced::{Provenance, PollCadence, FailurePolicy}`); always path-qualify. The GM->profile mapping is a later wave's job.

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalCategory { Powertrain, Turbo, Fuel, Transmission, Body, Chassis, Emissions, Other }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollCadence { Fast, Medium, Slow, OnDemand }

// Mirrors gm_enhanced::Confidence variants so LLY signals migrate losslessly later.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence { Candidate, LiveObserved, Community, Verified, Rejected }

// FINAL: includes LocalFixture for Wave 9's proof/fixture profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provenance { ScanGaugePublished, LiveObserved, LegacySpec, LocalRejection, LocalFixture }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailurePolicy { SurfaceUnavailable, PreferStandardPid, CandidateOnly, DoNotPoll }

// FINAL: BoundedLive is the Phase-6 "bounded live evidence" policy that Wave 7
// and Wave 9 (EvidencePolicy::BoundedLive) require. Unit variant so it is a value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidencePolicy { None, OnError, OnDemand, BoundedLive, Always }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackoffPolicy {
    pub skip_after_misses: u8, // consecutive NoData/Unsupported before skipping
    pub max_skips: u8,         // GM Class2 today caches for 3 skips (gm backoff)
}
impl BackoffPolicy {
    pub const NONE: Self = Self { skip_after_misses: 0, max_skips: 0 };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyClass { Passive, StationaryOnly, IdleOnly, Locked }
```

Capability definitions (the named Wave-1 deliverables) plus the small stub types the trait pulls in. Everything here is pure data:

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub category: SignalCategory,
    pub route: RouteDefinition,
    pub service_id: u8,
    pub request_data: &'static [u8],   // service payload bytes only; no header/framing
    pub decoder_id: &'static str,
    pub unit: &'static str,
    pub cadence: PollCadence,
    pub confidence: Confidence,
    pub provenance: &'static [Provenance],
    pub source_fields: SourceFields,
    pub evidence_policy: EvidencePolicy,
    pub failure_policy: FailurePolicy,
    pub preferred_over: Option<&'static str>, // other signal key (generic-rail preference)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DtcServiceDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub route_set: RouteSet,
    pub service_id: u8,
    pub request_data: &'static [u8],
    pub decoder_id: &'static str,
    pub backoff_policy: BackoffPolicy,
}

// Stub: the LLY-justified forced-Mode-01 PID workaround moves under a profile in Phase 4.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardPidOverride {
    pub pid: u8,
    pub reason: &'static str,
}

// Stub: passive bus monitor (not yet exercised by any path).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PassiveMonitorDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub route: RouteDefinition,
    pub decoder_id: &'static str,
}

// Stub: active-test command/preconditions (fleshed out in Phase 8).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileRequestDefinition {
    pub route: RouteDefinition,
    pub service_id: u8,
    pub request_data: &'static [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivePrecondition {
    pub label: &'static str,
    pub detail: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveCommandProfile {
    Locked,                                   // no verified bytes (LLY VGT today)
    Verified(ProfileRequestDefinition),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

Decode results / error and the trait. `DecodedSignal` carries BOTH the raw bytes (plan invariant 8 -- evidence must preserve raw bytes; this field is NEVER dropped) AND the Wave-4 decode metadata, so Wave 4 reuses this exact struct instead of reproducing one that loses `raw`:

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedSignal {
    pub key: &'static str,
    pub value: f64,
    pub unit: &'static str,
    pub raw: Vec<u8>,         // invariant 8: full post-skip payload as received (NEVER dropped)
    pub selected_raw: Vec<u8>, // the specific bytes the decoder consumed (populated Wave 4+)
    pub module: ModuleId,     // responding module, obd2_core::vehicle::ModuleId (Wave 4+)
    pub confidence: Confidence, // per-sample confidence (Wave 4+)
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedDtc {
    pub code: String,
    pub status_raw: Option<u8>,       // Some(GM Class2 status); None for SAE
    pub status_flags: Vec<String>,
    pub raw: Vec<u8>,                 // invariant 8
}

// FINAL variant set. PayloadTooShort is the STRUCT form (Wave 9 must not use a
// unit variant); UnknownDecoder is &'static str (decoder ids are static metadata; matches Wave 9); Decode is Wave 5's ::Decode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProfileDecodeError {
    PayloadTooShort { expected: usize, got: usize },
    MismatchedResponse,
    UnknownDecoder(&'static str),
    NegativeResponse { service: u8, nrc: u8 }, // Wave 5: leading-0x7F (7F 19 nrc) guard
    Decode(String),          // general decode failure (Wave 5 `ProfileDecodeError::Decode`)
    Other(String),
}

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

Registry skeleton in `crates/obd2-dash/src/profiles/registry.rs`. Storage is `&'static dyn DiagnosticProfile` (profiles are compile-time-static data; NO `Box`) so Wave 9 registers `&FIXTURE_PROFILE`; `get` returns a borrow for Wave 3; `select` is signature-only (body `unimplemented!()`) and is filled by Wave 2:

```rust
use super::model::{DiagnosticProfile, ProfileId, Selection, VehicleContext};

pub struct ProfileRegistry {
    profiles: Vec<&'static dyn DiagnosticProfile>,
}

impl ProfileRegistry {
    pub fn new() -> Self { Self { profiles: Vec::new() } }

    // Wave 9 fixture profile: registry.register(&FIXTURE_PROFILE) -- a &'static, never a Box.
    pub fn register(&mut self, profile: &'static dyn DiagnosticProfile) {
        self.profiles.push(profile);
    }

    pub fn profiles(&self) -> &[&'static dyn DiagnosticProfile] { &self.profiles }

    // Wave 3: registry.get(ProfileId) -> Option<&dyn DiagnosticProfile>.
    pub fn get(&self, id: ProfileId) -> Option<&dyn DiagnosticProfile> {
        self.profiles.iter().find(|p| p.id() == id).copied()
    }

    pub fn is_empty(&self) -> bool { self.profiles.is_empty() }
    pub fn len(&self) -> usize { self.profiles.len() }

    // SIGNATURE ONLY. Wave 2 implements the body: match floor, ambiguity
    // detection, corrupted-VIN rejection, and SelectedProfile minting via
    // SelectedProfile::seal (callable here because registry.rs is inside
    // crate::profiles). Partial matches return Selection::Partial (visible,
    // no token); only Exact returns Selection::Matched. Never reached in Wave 1.
    #[allow(clippy::unimplemented)]
    pub fn select(&self, _ctx: &VehicleContext) -> Selection {
        unimplemented!("Wave 2: resolver-owned profile selection")
    }
}

impl Default for ProfileRegistry {
    fn default() -> Self { Self::new() }
}
```

Note on object safety (verify, do not assume): `DiagnosticProfile` is used as `&'static dyn DiagnosticProfile` in the registry and returned as `&dyn DiagnosticProfile` from `get`, so it MUST stay object-safe. The signatures above are object-safe (no generics, no `Self` returns, no associated consts). Adding a generic or `-> Self` method in a later wave silently breaks the registry; pin this with a `fn _assert_object_safe(_: &dyn DiagnosticProfile) {}` in the test module.

Note on `select` and the warnings gate: the `#[allow(clippy::unimplemented)]` attribute is harmless under plain `rustc` (tool-lint allows are tolerated) and pre-empts the `clippy::unimplemented` restriction lint if a strict CI enables it. Do NOT "soften" the body to `Selection::None` -- a silent `None` would masquerade as a real (always-no-match) resolver and is a worse failure than a panic that is provably never called in Wave 1.

### Tests

Unit (inline `#[cfg(test)]` in `model.rs`):
- `id_is_copy_eq_hash` -- put two `ProfileId::new("gm.gmt800.lly.class2")` in a `HashSet`, assert dedup to len 1 and equality; assert `as_str` round-trips. Pins the Copy/Eq/Hash derive contract the registry needs.
- `manufacturer_is_exhaustive_and_has_generic` -- exhaustive `match` over all four variants (compile-enforces no silent variant drop) including `Generic`.
- `match_confidence_is_exhaustive` -- exhaustive `match` over `ProtocolPlusVinDimension`/`VinExact`/`VinPlusSpec`. Pins the FINAL three-variant set so a later wave cannot reintroduce `VinDerived`/`SpecConfirmed`/`VinAndSpec` or reference a nonexistent `::High` without breaking this test.
- `identity_confidence_is_trusted` -- assert `IdentityConfidence::Confirmed.is_trusted()` is true and `Unread`/`Corrupted`/`Single` are false; exhaustive `match` over the four variants. Pins both the FINAL variant set and the `is_trusted()` accessor Wave 9 consumes.
- `evidence_policy_is_exhaustive_with_bounded_live` -- exhaustive `match` including `BoundedLive`. Compile-enforces that the Phase-6 variant Wave 7/9 need exists and is a unit variant (usable as a value).
- `selected_profile_round_trips_via_seal` -- `SelectedProfile::seal(id, 7, false)`, assert `profile_id()==id`, `context_generation()==7`, `manual_confirmed()==false`. This is the ONLY in-crate path that can mint a token; it exercises the sealed constructor.
- `selected_profile_manual_confirm_is_carried` -- `SelectedProfile::seal(id, 7, true)`, assert `manual_confirmed()` is true. Pins that the weak-identity manual override lives on the token (not on `IdentityConfidence`).
- `selected_profile_generation_distinguishes_tokens` -- two tokens, same id, generations 7 vs 8 (both `manual_confirmed=false`), assert `!=`. Pins the generation field that later-wave stale-token validation relies on.
- `profile_match_partial_carries_reason` -- build `ProfileMatch::Partial { reason: "vin 8th digit mismatch".into() }`, exhaustive match over `Exact/Partial/NoMatch`.
- `address_template_variants_are_exhaustive` -- construct `J1850{node:0x18}`, `Can11{..}`, `Can29{..}`, exhaustive match. Asserts there are exactly three arms and (by the absence of a `J1939` arm) that J1939 is not declarable. Also asserts `AddressTemplate` has NO `to_physical_address`/header method in Wave 1 (header synthesis is Layer 2, later) -- enforced by it simply not existing; document in the test comment.
- `route_definition_uses_module_key_not_label` -- build a TCM route `RouteDefinition { module: ModuleKey::Tcm }`; assert `route.module.canonical() == "tcm"`. Guards the `0x1940` TCM-vs-ECM bug class at the type level (Layer 3 can express TCM; the bug fix itself lands in the GM-data wave) and exercises the closed-enum `ModuleKey` + `canonical()`.
- `route_set_single_explicit_and_discovered` -- `RouteSet::single(route)` produces `RouteScope::Single(route)`; `RouteSet::explicit(&[route])` produces `RouteScope::Explicit`; `RouteSet::discovered_on_bus(BusKey::new("j1850vpw"))` produces `RouteScope::DiscoveredOnBus`. Pins the FINAL `RouteSet { scope }` shape and the constructors Waves 5/9 call.
- `source_fields_preserve_strings` -- `SourceFields { txd: "TXD123", rxd: Some(RxdSource { raw: "3008", bit_width: Some(16) }), raw_mth: Some("..."), ..Default::default() }`; assert `txd`, `rxd.unwrap().raw`, `rxd.unwrap().bit_width`, and `raw_mth` survive verbatim. Directly pins plan lines 222-226 (do not lose the fuel-rail range caveat) and the renamed/non-optional FINAL shape.
- `signal_definition_is_pure_static_data` -- build a `const SIGNAL: SignalDefinition` with `request_data: &[0x15, 0x42, 0x01]` and a fully-spelled `SourceFields { .. }` (no `Default::default()` -- it is not `const`); assert no header bytes are present in the struct (it has no header field). Encodes the Layer-3 rule "profiles own service+payload, never framing."
- `decoded_signal_preserves_raw_and_carries_module` -- build `DecodedSignal { raw: vec![0x62,0x15,0x42,0x0A], selected_raw: vec![0x0A], module: ModuleId::new("ecm"), confidence: Confidence::Verified, .. }`; assert `raw` is non-empty and unchanged and that `selected_raw`/`module`/`confidence` are all present. Pins invariant 8 (raw never dropped) AND that the Wave-4 metadata fields coexist on the same struct, so Wave 4 cannot redefine a `raw`-less variant.
- `profile_decode_error_is_exhaustive` -- construct each of `PayloadTooShort { expected: 4, got: 2 }`, `MismatchedResponse`, `UnknownDecoder("x")`, `NegativeResponse { service: 0x19, nrc: 0x11 }`, `Decode("bad".into())`, `Other("y".into())`; exhaustive match. Pins the FINAL variant set (struct-form `PayloadTooShort`, `&'static str` `UnknownDecoder`, the `NegativeResponse` arm Wave 5's `$19` leading-`0x7F` guard returns, and the `Decode` arm Wave 5 needs).
- `_assert_object_safe` -- referenced above; a function taking `&dyn DiagnosticProfile`, called from a test, so loss of object safety breaks the build.

Unit (inline in `registry.rs`):
- `empty_registry_has_no_profiles` -- `ProfileRegistry::new().is_empty()` is true, `len()==0`, `profiles().is_empty()`. Pins that Wave 1 registers nothing in non-test code (no behavior).
- `register_increments_len_and_get_finds_by_id` -- define a minimal `#[cfg(test)] struct TestProfile` implementing `DiagnosticProfile` (id returns `ProfileId::new("test.fixture")`, manufacturer `Generic`, `matches` returns `NoMatch`, all slice getters return `&[]`, both decode methods return `Err(ProfileDecodeError::Decode("test".into()))`). Then `static TEST_PROFILE: TestProfile = TestProfile; let mut r = ProfileRegistry::new(); r.register(&TEST_PROFILE);` assert `len()==1` and `r.get(ProfileId::new("test.fixture")).is_some()`. (`TestProfile` is a zero-field unit struct so it can be a `static`.) This is test-only (not shipped) and exercises the Wave-9 `register(&'static ..)` path and the Wave-3 `get` path; it also doubles as a runtime object-safety witness for `&'static dyn DiagnosticProfile`.
- `get_returns_none_for_unknown_id` -- on the same registry, `r.get(ProfileId::new("nope")).is_none()`.
- NOTE: `select` is intentionally NOT exercised in Wave 1 (its body is `unimplemented!()`); Wave 2 adds its tests when it fills the body.

Integration-with-mock: NONE in Wave 1. There is no runtime path and no mock adapter interaction (no request is ever built). Adding a mock-adapter test here would imply wiring, which is out of scope. Explicitly note: the addressed-J1850 mock limitation flagged in the core inventory (MockAdapter::routed_request downgrades Addressed->Broadcast, mock.rs:300-313) belongs to the wave that first executes a routed profile request, NOT here. Per the cross-wave decision, integration waves standardize on the `Elm327Adapter + MockTransport::expect` harness (Wave 9's pattern) rather than an obd2-core MockAdapter change, so no "fix the mock first" obligation is created by Wave 1.

Golden-corpus: NONE added in Wave 1, and NONE may change. There is no `tests/corpus/` yet (confirmed absent) and Wave 1 introduces no decoder, so the only corpus obligation is negative: the existing LLY behavior tests must stay green byte-for-byte. The frozen baseline this wave must not perturb is the 12 inline `#[test]`s in `gm_enhanced.rs` (lines 841-968), including the ones pinning `LLY_ENHANCED_DIDS.len()==24` (:893) and `LLY_REJECTED_DIDS` order/contents (:904), plus `gm_class2.rs` and `session_runner.rs` tests (e.g. `test_should_force_barometric_standard_poll` :1086, `test_should_force_dashboard_standard_polls` :1091). Wave 1 does NOT touch the still-global poll policy (`should_force_standard_poll` at session_runner.rs:131/824; cadence `cycle % 5/10/20/60` at :207/216/217/232; candidate-DID suppression 0x1542; fuel-rail `preferred_over` at tui/ui.rs:2403-2404 and main.rs:322-343); the `StandardPidOverride`/`preferred_over`/`FailurePolicy` types here are inert vocabulary, and the actual Phase-4 migration is Wave 3.5. Wave 1 must NOT migrate the live policy, or it will regress LLY fuel-rail/baro and cadence on a green build.

Architectural (`tests/wave1_architecture.rs`, std-fs string scan, no new dep):
- `live_modules_do_not_reference_profiles` -- read every `.rs` under `src/` except `src/profiles/` and `src/lib.rs`, assert none contains `crate::profiles` or `profiles::` or `use crate::profiles`. Proves Wave 1 is unwired (the structural form of "no behavior change"), and that `select`'s `unimplemented!()` is never reached from live code.
- `profiles_module_does_not_import_gm` -- read `src/profiles/*.rs`, assert none contains `gm_enhanced`, `gm_class2`, `gm_active`, or `gm_evidence`. Keeps Layer 3's neutral model free of the GM data it will later replace; prevents an accidental dependency edge that would couple the model to LLY.
- `lib_declares_profiles` -- read `src/lib.rs`, assert it contains `pub mod profiles;`. Trivial guard that the module is actually compiled.
- Optional (flagged against the conservative dependency policy): a `trybuild` compile-fail case proving external code cannot call `SelectedProfile::seal` or fabricate the `_sealed` field. `trybuild` is a new dev-dependency; given the minimal-deps policy, DEFER this and rely on `pub(in crate::profiles)` visibility + the documented invariant. The real runtime enforcement (live code cannot reach manufacturer routed APIs) is the Phase-3 architectural import test in a later wave, not Wave 1.

### Acceptance criteria

- [ ] `cargo build -p obd2-dash` compiles with the new `profiles` module.
- [ ] `cargo build -p obd2-dash` produces NO `dead_code` warnings, and a `-D warnings`/clippy build passes. (The types are unused by design; `#![allow(dead_code)]` on `profiles/mod.rs` is required, with a `// TODO(wave-2+): remove once the registry/dispatcher consume these` note. The `#[allow(clippy::unimplemented)]` on `select` keeps the warnings gate green even if the restriction lint is enabled.)
- [ ] `cargo test -p obd2-dash` is green, and the existing LLY tests show ZERO diffs in count or assertions (the 24-DID pin, rejected-DID pin, forced-standard-PID pins, and all `gm_class2`/`session_runner` tests still pass unchanged). No existing test file is edited.
- [ ] Existing LLY golden corpus stays green with zero diffs. (No profile corpus exists yet; the obligation is that nothing this wave touches alters any existing decoded output. Because no live module imports `profiles`, this is structurally guaranteed and additionally asserted by `live_modules_do_not_reference_profiles`.)
- [ ] `cargo test -p obd2-core` is green (untouched; this wave makes no obd2-core change).
- [ ] `SelectedProfile` cannot be constructed outside `crate::profiles`: the `_sealed: ()` field is private and `seal` is `pub(in crate::profiles)`. Wave 2 MUST NOT widen `seal` to `pub(crate)`/`pub`. Verified by visibility + documented; no external mint path exists.
- [ ] Every shared type matches the "Type ownership contract": `DecodedSignal` carries `raw` (invariant 8) plus `selected_raw`/`module`/`confidence`; `SourceFields` has non-optional `txd`, `rxd: Option<RxdSource>`, and `raw_mth`; `MatchConfidence` is exactly `{ProtocolPlusVinDimension, VinExact, VinPlusSpec}`; `IdentityConfidence` is exactly `{Unread, Corrupted, Single, Confirmed}` with `is_trusted()`; `EvidencePolicy` includes `BoundedLive`; `RouteSet` is `{ scope: RouteScope }` with `single`/`explicit`/`discovered_on_bus`; `Provenance` includes `LocalFixture`; `ProfileDecodeError` includes struct-form `PayloadTooShort`, `UnknownDecoder(&'static str)`, `NegativeResponse { service, nrc }`, and `Decode(String)`; `ModuleKey` is the closed enum `{Ecm,Tcm,Ficm,Bcm,Ebcm,Ipc,Sdm,Hvac}` with `canonical()`; `RouteDefinition` is module-only.
- [ ] `ProfileRegistry` stores `Vec<&'static dyn DiagnosticProfile>` (NO `Box`) and exposes `new`/`register`/`get`/`profiles`/`is_empty`/`len`/`select`; `register(&'static dyn DiagnosticProfile)` (Wave 9 passes `&FIXTURE_PROFILE`) and `get(ProfileId) -> Option<&dyn DiagnosticProfile>` (Wave 3) compile; `select` body is `unimplemented!()` (Wave 2 fills it).
- [ ] `BusKey` exposes `const fn new`/`const fn as_str`; `ModuleKey` is the closed enum with `const fn canonical()` (Waves 3/5/9). No stringly `ModuleKey::new`/`as_str`.
- [ ] `AddressTemplate` has exactly three arms (`J1850`/`Can11`/`Can29`), is NOT `#[non_exhaustive]`, and exposes NO `PhysicalAddress` conversion or header-synthesis method (that is Layer 2, later wave). The `6C <node> F1` convention appears NOWHERE in Wave 1.
- [ ] `VehicleContext` uses `obd2_core::vehicle::ModuleId` (not the plan's non-existent `obd2_core::domain::ModuleId`) and compiles. `Option<VehicleSpec>` by value is correct (`VehicleSpec: Clone` verified); no `Arc` needed.
- [ ] `DiagnosticProfile` is object-safe (`Box<dyn DiagnosticProfile>` and `&dyn DiagnosticProfile` compile; `_assert_object_safe` test passes).
- [ ] `ProfileRegistry::new()` is empty; nothing in non-test code registers a profile in this wave (the `register`/`get` tests use a `#[cfg(test)]`-only stub profile that is not compiled into the binary).
- [ ] The three `gm_*` modules and all of `session_runner.rs`/`app.rs`/`main.rs`/`tui`/`widget`/`recording` are byte-identical to pre-wave except the single added `pub mod profiles;` line in `lib.rs`.

### Rollback notes

- This wave is purely additive and unwired, so it is independently shippable and trivially reversible: it can merge to `master` with no effect on the running TUI because no code path reaches it. The compiled binary's behavior is identical.
- Revert procedure: delete the `src/profiles/` directory and `tests/wave1_architecture.rs`, then remove the one `pub mod profiles;` line from `lib.rs`. No other file was modified, so there is nothing else to undo and no data migration to reverse.
- Flag option: if a stricter "no dead code lands on master" policy is preferred over `#![allow(dead_code)]`, gate the module behind a cargo feature: `#[cfg(feature = "profiles")] pub mod profiles;` in `lib.rs` and a `profiles = []` feature in `Cargo.toml`. This keeps the types out of default builds entirely until Wave 2 wires them. Trade-off: the architectural and unit tests must then run under `--features profiles`, and CI must add that invocation. Given the wave is dead-but-harmless, the simpler `#![allow(dead_code)]` is recommended and the feature flag is the fallback if the warnings policy forbids it.
- Hard problems intentionally NOT solved in this wave (so a reviewer does not mistake the wave for more than it is): (1) the `ModuleKey -> ModuleId` and `ModuleMap + RouteDefinition -> PhysicalAddress` resolution (including the `[0x6C, node, 0xF1]` J1850 header synthesis) is deferred to the Layer-2 resolver wave; (2) the match floor, ambiguity detection, corrupted-VIN rejection, and `SelectedProfile` minting (the `select` body) are deferred to the session-owned-selection wave (Wave 2) -- including the invariant-5 contract that a `Selection::Partial` is visible but yields no token and is refused at `execute_request`; (3) the `Confidence`/`Provenance`/`PollCadence`/`FailurePolicy` name collisions with `gm_enhanced` and `obd2_core::protocol::enhanced` are documented but the GM->profile value mapping is deferred to the GM-data migration wave; (4) the Phase-4 poll policy (forced standard PIDs, cadence, candidate-DID suppression, `preferred_over`) is still global/UI-local in source and is owned by Wave 3.5 -- Wave 1 ships only the inert `StandardPidOverride`/`FailurePolicy`/`preferred_over` vocabulary and must NOT migrate the live policy, or it will regress LLY fuel-rail/baro and cadence on a green build; (5) `VehicleContext` owning a cloned `VehicleSpec` versus `Arc<VehicleSpec>` is left as a by-value `Option<VehicleSpec>` (Clone confirmed) since no context is built at runtime in this wave; revisit if Phase 2 shows per-generation clone cost.

Both locations confirmed: `Message` at app.rs:78, `DomainMessage` at domain.rs:140. Producing the corrected section.

---

## Wave 2: Profile Selection

### Objective

Make the dash session lifecycle the single owner of profile selection: build one immutable `VehicleContext` per identity generation (after VIN identification, VIN-confidence/retry, and discovery), run every registered profile through a framework-enforced match floor that can only seal an unambiguous, generation-bound `SelectedProfile` for an exact match, and implement the `gm.gmt800.lly.class2` matcher as the first registered profile. This wave builds the selection machinery in parallel with the existing live gates and proves equivalence; it does NOT yet rewire polling through the dispatcher (that is Wave 3).

### Depends on

- **Wave 1 (Neutral Profile Model, plan Phase 1) MUST land first.** Wave 2 consumes the model types Wave 1 introduced under `crates/obd2-dash/src/profiles/`: `ProfileId`, `Manufacturer`, `VehicleContext`, `ProfileMatch`, `MatchConfidence`, `IdentityConfidence`, the sealed `SelectedProfile` (with its `pub(in crate::profiles) fn seal` constructor), the `DiagnosticProfile` trait, and a `ProfileRegistry` skeleton with at least a stub `GmLlyClass2Profile` registered (its `matches` may return `NoMatch` as a placeholder). If Wave 1 does not exist, do not proceed (verified absent today: `crates/obd2-dash/src/lib.rs` exports only `gm_active`, `gm_class2`, `gm_enhanced`, `gm_evidence`).
- **Wave 1 owns the final shape of `MatchConfidence` and `IdentityConfidence`; Wave 2 MUST NOT redefine them.** Per the cross-wave PROFILE MODEL TYPE INSTABILITY finding, the model types are pinned in Wave 1 and every later wave conforms additively. If Wave 1 shipped `MatchConfidence`, `IdentityConfidence`, or the `SelectedProfile` constructor as TODO placeholders, the fix is to amend Wave 1 in the same commit (and update Wave 1's inline tests), NOT to re-`enum` them inside Wave 2. The variant sets and the `seal` signature shown in this spec are the agreed final shapes; if they differ from what Wave 1 shipped, reconcile them in Wave 1 first. The only additive change Wave 2 makes to a Wave 1 type is the new `manual_confirmed: bool` field on `SelectedProfile` (see "Wave 1 amendment" below).
- **obd2-core is unchanged by this wave.** Wave 2 only reads existing core accessors (`Session::read_vin`, `Session::identify_vehicle`, `Session::vehicle`, `Session::spec`, `Session::discovery`, `Session::adapter_info`). No `Adapter`/`Session`/`VehicleSpec` signature changes. This keeps the obd2-core protocol golden corpus untouched.
- **No dependency on Wave 3+ (dispatcher).** Wave 2 must be shippable while `find_lly_did`, `session_matches_lly_profile`, `append_dash_gm_targets`, and `should_scan_gm_class2` remain the live decision points. The new `ProfileState` is computed and surfaced but is not yet the polling gate.

### Wave 1 amendment (same commit)

Adding `manual_confirmed: bool` to `SelectedProfile` changes the `seal` constructor signature that Wave 1 shipped and exercised. This wave MUST, in the same commit:

- Extend `SelectedProfile` with `manual_confirmed: bool` and thread it through `pub(in crate::profiles) fn seal(...)`. The constructor visibility stays exactly `pub(in crate::profiles)` (see the critical note in "Hard problems" below). Do NOT rename `seal` to `mint`, and do NOT widen it to `pub(crate)` or `pub`.
- Update Wave 1's shipped inline test `selected_profile_round_trips_via_seal` to pass the new argument and assert `manual_confirmed()`.
- Record the signature change in the Wave 1 changelog/commit message so the rename/extension is documented rather than silent.

### Files touched

- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/selection.rs` - `ProfileState`, `PartialProfileMatch`, `ProfileAmbiguity`, `ProfileStateSnapshot` (UI-safe, token-free), the process-global monotonic generation source, `acquire_identity` (VIN identify + opt-in corroboration + confidence), `validate_vin_charset`, `build_vehicle_context`, and `select_into_state`. (`IdentityConfidence` is NOT defined here; it is a Wave 1 model type consumed by this module.)
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/registry.rs` - implement `ProfileRegistry::select` with the match floor + ambiguity detection (the ONLY seal path; the only place `SelectedProfile::seal` is called) and `ProfileRegistry::confirm_manual`. `registry.rs` is inside `crate::profiles`, so `pub(in crate::profiles) fn seal` is already callable here with no visibility widening.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/model.rs` - add `manual_confirmed: bool` to `SelectedProfile`; thread it through the existing `pub(in crate::profiles) fn seal(...)` (keep this visibility); add `pub fn manual_confirmed(&self) -> bool` and `pub fn is_valid_for(&self, generation: u64) -> bool`; keep `_sealed: ()` so external construction stays impossible. Do NOT redefine `MatchConfidence` or `IdentityConfidence` here - they are Wave 1 types.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/lly.rs` (or `profiles/gm/mod.rs` if Wave 1 placed it there) - implement `DiagnosticProfile::matches` for `GmLlyClass2Profile` by DELEGATING to `gm_enhanced::lly_profile_matches` / `gm_enhanced::is_lly_spec_identity` (regression-safe), plus finer `Partial` logic; declare allowed protocols (`J1850Vpw` only).
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/mod.rs` - `pub mod selection;` and re-exports.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/session_runner.rs` - add `profile_state: ProfileState` and an identity-generation field to `PreparedSession` (struct at `:41`); after `identify_vehicle` in `prepare_session` (`:103-123`) call `acquire_identity` + `build_vehicle_context` + `select_into_state`; in `run_prepared_session` (`:150`) detect protocol/VIN/spec change each cycle and bump generation + re-select; emit the new UI-facing `app.rs::Message`s. Leave `session_matches_lly_profile`, `append_dash_gm_targets`, `build_enhanced_targets`, `should_scan_gm_class2` UNCHANGED.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/app.rs` - add `Message::ProfileStateUpdate(ProfileStateSnapshot)` and `Message::IdentityConfidenceWarning(String)` to the UI-facing `Message` enum (`:78`), plus their inert `update()` arms (display only, no behavior change). This is the enum the UI consumes; the token-free snapshot belongs here, NOT in `domain.rs`.
- **MODIFY (only if recording needs it)** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/domain.rs` - the enum here is `DomainMessage` (`:140`), consumed by `update()` for recording/domain state. Add a `DomainMessage` variant ONLY if profile-state recording is required by domain state; the UI snapshot does NOT belong here. Any message reference written in `domain.rs` context must be spelled `DomainMessage::`, never `Message::`.
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/profile_selection.rs` - unit + mock integration tests (the `tests/` dir does not exist yet; create it).
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus_selection.rs` - cross-profile selection corpus runner.
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus/selection/gm.gmt800.lly.class2/*.json` - frozen selection cases (context -> expected match). Keep separate from the decode goldens under `tests/corpus/profile/...` planned for a later wave.

### Exact APIs

obd2-core signatures consumed (already exist, do not change):

```rust
// session/mod.rs
pub async fn read_vin(&mut self) -> Result<String, obd2_core::error::Obd2Error>;        // :451
pub async fn identify_vehicle(&mut self) -> Result<obd2_core::vehicle::VehicleProfile, Obd2Error>; // :478
pub fn vehicle(&self) -> Option<&obd2_core::vehicle::VehicleProfile>;                    // :980
pub fn spec(&self) -> Option<&obd2_core::vehicle::VehicleSpec>;                          // :985
pub fn discovery(&self) -> Option<&obd2_core::session::discovery::DiscoveryProfile>;     // :167
pub fn adapter_info(&self) -> &obd2_core::adapter::AdapterInfo;                          // :990
// vehicle/mod.rs
impl VinMatcher { pub fn matches(&self, vin: &str) -> bool; }                            // :129
// DiscoveryProfile { selected_protocol: Protocol, active_bus: Option<ResolvedBus>, modules: HashMap<ModuleId, ResolvedModule> }
```

Wave 1 model types Wave 2 consumes (defined in `profiles/model.rs` by Wave 1 - shown for reference, DO NOT redefine in Wave 2):

```rust
// OWNED BY WAVE 1. Wave 2 references these variant sets; it does not re-enum them.
// If Wave 1 shipped a different variant set, reconcile in Wave 1 (same commit), not here.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchConfidence { VinExact, VinPlusSpec, ProtocolPlusVinDimension }

/// VIN identity confidence. Floor for Exact is `Confirmed`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityConfidence {
    Unread,    // no usable VIN read at all
    Corrupted, // wrong length or illegal VIN charset (I/O/Q or non-ASCII)
    Single,    // exactly one clean read, not yet corroborated
    Confirmed, // >= 2 clean reads agree
}
```

`SelectedProfile` after the Wave 1 amendment (in `profiles/model.rs`):

```rust
// SelectedProfile gains manual_confirmed; constructor stays Wave 1's `seal`,
// at exactly pub(in crate::profiles). DO NOT rename to `mint`. DO NOT widen.
pub struct SelectedProfile {
    profile_id: ProfileId,
    context_generation: u64,
    manual_confirmed: bool,
    _sealed: (),
}

impl SelectedProfile {
    /// ONLY seal path. pub(in crate::profiles) so registry.rs (which lives in
    /// crate::profiles) can call it, while session_runner.rs / app.rs / gm_*.rs
    /// CANNOT fabricate a token directly. This is the resolver-only minting
    /// invariant (plan line 87) and the sealed-token safety hinge (plan OWL
    /// line 946). Wave 1 said "Do NOT widen this to pub" - that holds; pub(crate)
    /// is also too wide and is forbidden.
    pub(in crate::profiles) fn seal(
        profile_id: ProfileId,
        context_generation: u64,
        manual_confirmed: bool,
    ) -> Self;
    pub fn profile_id(&self) -> ProfileId;
    pub fn manual_confirmed(&self) -> bool;
    /// Generation gate the Wave 3 dispatcher will call before every routed request.
    pub fn is_valid_for(&self, generation: u64) -> bool; // == self.context_generation == generation
}
```

New types in `profiles/selection.rs`:

```rust
use std::sync::atomic::{AtomicU64, Ordering};
use obd2_core::vehicle::{Protocol, VehicleSpec, ModuleId};
use obd2_core::adapter::Adapter;
use obd2_core::session::Session;
use crate::profiles::model::{ProfileId, IdentityConfidence, SelectedProfile}; // IdentityConfidence is Wave 1's

#[derive(Clone, Debug)]
pub struct PartialProfileMatch { pub profile_id: ProfileId, pub reason: String }

#[derive(Clone, Debug)]
pub struct ProfileAmbiguity {
    pub profile_ids: Vec<ProfileId>,
    pub evidence: String, // human-readable; logged at WARN with colliding ids + match dims
}

/// Session-lifecycle-owned selection state. Held in PreparedSession (dash side),
/// NOT in obd2_core::Session (which cannot depend on dash profiles).
#[derive(Debug, Default)]
pub struct ProfileState {
    pub generation: u64,
    pub vin_confidence: Option<IdentityConfidence>,
    pub selected: Option<SelectedProfile>,
    pub exact_matches: Vec<ProfileId>,
    pub partial_matches: Vec<PartialProfileMatch>,
    pub ambiguity: Option<ProfileAmbiguity>,
}

/// UI-facing, token-free projection. Carries NO SelectedProfile so the UI cannot
/// execute. This is what travels in app.rs::Message::ProfileStateUpdate.
#[derive(Clone, Debug)]
pub struct ProfileStateSnapshot {
    pub generation: u64,
    pub selected_profile_id: Option<&'static str>,
    pub manual_confirmed: bool,
    pub vin_confidence: Option<IdentityConfidence>,
    pub exact_matches: Vec<&'static str>,
    pub partial_matches: Vec<(&'static str, String)>,
    pub ambiguity: Option<Vec<&'static str>>,
}

/// Process-global monotonic generation. Guarantees no generation value is ever
/// reused across sessions/tasks, so a stale token from a prior connection can
/// never validate against a later one.
static GENERATION: AtomicU64 = AtomicU64::new(1);
pub fn next_generation() -> u64 { GENERATION.fetch_add(1, Ordering::SeqCst) }

/// VIN charset floor per ISO 3779: exactly 17 chars, [A-HJ-NPR-Z0-9] only
/// (excludes I, O, Q). This is the cheap, deterministic corrupted-VIN catch for
/// the documented I/1 confusion; obd2-core VinMatcher::matches does NOT do this.
pub fn validate_vin_charset(vin: &str) -> bool;

pub struct IdentityOutcome { pub vin: Option<String>, pub confidence: IdentityConfidence }

/// VIN identify + OPT-IN corroboration + confidence.
///
/// Calls Session::identify_vehicle once (preserving existing behavior + spec
/// match + raw-capture rename), then performs up to `extra_reads` corroborating
/// Session::read_vin calls.
///
/// CRITICAL traffic invariant: `extra_reads` defaults to 0 in the unflagged
/// build, and the session_runner call site passes 0 unless the
/// `profile-selection` feature is enabled. With extra_reads == 0 this issues
/// EXACTLY today's VIN traffic (one identify_vehicle, no extra $0902 reads), so
/// the wire is byte-for-byte unchanged. Corroboration (and therefore the
/// `Confirmed` state) is opt-in and stays dark until Wave 3 consumes the state.
///
/// Confidence:
///   - Unread    if identify_vehicle and all reads fail
///   - Corrupted if the accepted VIN fails validate_vin_charset
///   - Confirmed if a corroborating read equals the identify VIN (requires extra_reads >= 1)
///   - Single    if only the identify VIN is clean and uncorroborated
///             (this is the ceiling whenever extra_reads == 0)
pub async fn acquire_identity<A: Adapter>(
    session: &mut Session<A>,
    extra_reads: u8, // 0 in the default build; > 0 only under the profile-selection flag / in tests
) -> IdentityOutcome;

/// Build the immutable context for this generation from live session state.
/// protocol <- discovery().selected_protocol else adapter_info().protocol;
/// vin <- identity outcome (NOT a fresh read); spec <- session.spec().cloned();
/// discovered_modules <- discovery().modules keys; active_bus <- discovery().active_bus id.
pub fn build_vehicle_context<A: Adapter>(
    session: &Session<A>,
    generation: u64,
    identity: &IdentityOutcome,
) -> VehicleContext;

/// Run the registry and fold the result into ProfileState. Pure: no I/O.
pub fn select_into_state(registry: &ProfileRegistry, ctx: &VehicleContext) -> ProfileState;
```

`profiles/registry.rs`:

```rust
impl ProfileRegistry {
    /// The ONLY place a SelectedProfile is sealed (the only caller of
    /// SelectedProfile::seal). Enforces the framework match floor, detects
    /// cross-profile ambiguity, and seals only for a single unambiguous Exact
    /// whose VIN confidence is Confirmed.
    pub fn select(&self, ctx: &VehicleContext) -> ProfileState;

    /// Deliberate, visible override. Requires the named profile to return at
    /// least Partial for ctx (never NoMatch). Seals with manual_confirmed=true.
    /// Active tests stay locked under this flag (enforced by the Wave 8 dispatcher).
    pub fn confirm_manual(
        &self,
        ctx: &VehicleContext,
        profile_id: ProfileId,
    ) -> Result<SelectedProfile, ManualConfirmError>;
}

#[derive(Debug, thiserror::Error)]
pub enum ManualConfirmError {
    #[error("profile {0:?} is not registered")] NotRegistered(ProfileId),
    #[error("profile {0:?} returned NoMatch; manual confirm requires at least Partial")]
    NotEvenPartial(ProfileId),
}
```

The framework match floor inside `select` (applied to every profile's `ProfileMatch::Exact` before it is honored; a profile that returns `Exact` while failing any floor item is downgraded to `Partial` with reason and logged):

1. `ctx.protocol != Protocol::Auto` and the profile declares that concrete protocol/family as allowed (`Protocol::Auto` -> never Exact; it maps to `BusFamily::Can` in core and must not satisfy a J1850 profile).
2. at least one VIN-derived identity dimension present (model year, engine 8th digit, platform, or WMI) - not merely protocol.
3. when `ctx.spec.is_some()`, spec identity is consistent with the profile.
4. `ctx.vin_confidence == Some(IdentityConfidence::Confirmed)` (Single/Corrupted/Unread/None -> at most Partial). Note: in the default (unflagged) build, `acquire_identity` is called with `extra_reads == 0`, so confidence never exceeds `Single` and no profile reaches Exact through the live path. This is intentional - Wave 2 does not drive live behavior - and the Confirmed-path tests below construct contexts directly or pass `extra_reads >= 1`.
5. no discovered-module evidence contradicting the profile.

`profiles/gm/lly.rs`:

```rust
impl DiagnosticProfile for GmLlyClass2Profile {
    fn id(&self) -> ProfileId; // ProfileId("gm.gmt800.lly.class2")
    fn manufacturer(&self) -> Manufacturer { Manufacturer::Gm }

    fn matches(&self, ctx: &VehicleContext) -> ProfileMatch {
        // Protocol floor: only J1850Vpw.
        // Exact path DELEGATES to the existing gate to guarantee identical decisions:
        //   gm_enhanced::lly_profile_matches(vin, spec.as_ref(), ctx.protocol) == true
        //   AND ctx.vin_confidence == Confirmed
        // Partial when SOME identity dimension matches but Exact fails
        //   (e.g. J1850 + LLY spec identity present but VIN weak/charset-invalid,
        //    or WMI/platform hints without confirmed VIN-8).
        // NoMatch otherwise (wrong protocol, no VIN-derived dimension, contradicting spec).
    }
    // signals/dtc_services/active_tests/passive_monitors: return Wave 1 placeholders
    // (real data migrates in Wave 5). decode_* unused in this wave.
}
```

`session_runner.rs` wiring (additive; existing gates untouched):

```rust
pub struct PreparedSession {
    poll_ms: u64,
    poll_config: PollConfig,
    enhanced_targets: Vec<EnhancedPollTarget>,
    gm_class2_backoff: GmClass2Backoff,
    last_connection: Option<ConnectionState>,
    last_discovery: Option<DiscoveryState>,
    profile_state: ProfileState,      // NEW
    identity: IdentityOutcome,        // NEW: cached so context rebuilds do not re-read VIN gratuitously
}

// At the prepare_session call site, the corroboration count is gated so the
// default build's VIN traffic is unchanged:
//   #[cfg(feature = "profile-selection")] const EXTRA_VIN_READS: u8 = 2;
//   #[cfg(not(feature = "profile-selection"))] const EXTRA_VIN_READS: u8 = 0;
//   let identity = acquire_identity(session, EXTRA_VIN_READS).await;
```

`app.rs` (UI-facing `Message` enum, `:78`):

```rust
// Added to the existing `pub enum Message { ... }` at app.rs:78, with inert
// (display-only) update() arms. Token-free: ProfileStateSnapshot carries no
// SelectedProfile, so the UI can never execute a routed request from it.
ProfileStateUpdate(crate::profiles::selection::ProfileStateSnapshot),
IdentityConfidenceWarning(String),
```

`domain.rs` (`DomainMessage` enum, `:140`) - touch ONLY if recording needs it:

```rust
// The enum in domain.rs is DomainMessage, consumed by update() for
// recording/domain state. Do NOT add the UI snapshot here. If (and only if)
// recording must persist profile-state transitions, add a DomainMessage
// variant - spelled DomainMessage::, never Message:: - and nothing more.
// As of Wave 2 there is no recording requirement, so domain.rs is untouched.
```

### Tests

Unit (in `tests/profile_selection.rs` and/or `#[cfg(test)]` in `selection.rs`/`registry.rs`):

- `vin_charset_rejects_illegal_letters` - `validate_vin_charset` returns false for any VIN containing `I`, `O`, or `Q`, for length != 17, and for non-ASCII; true for `1GTHK29294E391526`.
- `identity_confidence_confirmed_on_agreement` - mock yields the same VIN twice, called with `extra_reads >= 1` -> `Confirmed`.
- `identity_confidence_single_when_uncorroborated` - one clean read, second read errors (or `extra_reads == 0`) -> `Single`.
- `identity_confidence_corrupted_on_bad_charset` - read yields a 17-char VIN with `I` -> `Corrupted`.
- `identity_confidence_unread_when_all_fail` - all reads error -> `Unread`, `vin == None`.
- `default_build_issues_no_extra_vin_reads` - with `extra_reads == 0`, `acquire_identity` calls `read_vin` zero times beyond the single `identify_vehicle` (assert via a counting mock); confidence ceiling is `Single`. Pins the byte-for-byte VIN-traffic invariant.
- `selected_profile_is_generation_bound` - `seal(id, 7, false).is_valid_for(7)` true; `.is_valid_for(8)` false. (Lives inside `crate::profiles`, since `seal` is `pub(in crate::profiles)`.)
- `selected_profile_cannot_be_constructed_externally` - compile-fence note: `_sealed: ()` plus `pub(in crate::profiles) fn seal` make external construction impossible at compile time. A `tests/`-level (external crate) test confirms there is no public constructor (`SelectedProfile::seal` is not nameable, no `pub fn new`).
- `seal_symbol_not_named_outside_profiles` - architectural guard required by the OWL critical finding: a source-scanning test that greps `crates/obd2-dash/src/`, excluding `src/profiles/`, and asserts the symbol `SelectedProfile::seal` (and the bare identifier `seal(` on a SelectedProfile path) appears in ZERO files. This catches a future careless widening to `pub(crate)`/`pub` even if it still compiles, defeating any attempt by session_runner/app/gm_* to fabricate a token and bypass the registry match-floor.
- `next_generation_is_strictly_monotonic` - two calls produce distinct increasing values.
- `manual_confirm_requires_partial` - `confirm_manual` for a profile returning `NoMatch` -> `Err(NotEvenPartial)`; for `Partial` -> `Ok` with `manual_confirmed() == true`.
- `floor_rejects_protocol_auto` - context with `Protocol::Auto` + otherwise-valid LLY identity -> no Exact (Partial at best).
- `floor_requires_confirmed_vin_for_exact` - same LLY context with `vin_confidence = Single` -> Partial, not Exact, and `selected.is_none()`.

Integration-with-mock (`tests/profile_selection.rs`, using `obd2_core::adapter::MockAdapter` + a spec registry loaded with the LLY spec; note MockAdapter overrides `routed_request` but downgrades addressed to broadcast and returns `NoData` for `0x03/0x07/0x0A` - acceptable here because selection issues no routed requests; tests that need `Confirmed` pass `extra_reads >= 1`):

- `lly_exact_match_from_mock_identity` - mock returns VIN `1GCHK23224F000001` (mock_profile "duramax", VIN-8 `2`) under `J1850Vpw` with the LLY spec loaded, `extra_reads >= 1` so corroboration yields `Confirmed`; `select` -> exactly one Exact, `selected.is_some()`, generation set.
- `missing_spec_falls_back_to_generic` - VIN present, `spec == None` -> no Exact, `selected.is_none()`, `partial_matches` may list LLY with reason "no decoded spec".
- `missing_vin_falls_back_to_generic` - identify fails -> `Unread`, `selected.is_none()`.
- `wrong_protocol_rejects` - LLY VIN+spec but `Protocol::Can11Bit500` -> NoMatch/Partial, never Exact.
- `wrong_vin_eighth_digit_rejects` - VIN with 8th char != `2` -> not Exact.
- `wrong_engine_spec_rejects` - spec with engine code != LLY -> not Exact (delegation to `is_lly_spec_identity`).
- `generation_reselect_on_protocol_change` - simulate `discovery().selected_protocol` flipping from `J1850Vpw` to `Can11Bit500`; assert generation increments and prior `SelectedProfile.is_valid_for(old_gen)` is now stale relative to the new generation.

Equivalence (the zero-diff guard, `tests/profile_selection.rs`):

- `select_exact_iff_legacy_gate_true_for_confirmed_vin` - for every selection corpus case, with `vin_confidence = Confirmed` (set directly on the constructed context, not via the live `extra_reads == 0` path), assert `select(ctx).selected.is_some()` iff `gm_enhanced::lly_profile_matches(vin, spec.as_ref(), protocol) == true`. This pins that Wave 2 selection mirrors the existing live gate exactly for clean VINs, so no real LLY truck changes state.

Golden / selection corpus (`tests/corpus_selection.rs` over `tests/corpus/selection/**/*.json`):

- `corpus_cases_match_expected` - each frozen case `{protocol, vin, vin_confidence, spec_id|inline_identity, expected}` is replayed through `select` and the resulting match (Exact/Partial/NoMatch + selected/none) equals `expected`, byte/enum-for-enum. The PRIMARY positive case uses the real captured truck identity; per the corpus-seed OWL caveat, freeze its `expected` only after confirming the loaded LLY spec's `vin_match.wmi_prefixes` actually admits the captured WMI - if it does not, the frozen expected is `Partial`, not `Exact`, and that is recorded in the case file with a note.
- `corrupted_vin_never_exact` - a synthesized corrupted-VIN case (17 chars, `2` at index 8, but containing `I`) -> `vin_confidence = Corrupted` -> expected `Partial`/`NoMatch`, never Exact. (Cannot be seeded from the `1IGTHKI...` capture: its `0902` bytes decode cleanly per the corpus inventory; this case is synthetic.)

Architectural / cross-profile (`tests/corpus_selection.rs`):

- `no_two_profiles_exact_for_same_context` - register a test-only non-GM fixture profile (under `#[cfg(test)]`) alongside the real registry; for every corpus context assert at most one profile yields Exact (post-floor). With only LLY registered today this is trivially true, so the fixture is required to actually exercise the ambiguity path before more profiles land.
- `ambiguity_blocks_selection_and_logs` - construct a context two fixture profiles both claim Exact; assert `select` returns `selected.is_none()`, `ambiguity.is_some()`, and the ambiguity evidence lists both colliding `ProfileId`s.
- `no_exact_without_protocol_plus_vin_dimension` - property test across all registered profiles: a context with protocol but zero VIN-derived dimension never yields Exact.
- `fixture_profile_cannot_decode_through_gm` - the non-GM fixture's `matches` returning Exact does not give it access to GM signal decode (decoder isolation is enforced by `decoder_id` ownership in Wave 3, but this test pins that selecting the fixture does not select or expose the GM profile's capabilities).

### Acceptance criteria

- [ ] `profiles/selection.rs` compiles with `ProfileState`, `ProfileStateSnapshot`, `acquire_identity`, `validate_vin_charset`, `build_vehicle_context`, `select_into_state`, and the global generation source. It consumes Wave 1's `IdentityConfidence` and `MatchConfidence` without redefining them.
- [ ] `SelectedProfile` is sealable only inside `crate::profiles` (`_sealed: ()` + `pub(in crate::profiles) fn seal`); the constructor was NOT renamed to `mint` and NOT widened to `pub(crate)`/`pub`; `registry.rs` (in `crate::profiles`) still calls it. `is_valid_for` rejects a mismatched generation.
- [ ] `seal_symbol_not_named_outside_profiles` passes: no module outside `crate::profiles` names the `seal` constructor.
- [ ] Wave 1's `selected_profile_round_trips_via_seal` test was updated in the same commit for the new `manual_confirmed` argument, and the signature change is documented.
- [ ] `ProfileRegistry::select` is the only seal path and enforces all five floor items; a profile returning `Exact` while failing any floor item is downgraded to `Partial` and logged.
- [ ] Ambiguity (>=2 post-floor Exact) yields `selected = None`, `ambiguity = Some`, and a WARN log naming the colliding ids and match dimensions.
- [ ] `gm.gmt800.lly.class2` matcher delegates its Exact decision to `gm_enhanced::lly_profile_matches`; the equivalence test `select_exact_iff_legacy_gate_true_for_confirmed_vin` passes.
- [ ] Default-build VIN traffic is unchanged: `acquire_identity` is called with `extra_reads == 0` in the unflagged build, issues no extra `$0902` reads beyond the single `identify_vehicle`, and `default_build_issues_no_extra_vin_reads` passes. Corroboration (and `Confirmed`) is only reachable under the `profile-selection` feature flag or with an explicit `extra_reads >= 1` in tests. An identity-confidence warning (`Message::IdentityConfidenceWarning`) is emitted when confidence is below `Confirmed`.
- [ ] Generation invalidation is wired for: protocol change, VIN change, and decoded-spec change inside `run_prepared_session`; each new `prepare_session` (new connection -> new `Session`) gets a fresh generation from the global source, covering disconnect/reconnect/adapter change.
- [ ] Manual confirm requires at least `Partial`, seals with `manual_confirmed = true`, and is surfaced in `ProfileStateSnapshot`.
- [ ] The UI snapshot travels in `app.rs::Message::ProfileStateUpdate` (enum at `:78`) with inert display-only `update()` arms; it carries NO `SelectedProfile` token, so the UI cannot execute requests with it. `domain.rs`'s `DomainMessage` (`:140`) is untouched (no recording requirement in this wave).
- [ ] **Existing live gates are byte-for-byte unchanged in Wave 2:** `session_matches_lly_profile` (`session_runner.rs:900`), `append_dash_gm_targets` (`:873`), `build_enhanced_targets` (`:846`), and `should_scan_gm_class2` (`:622`) are not edited. Enhanced-target building and `$19` scanning still gate on the old boolean.
- [ ] **Existing LLY golden corpus stays green with zero diffs.** No decode path, no DID, no `$19` path, no `EnhancedPollTarget`, and no UI label changes in this wave. The existing pinned tests in `gm_enhanced.rs` (`len()==24`, rejected-DID order) and `session_runner.rs` (`test_should_force_*`) are untouched.
- [ ] `cargo test -p obd2-core` unchanged and green (no core edits).
- [ ] `cargo test -p obd2-dash` green, including the new `tests/profile_selection.rs`, `tests/corpus_selection.rs`, and the frozen `tests/corpus/selection/**` cases.
- [ ] The primary positive selection corpus case's `expected` was verified against the actually loaded LLY spec `vin_match` before freezing (Exact only if the spec admits the captured WMI; otherwise Partial, with a note in the case file).

### Rollback notes

- **Independently shippable because it is parallel, not load-bearing.** Wave 2 adds selection machinery but does not move the live polling/scan gates; live LLY behavior is driven by the unchanged `session_matches_lly_profile` gates. If `ProfileState` is wrong, nothing on the wire or screen changes except the new (optional) status messages.
- **Feature-flag option:** gate the `session_runner.rs` wiring (the `acquire_identity` + `build_vehicle_context` + `select_into_state` calls, the `extra_reads` corroboration count, and the two new `app.rs::Message` emits) behind `#[cfg(feature = "profile-selection")]`, default-off. With the feature off, `EXTRA_VIN_READS == 0` (same VIN traffic as today), `PreparedSession` either omits the new fields or fills them with `ProfileState::default()`, and the `profiles/` types are inert library code (no caller). This lets the wave merge while the runtime path stays dark.
- **Plain revert:** because no existing function was modified in behavior (only additive struct fields, additive `app.rs::Message` variants with display-only `update` arms, the one additive `manual_confirmed` field on `SelectedProfile`, and new files), reverting is: drop the two `Message` variants and their inert arms in `app.rs`, remove the two new `PreparedSession` fields and their initialization in `session_runner.rs`, and delete `profiles/selection.rs` + the new `tests/` files. The `profiles/registry.rs` / `model.rs` / `gm/lly.rs` edits can remain (they are unreferenced by live paths) or revert to Wave 1 stubs - except the `manual_confirmed` field, whose revert must also restore Wave 1's original `seal` signature and `selected_profile_round_trips_via_seal` test. No obd2-core change to revert.
- **What stays behind a flag for later waves:** the actual consumption of `ProfileState` as the polling/scan gate (replacing `session_matches_lly_profile`) is Wave 3 and must NOT be enabled here. Keep `SelectedProfile.is_valid_for` and `manual_confirmed` defined but unconsumed until the dispatcher exists.

### Hard problems and OWL hazards surfaced (do not smooth over)

- **The seal constructor must stay `pub(in crate::profiles)` - widening it is a profile-isolation hole (OWL critical).** `registry.rs` lives in `crate::profiles`, so `pub(in crate::profiles)` already lets the resolver call `SelectedProfile::seal`. Widening to `pub(crate)` (as an earlier draft did) is strictly wider and would let `session_runner.rs` / `app.rs` / `gm_*.rs` fabricate a `SelectedProfile` directly, bypassing the registry match-floor and defeating "token created only by the resolver" (plan line 87) and the sealed-token safety hinge (plan OWL line 946). Wave 1 explicitly said "Do NOT widen this to pub." Keep the visibility; do not rename `seal` to `mint`; and ship `seal_symbol_not_named_outside_profiles` so a future careless widening is caught even on a green build.
- **Do not redefine Wave 1 model types (OWL major).** `MatchConfidence` and `IdentityConfidence` are owned by Wave 1. Re-`enum`-ing them in Wave 2 (with a different variant set, or renaming `seal` to `mint` and bolting on `manual_confirmed` in a fresh definition) breaks Wave 1's shipped inline test `selected_profile_round_trips_via_seal` and risks the cross-wave type-instability failure where `MatchConfidence::High` / variants no wave defines stop compiling. Consume Wave 1's definitions; make only the single additive `manual_confirmed` change, and update Wave 1's test in the same commit.
- **The UI message goes in `app.rs::Message`, not `domain.rs::DomainMessage` (OWL major).** Verified: `domain.rs:140` defines `DomainMessage` (consumed by `update()` for recording/domain state); the UI-facing enum is `Message` at `app.rs:78`. A token-free UI snapshot belongs in `app.rs::Message` with its own `update()` arm; only recording/domain state belongs in `DomainMessage`. Adding `Message::ProfileStateUpdate`/`Message::IdentityConfidenceWarning` to a non-existent enum at `domain.rs:145` would not compile. Any message reference written in `domain.rs` context must read `DomainMessage::`.
- **Corroboration changes the wire unless gated (OWL minor).** `acquire_identity` performs up to `extra_reads` additional `Session::read_vin` (`$0902`) calls after `identify_vehicle`. If wired unconditionally into `prepare_session`, that adds reads versus today's single identify - a real (if benign) traffic change the LLY value corpus would not catch. Default `extra_reads` to 0 and gate any value > 0 behind the `profile-selection` feature flag, so the unflagged build issues identical VIN traffic. Document that corroboration (and therefore `Confirmed`) is opt-in until Wave 3 consumes the state.
- **"Session owns selection" is a layering fiction in this codebase.** `obd2_core::Session<A>` cannot hold a dash-side `ProfileState`/`SelectedProfile` without an illegal dependency. The real owner is the dash session lifecycle (`PreparedSession` in `session_runner.rs`). Document this; do not try to push `ProfileState` into obd2-core.
- **Equivalence is only safe if the matcher DELEGATES.** Reimplementing the LLY identity logic in `matches` risks a subtle divergence (`chars().nth(7)` vs `chars[7]`, trim, case folding) that the LLY decode golden corpus would NOT catch (it tests decode, not selection), silently changing which trucks select LLY. Delegate to `gm_enhanced::lly_profile_matches` and guard with the equivalence test.
- **Generation must be process-global, not per-task.** Each connection builds a fresh `Session` and `PreparedSession`; a per-task counter starting at 0 would reuse generation values across reconnects, so a stale token from a prior vehicle could validate against a new one in Wave 3. Use the global `AtomicU64`.
- **`read_vin` masks corruption.** It filters non-printable bytes and takes the first 17 printable chars (`session/mod.rs:451`), so a corrupt stream can yield a valid-looking 17-char VIN. Structural validation alone cannot catch the documented `I`/`1` confusion except via the ISO-3779 charset rule (no `I/O/Q`) plus cross-read agreement. Confidence requires agreement for `Confirmed`, not just a single clean-looking read - which is also why `Confirmed` is unreachable when `extra_reads == 0`.
- **`VinMatcher::matches` does not reject corrupted VINs** (`vehicle/mod.rs:129`, length-only) and `SpecRegistry::match_vin` returns the first match with no ambiguity detection (`:505`). Both the corrupted-VIN floor and the multi-exact ambiguity rule MUST live in the new dash resolver, not in obd2-core.
- **The new floor is strictly stricter than the old gate, by design.** `select` can refuse Exact (corrupted VIN, `Protocol::Auto`, weak confidence) where the old boolean gate would have accepted. This is intentional safety, not a regression - but it only takes effect when Wave 3 rewires the gate. In Wave 2 the old gate still drives live polling, so no real truck loses data; the equivalence test is therefore scoped to confirmed/clean VINs.
- **The primary corpus VIN-8 must be re-counted before freezing.** `1GTHK29294E391526` -> 8th char is `2` (matches `vin_8 = Some('2')`), but whether it reaches `Exact` also depends on the loaded spec's `vin_match.wmi_prefixes` admitting the captured WMI. Do not freeze `Exact` until that is verified; otherwise the primary firewall asserts the wrong outcome.
- **Ambiguity cannot be tested with one profile.** With only LLY registered, "no two Exact" is vacuously true. A `#[cfg(test)]` fixture profile is required now to exercise ambiguity and cross-profile isolation before real profiles land in later waves.

I have full evidence now. Confirmed facts driving the revision:
- `find_lly_did` is called cross-crate from the GUI (`apps/obd2-gui/src-tauri/src/main.rs:21,649,1354-1413`), plus `session_runner.rs:417` and examples -> it cannot be gated to `pub(crate)` this wave without breaking the GUI build, and its rewire needs LLY signals (Wave 4).
- `class2_routed_request`/`class2_header`/`class2_dtc_all_request`/`class2_dtc_active_request` and `gm_enhanced::routed_request`/`request_header` are on NO live path (only in-module tests + examples) -> safe to gate behind `probe` now.
- `request_gm_node` + `adapter_mut` are GUI-only (callers at `:692/784/788/797`); rewire splits across Wave 4 (enhanced reads) and Wave 5 ($19) -> gating deferred.
- `MockAdapter::routed_request` (mock.rs:300-310) downgrades `Addressed -> Broadcast`; harness must be `Elm327Adapter::new(Box<dyn Transport>)` + `MockTransport::expect(command, response)` (verified at elm327.rs:43, transport/mock.rs:20,29).

Here is the corrected section.

## Wave 3: Dispatcher

### Objective

Introduce `ProfileRuntime::execute_request`, the single protocol-agnostic execution path for every manufacturer-specific routed request: it validates the `SelectedProfile` (id + generation), confirms capability/route ownership, resolves a route to an `obd2_core::protocol::service::Target`, sends through the obd2-core session, decodes by `decoder_id` via the profile's OWN `decode_signal`/`decode_dtc_response`, and emits bounded evidence through a sink -- with zero per-OEM branches. Establish the compile-time probe boundary (`profiles::probe`, the `probe` Cargo feature) and quarantine the GM raw-framing helpers (`class2_routed_request`, `class2_header`, `class2_dtc_all_request`, `class2_dtc_active_request`, and `gm_enhanced`'s J1850 `routed_request`/`request_header` builders) behind it, enforced by an architectural import test plus a `probe`-off compile-fail fixture.

This wave is ADDITIVE-ONLY at the live call sites. It does NOT rewire any live consumer. Concretely:

- `session_runner.rs` `read_enhanced_target` (the `find_lly_did(target.did)` decode at `:417` and its `raw_request` send at `:411`) is LEFT INTACT. Its migration to `execute_request` is **Wave 4** (it cannot be done before LLY `SignalDefinition`s and `decode_signal` bodies exist).
- `session_runner.rs` `append_gm_class2_dtcs` (the `$19` `raw_request` send at `:512`) is LEFT INTACT. Its migration is **Wave 5**, because rewiring it here would strip the `GmClass2Backoff` 3-skip cadence (`session_runner.rs:63-71,633-666`) that Wave 5 owns -- a silent behavior change.
- `apps/obd2-gui/src-tauri/src/main.rs` `request_gm_node` (`:821`) and its three live callers (`read_gm_did_value :641`-ish via `find_lly_did :649`, `read_enhanced_scalar`, `scan_gm_class2_dtcs :776`) are LEFT INTACT. The GUI rewire ALSO depends on LLY signals and so splits across Wave 4 (enhanced reads) and Wave 5 ($19 scan), mirroring the session_runner split. This wave only establishes the boundary mechanism and records the GUI file on the architectural-test pending-migration allowlist; see OWL note below.

OWL on why the rewires are NOT here (the critical sequencing fact): Wave 1 ships a NEUTRAL model with an empty/placeholder registry and Wave 2's profile returns placeholder `signals()`. There is no LLY `SignalDefinition`, and no `decode_signal` body, until Wave 4. A dispatcher that "rewires `find_lly_did` through `profile.decode_signal`" in this wave would call into an unpopulated profile and return `CapabilityNotOwned`/blank values on a green build -- the exact silent-LLY-regression class this plan is meant to prevent. Therefore Wave 3 builds and proves the dispatcher against a SYNTHETIC fixture `DiagnosticProfile`; the real LLY decode-parity (zero-diff corpus) tests live in Wave 4 (enhanced) and Wave 5 ($19/$59).

### Depends on

- Wave 1 (Neutral Profile Model) MUST land first, but ONLY for the model surface -- NOT for a populated LLY profile. The dispatcher consumes, and cannot compile without: `ProfileId`, the sealed `SelectedProfile { profile_id, context_generation, _sealed: () }` with accessors `profile_id()` / `context_generation()`, the `DiagnosticProfile` trait (`signals()`, `dtc_services()`, `active_tests()`, `decode_signal()`, `decode_dtc_response()`), `SignalDefinition`, `DtcServiceDefinition`, module-only `RouteDefinition`, `ModuleMap` / `ModuleDefinition` / `AddressTemplate`, `RouteSet` (including `DiscoveredOnBus { bus }`, used by the DTC service route path), `DecodedSignal`, `DecodedDtc`, `ProfileDecodeError`, and `ProfileRegistry` exposing `ProfileRegistry::get(ProfileId) -> Option<&dyn DiagnosticProfile>`. Wave 3 drives these against a fixture profile; it does NOT require LLY `signals()` to be populated (Wave 4) or the GM `$19` `DtcServiceDefinition` to be wired (Wave 5).
  - Wave-1 shape constraints this wave RELIES ON (flagged because they are contested across waves and must be pinned in Wave 1, conformed to additively later): (a) `SelectedProfile`'s mint constructor stays `pub(in crate::profiles)` -- NOT `pub(crate)` -- so only the resolver mints tokens and no dash module can fabricate one (resolver-only minting; a `pub(crate)` mint is a profile-isolation hole). (b) `ProfileRegistry` stores `Vec<&'static dyn DiagnosticProfile>` and registers `&'static dyn DiagnosticProfile`; Wave 9's `&FIXTURE_PROFILE` registration must compile against the same `get` signature used here. (c) `ProfileDecodeError` and `DecodedSignal`/`DecodedDtc` final shapes are frozen in Wave 1; Wave 3 wraps the whole `ProfileDecodeError` in `DispatchError::Decode(..)` and does not pattern-match its variants, so it is insulated from variant churn -- but Wave 1 must still pin them.
- Wave 2 (Session-Owned Profile Selection) MUST land first. The dispatcher validates a token it does not mint: it needs the session to own `ProfileState { generation, selected: Option<SelectedProfile>, ... }`, a `VehicleContext { generation, protocol, ... }`, generation increment on disconnect/adapter/protocol/VIN/spec change, and resolver-only minting of `SelectedProfile`. Without Wave 2 there is no `generation` to bind to and no single gate to read.
- NO corpus dependency for THIS wave. Wave 3 asserts no LLY golden zero-diff (those move to Wave 4/5). It does require that Wave 0 has already pinned ONE corpus schema (signal_key + module keyed) and ONE directory layout + shared loader, so Wave 4/5 do not have to rewrite frozen files (Regression Firewall). Wave 3 does not read the corpus.

OWL on ordering: a careless implementer who starts the dispatcher AND rewires `read_enhanced_target` in the same change (because "the dispatcher is the point") will wire live LLY reads into an empty profile registry and ship blank VGT/baro on a passing build. The hard rule: in this wave the dispatcher has exactly one driver -- the fixture profile in `tests/dispatcher.rs`. Do not touch `session_runner.rs` or `apps/obd2-gui/src-tauri/src/main.rs` live code paths.

### Out of scope (explicit non-goals -- surfaced because other waves wrongly assume Wave 3 owns them)

- **`plan_poll_cycle` and `src/profiles/scheduler.rs` are NOT created here.** Wave 3's only runtime API is `execute_request`. Wave 3.5 owns `profiles/scheduler.rs` + `ProfileRuntime::plan_poll_cycle` and migrates the still-global poll policy (`should_force_standard_poll` at `session_runner.rs:131/824` with pinned tests `:1087/:1108`; cadence `cycle % 5/10/20/60`; candidate-DID suppression `0x1542`; `preferred_over` `0x163E` over generic rail; the GUI copies at `apps/obd2-gui/src-tauri/src/main.rs`). Wave 4/5/6/9 depend on Wave 3.5 for scheduling; Wave 3 must not absorb that work.
- **No session_runner / GUI rewire** (see Objective). Wave 3 removes ZERO call sites, so it does NOT decrement the Wave 0 frozen call-site allowlist. Wave 4 decrements `find_lly_did` (`session_runner.rs:30,417`; `apps/obd2-gui/src-tauri/src/main.rs:21,649`) and the enhanced-read `raw_request` (`:411`); Wave 5 decrements the `$19` `raw_request` (`:512`) and the remaining `request_gm_node` callers. Each of those waves owns its allowlist decrement.

### Files touched

- CREATE `crates/obd2-dash/src/profiles/runtime.rs` -- `ProfileRuntime`, `ProfileRuntime::execute_request`, `CapabilityId`, `RequestId`, `ProfileResponse`, `DispatchError`, `resolve_route`, `resolve_route_target`, `bus_family`, `ProfileEvidenceSink` trait, `NullEvidenceSink`, `DispatchEvidence`.
- MODIFY `crates/obd2-dash/src/profiles/mod.rs` -- add `pub mod runtime;` and the probe module declaration: `#[cfg(feature = "probe")] pub mod probe;` / `#[cfg(not(feature = "probe"))] pub(crate) mod probe;`.
- CREATE `crates/obd2-dash/src/profiles/probe.rs` -- the ONLY live re-export point for the gated raw framing helpers, all `#[cfg(feature = "probe")]`-gated public: `pub use crate::gm_class2::{class2_routed_request, class2_header, class2_dtc_all_request, class2_dtc_active_request};`. Examples that build raw J1850 frames import from here under `--features probe`. NOTE: `find_lly_did` is NOT re-exported here this wave -- it stays `pub` in `gm_enhanced` because `session_runner`, the GUI, and the decode-only examples still call it; its move behind `probe` is **Wave 4** (which adds the `pub use crate::gm_enhanced::find_lly_did;` line here).
- MODIFY `crates/obd2-dash/src/gm_class2.rs` -- change `class2_header` (`:75`), `class2_routed_request` (`:79`), `class2_dtc_all_request` (`:90`), `class2_dtc_active_request` (`:98`) from `pub` to `#[cfg(feature = "probe")] pub` / `#[cfg(not(feature = "probe"))] pub(crate)`. These are confirmed off all live paths (only `gm_enhanced` wraps them, plus the in-module test at `:336` and probe examples), so gating is behavior-neutral. KEEP `decode_class2_dtcs` (`:220`), `GmClass2DtcRecord::into_dtc` (`:185`), `GmClass2Status`, and the `CLASS2_DTC_*_REQUEST` const data `pub` -- decode and request-byte data are Layer-3 capability content the GM profile legitimately owns; only the header/framing builders are quarantined.
- MODIFY `crates/obd2-dash/src/gm_enhanced.rs` -- change `GmDidDefinition::routed_request` (`:203`, calls `class2_routed_request`) and `request_header` (`:200`, calls `class2_header`) to `#[cfg(feature = "probe")] pub` / `#[cfg(not(feature = "probe"))] pub(crate)`; they build J1850 framing and are a Layer-1 reach-around, and are confirmed off all live paths (only the in-module test at `:919-922` and probe examples call them). KEEP `find_lly_did` (`:613`) `pub` -- do NOT gate it this wave (cross-crate GUI caller at `apps/obd2-gui/src-tauri/src/main.rs:649` + GUI in-module tests `:1354-1413` + `session_runner.rs:417` + decode examples still depend on it; gating it to `pub(crate)` would break the GUI build with no rewire available). KEEP `decode_value`, `decode_did_value`, `select_rxd_raw`, `apply_mth`, `request_data` `pub` (Layer-3 decode/payload content).
- MODIFY `crates/obd2-dash/Cargo.toml` -- add `[features] probe = []`, and add `[[example]]` entries with `required-features = ["probe"]` for every example that builds raw frames or calls the gated wrappers: `gm_class2_probe`, `gm_desired_map_probe`, `gm_desired_map_watch`, `gm_drive_logger`, `gm_pressure_probe`. (Examples calling only `find_lly_did` + `decode_value` would still build without `probe` this wave, but any example calling `.routed_request()`/`.request_header()` now needs `probe`; gating them all is the safe, durable choice.)
- MODIFY `apps/obd2-gui/src-tauri/Cargo.toml` -- ensure it does NOT enable obd2-dash's `probe` feature (it must build with `request_gm_node`, `find_lly_did`, and `adapter_mut` still reachable, because the GUI is unmodified this wave).
- CREATE `crates/obd2-dash/tests/dispatcher.rs` -- unit + Elm327Adapter/MockTransport integration tests driven by a SYNTHETIC fixture profile (see Tests).
- CREATE `crates/obd2-dash/tests/architectural_import.rs` -- source-scan import test with a wave-tagged pending-migration allowlist (see Tests).
- CREATE `crates/obd2-dash/tests/probe_gate/` (trybuild `compile_fail` fixtures) -- the `probe`-off compile-fail teeth (see Tests).

NOT touched this wave (called out to prevent accidental edits): `crates/obd2-dash/src/session_runner.rs` (Wave 4/5), `apps/obd2-gui/src-tauri/src/main.rs` live code (Wave 4/5), `crates/obd2-dash/tests/corpus_replay.rs` (created/extended by Wave 4 for enhanced goldens, Wave 5 for `$59` goldens).

OWL on the GUI deferral (reconciling the dash-inventory "CRITICAL OWL FINDING"): `request_gm_node` does not live in the dash crate; it is at `apps/obd2-gui/src-tauri/src/main.rs:821` and reaches the bus via `session.adapter_mut().routed_request(&request)` at `:836` -- the exact single-flight/`BusNotAvailable`-bypassing reach-around the dispatcher replaces. A reader who greps only the dash crate would miss it. BUT the GUI rewire cannot be done in this wave: its enhanced-read callers need LLY `SignalDefinition`s (Wave 4) and its `scan_gm_class2_dtcs` caller needs the GM `$19` `DtcServiceDefinition` (Wave 5). Doing a partial GUI rewire here would silently kill VGT (`0x1543`/`0x1540`) and all 8 injector-balance reads (`0x162F..=0x1636`) -- `read_enhanced_scalar` is a SEPARATE path from `read_gm_did_value`, so migrating one and not the other produces blank values with no compile error. Therefore Wave 3's only GUI action is to add `apps/obd2-gui/src-tauri/src/main.rs` to the architectural test's pending-migration allowlist (tagged "Wave 4: request_gm_node + find_lly_did enhanced callers" and "Wave 5: scan_gm_class2_dtcs"); the gating of `request_gm_node`/`adapter_mut` is finalized in Wave 5 once the last caller is migrated.

### Exact APIs

New module `crates/obd2-dash/src/profiles/runtime.rs`. All new symbols; Wave 1 owns every type referenced by path below.

```rust
use obd2_core::adapter::Adapter;
use obd2_core::protocol::codec::BusFamily;
use obd2_core::protocol::service::Target;
use obd2_core::session::Session;
use obd2_core::vehicle::{PhysicalAddress, Protocol};
use obd2_core::error::Obd2Error;

use crate::profiles::model::{
    DecodedDtc, DecodedSignal, DiagnosticProfile, DtcServiceDefinition, ModuleDefinition,
    ModuleMap, ProfileDecodeError, ProfileId, RouteDefinition, RouteResolveError,
    SelectedProfile, SignalDefinition, VehicleContext,
};
use crate::profiles::registry::ProfileRegistry;

/// Identifies which profile-owned capability a single request exercises.
/// The &'static str is the capability `key` from the matching definition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapabilityId {
    Signal(&'static str),     // SignalDefinition.key
    DtcService(&'static str), // DtcServiceDefinition.key
    ActiveTest(&'static str), // ActiveTestDefinition.key (rejected this wave)
}

/// Selects one concrete route within a capability's RouteSet.
/// Signals have exactly one route -> RequestId::SINGLE.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestId(pub usize);
impl RequestId { pub const SINGLE: RequestId = RequestId(0); }

#[derive(Debug)]
pub enum ProfileResponse {
    Signal(DecodedSignal),
    Dtcs(Vec<DecodedDtc>),
}

/// Dispatcher-layer error. MUST NOT be folded into obd2_core::Obd2Error::Adapter:
/// Adapter(String) drives ConnectionState::Error semantics via event handling, and
/// a safety-gate rejection is not a transport fault. (core inventory item 8)
#[derive(Debug)]
pub enum DispatchError {
    /// selected.profile_id() not present in the registry.
    UnknownProfile(ProfileId),
    /// token generation != ctx.generation (stale authority).
    StaleGeneration { token: u64, current: u64 },
    /// capability key not found on the selected profile.
    CapabilityNotOwned { profile: ProfileId, capability: CapabilityId },
    /// request_id out of range / route not declared by the capability.
    RouteNotOwnedByCapability { capability: CapabilityId, request: RequestId },
    /// route address family does not match the live protocol, or protocol is Auto.
    ProtocolFamilyMismatch { route: BusFamily, live: Option<BusFamily> },
    /// active test reached the dispatcher while locked (always, this wave).
    ActiveTestLocked { capability: CapabilityId },
    /// underlying transport/session failure (NoData, Timeout, BusNotAvailable, ...).
    Transport(Obd2Error),
    /// profile decoder rejected the payload.
    Decode(ProfileDecodeError),
}

/// Bounded evidence for one executed request. Generalized record is Phase 6/Wave 6;
/// this wave emits only what the dispatcher already has, through a sink.
pub struct DispatchEvidence<'a> {
    pub profile_id: ProfileId,
    pub capability: CapabilityId,
    pub route: &'a RouteDefinition,
    pub service_id: u8,
    pub request_data: &'a [u8],
    pub physical_address: PhysicalAddress, // from ModuleMap route resolution, for the record
    pub raw_payload: &'a [u8],
    pub decoder_id: &'static str,
    pub outcome: Result<&'a ProfileResponse, &'a DispatchError>,
    pub is_probe: bool, // always false from execute_request; true only via probe API
}

pub trait ProfileEvidenceSink {
    fn record(&mut self, evidence: &DispatchEvidence<'_>);
}

/// Default sink: writes nothing. Used by tests (and, once wired in Wave 4/5, by the
/// TUI live path) so that no new evidence files appear -> zero behavior diff.
pub struct NullEvidenceSink;
impl ProfileEvidenceSink for NullEvidenceSink {
    fn record(&mut self, _evidence: &DispatchEvidence<'_>) {}
}

pub struct ProfileRuntime<'r> {
    registry: &'r ProfileRegistry,
}

impl<'r> ProfileRuntime<'r> {
    pub fn new(registry: &'r ProfileRegistry) -> Self { Self { registry } }

    /// THE single execution path for manufacturer-specific routed requests.
    /// No `match manufacturer { ... }` anywhere in this function.
    pub async fn execute_request<A: Adapter, E: ProfileEvidenceSink>(
        &self,
        session: &mut Session<A>,
        ctx: &VehicleContext,
        selected: &SelectedProfile,
        capability: CapabilityId,
        request: RequestId,
        evidence: &mut E,
    ) -> Result<ProfileResponse, DispatchError>;
}

/// Resolved module route. The route carries only ModuleKey; ModuleMap supplies bus
/// + address. This is the only place an AddressTemplate becomes bytes.
pub struct ResolvedRoute<'a> {
    pub route: &'a RouteDefinition,
    pub module: &'a ModuleDefinition,
    pub bus_family: BusFamily,
    pub physical_address: PhysicalAddress,
    pub target: Target,
}

/// Mechanical, exhaustive route resolution. Protocol-shaped, NOT manufacturer-shaped.
/// The J1850 header is composed from ModuleMap bus data (for LLY: 6C <node> F1),
/// not from `RouteDefinition` and not from a GM branch.
pub fn resolve_route<'a>(
    map: &'a ModuleMap,
    route: &'a RouteDefinition,
    active: Protocol,
) -> Result<ResolvedRoute<'a>, RouteResolveError>;

/// The live send goes through Target::Module so obd2-core resolves the address
/// from discovery (preserving the BusNotAvailable guard + single-flight), NOT by
/// handing a PhysicalAddress straight to the adapter. ResolvedRoute.physical_address
/// is used for evidence and for the discovery cross-check below.
pub fn resolve_route_target(route: &RouteDefinition) -> Target {
    Target::Module(route.module.canonical().to_string())
}

/// Protocol -> BusFamily for the family-match gate. Auto returns None so a
/// misdetected/auto bus can NEVER satisfy ProtocolFamilyMismatch. Duplicates
/// elm327::protocol_family (which is a private adapter method, not callable here);
/// pinned by a test and kept additive-only.
pub fn bus_family(protocol: Protocol) -> Option<BusFamily> {
    match protocol {
        Protocol::J1850Vpw | Protocol::J1850Pwm => Some(BusFamily::J1850),
        Protocol::Can11Bit500 | Protocol::Can11Bit250
        | Protocol::Can29Bit500 | Protocol::Can29Bit250 => Some(BusFamily::Can),
        Protocol::Iso9141(_) => Some(BusFamily::Iso9141),
        Protocol::Kwp2000(_) => Some(BusFamily::Kwp2000),
        Protocol::Auto => None,
        _ => None, // Protocol is #[non_exhaustive]: unknown future variants do not match
    }
}
```

`execute_request` body, in exact order (this is the validation contract from plan lines 326-334):

1. `let profile: &dyn DiagnosticProfile = self.registry.get(selected.profile_id()).ok_or(DispatchError::UnknownProfile(selected.profile_id()))?;`
2. Generation: `if selected.context_generation() != ctx.generation { return Err(DispatchError::StaleGeneration { token: selected.context_generation(), current: ctx.generation }); }`
3. Capability ownership: resolve `capability` to a `&SignalDefinition` (via `profile.signals().iter().find(|s| s.key == key)`) or `&DtcServiceDefinition`; `None -> CapabilityNotOwned`. `CapabilityId::ActiveTest(_) -> Err(DispatchError::ActiveTestLocked { capability })` (Phase 8 unlocks; preserves current always-blocked behavior).
4. Route ownership: get the `RouteDefinition` for `request` from the capability (`SignalDefinition` has one route; `DtcServiceDefinition.route_set` is indexed/policy-resolved by `request.0`, including the `RouteScope::DiscoveredOnBus { bus }` policy); out-of-range/undeclared -> `RouteNotOwnedByCapability`.
5. Resolve route from the profile's `ModuleMap`: `let resolved = resolve_route(profile.module_map(), route, ctx.protocol)?;` Map `RouteResolveError::BusNotActive` to `ProtocolFamilyMismatch`; map unresolved/candidate/unknown module to the corresponding dispatch error. The route itself carries no address.
6. Service/payload: source `service_id` and `request_data` ONLY from the capability definition (`signal.service_id`/`signal.request_data`, or `dtc.service_id`/`dtc.request_data`). There is no caller-supplied byte path, so "request payload matches the capability definition" holds structurally.
7. Resolve target: use `resolved.target` for the send and `resolved.physical_address` for evidence + optional cross-check.
8. Execute: `let payload = session.raw_request(service_id, request_data, target).await.map_err(DispatchError::Transport)?;` -- this funnels through `Session::send_request` (single-flight `request_in_flight` guard, `resolve_module_address` bus-availability check, `record_visible_target`, `apply_adapter_events`). `execute_request` is the ONE sanctioned caller of `raw_request` in live dash code once Wave 4/5 finish migrating the legacy sites.
9. Decode by `decoder_id`: signal -> `let decoded = profile.decode_signal(signal, &payload).map_err(DispatchError::Decode)?; ProfileResponse::Signal(decoded)`. DTC -> `let dtcs = profile.decode_dtc_response(dtc, &payload).map_err(DispatchError::Decode)?; ProfileResponse::Dtcs(dtcs)`. The runtime NEVER calls `find_lly_did`; isolation is the profile's own registry inside `decode_signal`.
10. Evidence: build `DispatchEvidence { is_probe: false, .. }` and `evidence.record(&ev)`. Tests pass `&mut NullEvidenceSink` -> no files written.
11. Return the `ProfileResponse`.

Referenced obd2-core signatures (unchanged, from the core inventory):
- `Session::raw_request(&mut self, service: u8, data: &[u8], target: Target) -> Result<Vec<u8>, Obd2Error>` (`session/mod.rs:1020`, public; ends at `send_request`).
- `Target::Module(String)` / `Target::Broadcast` (`protocol/service.rs:149`).
- `PhysicalAddress::J1850 { node: u8, header: [u8; 3] }` / `Can11Bit { request_id: u16, response_id: u16 }` / `Can29Bit { request_id: u32, response_id: u32 }` (`vehicle/mod.rs:35`, `#[non_exhaustive]`; we construct, never `match`, so the missing `J1939` arm and non-exhaustiveness do not break us).
- `RoutedRequest { service_id: u8, data: Vec<u8>, target: PhysicalTarget }` (`adapter/mod.rs:29`) -- the dispatcher does NOT build this directly; `raw_request -> resolve_request` builds it. We deliberately avoid `Adapter::routed_request`/`adapter_mut()` to keep the bus check and single-flight.
- `BusFamily { Can, J1850, Iso9141, Kwp2000 }` (`protocol/codec.rs:6`).

OWL on the address-resolution design fork (the single hardest decision in this wave):

- The earlier route-level address model is retired. `RouteDefinition` has no address; it carries only `ModuleKey`. Bus + address live once in `ModuleMap`.
- The LIVE send uses `Target::Module(route.module.canonical())` and lets obd2-core resolve the address from discovery (the LLY spec's `BusConfig.modules` already carry `PhysicalAddress::J1850 { node, header: [0x6C, node, 0xF1] }`, per `vehicle/mod.rs:906`). This is exactly how today's TUI selector path already works (`read_enhanced_target` uses `Target::Module(target.module_id.0.clone())` at `session_runner.rs:414`), so when Wave 4 rewires that path it is byte-for-byte behavior-preserving.
- `resolve_route(profile.module_map(), route, active_protocol)` is retained for evidence and debug cross-checks. It looks up the module in `ModuleMap`, checks the active protocol, rejects candidate/unresolved addresses, composes the `PhysicalAddress`, and returns the canonical `Target::Module`.
- The cross-check (recommended `debug_assert!`/test, not a hard runtime gate): `resolved.physical_address` must equal the address discovery resolved for `resolved.target`. This is the firewall against the ECM/TCM mislabel (dash inventory #3): with `route.module = ModuleKey::Tcm` for `0x1940`, discovery resolves TCM node `0x18` AND the ModuleMap resolver composes `[0x6C,0x18,0xF1]`; they agree. A future ModuleMap entry whose node disagrees with the embedded spec is caught here.
- The `6C <node> F1` synthesis is bus data (`J1850HeaderConvention`) consumed by `resolve_route`, NOT a manufacturer branch. If a future J1850 profile needs a non-`6C` priority byte, change that profile's bus data; do not special-case by manufacturer in Layer 2.

OWL on the DtcService route-set (deferred to Wave 5, but the dispatcher contract is fixed here): today `gm_class2_scan_modules` delegates to the generic `dtc_scan_modules` (discovered modules on the active bus), NOT `DEFAULT_CLASS2_NODES` (core inventory item 3). The dispatcher's `CapabilityId::DtcService` path resolves routes from the capability's `RouteSet`; to keep `$19` coverage identical when Wave 5 wires it, the LLY `$19` `DtcServiceDefinition.route_set` MUST be a `RouteScope::DiscoveredOnBus { bus }` policy (which Wave 1 provides) so the dispatcher executes one `(DtcService, discovered-module)` pair per call and validates "route belongs to capability" as "module is a discovered module on the capability's declared bus." Switching `$19` to an explicit static node list is a DELIBERATE coverage change deferred to a later wave with its own corpus -- it is not done in any of Waves 3/4/5 as currently scoped, or `$19` either narrows (fewer DTCs) or fires at non-Class2 nodes. Wave 3 only fixes the contract; Wave 5 owns the live `$19` rewire and its `GmClass2Backoff` interaction.

### Tests

Integration-test HARNESS (applies to every test below that needs a bus): use `obd2_core::adapter::elm327::Elm327Adapter::new(Box<dyn Transport>)` seeded with a `MockTransport` (`obd2_core::transport::mock::MockTransport::new()`) primed via `MockTransport::expect(command, response)`, then wrap in `Session`. Do NOT use `obd2_core::adapter::mock::MockAdapter` for byte-accurate addressed J1850 replay: `MockAdapter::routed_request` (`adapter/mock.rs:300-313`) downgrades every `PhysicalTarget::Addressed` to `Broadcast` and returns canned `[0x80,0x00]` for service `0x22`, so it physically cannot honor an addressed J1850 routed request. "Fixing the mock" is an obd2-core change and is forbidden by this wave's "`cargo test -p obd2-core` passes unchanged" criterion. The same harness rule applies to the integration tests in Waves 4/5/6/7.

Unit (`crates/obd2-dash/src/profiles/runtime.rs` `#[cfg(test)]`):
- `resolve_route_j1850_uses_module_map_header` -- a fixture `ModuleMap` with TCM on node `0x18` resolves `RouteDefinition { module: ModuleKey::Tcm }` to `Target::Module("tcm")` and `PhysicalAddress::J1850 { node: 0x18, header: [0x6C, 0x18, 0xF1] }`; also assert ECM `0x10` and EBCM/ABS `0x29`. The single most important regression pin in the wave.
- `resolve_route_can_passthrough` -- `Can11`/`Can29` templates stored on `ModuleDefinition.address` map field-for-field with no header synthesis.
- `bus_family_rejects_auto` -- `bus_family(Protocol::Auto) == None`; `J1850Vpw -> Some(J1850)`; `Can11Bit500 -> Some(Can)`. Guards core inventory Failure Mode 6.

Integration with a SYNTHETIC fixture profile (`crates/obd2-dash/tests/dispatcher.rs`). Define a `FixtureProfile` implementing `DiagnosticProfile` with: one J1850 `ModuleMap` entry for `ModuleKey::Tcm` at node `0x18`; one `SignalDefinition { key: "fix_signal", service_id: 0x22, request_data: [0x15, 0x40], decoder_id: "fix", route: RouteDefinition { module: ModuleKey::Tcm } }` whose `decode_signal` returns a deterministic `DecodedSignal`; one `DtcServiceDefinition { key: "fix.dtc", service_id: 0x19, ... route_set: RouteSet::discovered_on_bus(BusKey::new("j1850vpw")) }`; and one `ActiveTestDefinition`. Register it in a `ProfileRegistry` and mint a `SelectedProfile` through the Wave-2 resolver (or a `#[cfg(test)]` mint helper if the resolver requires a full context). The LLY-specific decode-parity tests are NOT here -- they are Wave 4/5 (they need the real LLY profile).

- `execute_signal_reads_and_decodes` -- `MockTransport` primes the `22 15 40` command with a fixture `62 15 40 ..` response; `execute_request(.., CapabilityId::Signal("fix_signal"), RequestId::SINGLE, &mut NullEvidenceSink)` returns `ProfileResponse::Signal` equal to `FixtureProfile::decode_signal`'s expected output. Proves the happy path end to end through `Session::raw_request`.
- `stale_generation_rejected` -- token minted at `generation = 1`, `ctx.generation = 2` -> `Err(StaleGeneration { token: 1, current: 2 })`; assert the `MockTransport` saw ZERO commands (no adapter write before the gate).
- `capability_not_owned_rejected` -- `CapabilityId::Signal("does_not_exist")` -> `CapabilityNotOwned`; zero transport commands.
- `unknown_profile_rejected` -- token id absent from registry -> `UnknownProfile`; zero transport commands.
- `route_not_owned_rejected` -- `CapabilityId::DtcService("fix.dtc")` with `RequestId(99)` out of range -> `RouteNotOwnedByCapability`; zero transport commands.
- `protocol_family_mismatch_rejected` -- `ctx.protocol = Can11Bit500` with the J1850 fixture route, and separately `ctx.protocol = Auto` -> both `ProtocolFamilyMismatch`; zero transport commands.
- `active_test_capability_is_locked` -- `CapabilityId::ActiveTest(..)` -> `ActiveTestLocked`; zero transport commands (preserves "locked tests do not send bytes").
- `dtc_service_dispatches_per_module` -- `MockTransport` primes a `59 ..` response; `execute_request(.., CapabilityId::DtcService("fix.dtc"), RequestId(module_index), ..)` returns `ProfileResponse::Dtcs` from `FixtureProfile::decode_dtc_response`. Proves per-module DTC dispatch generically; the LLY `decode_class2_dtcs -> into_dtc` parity is Wave 5.
- `partial_match_cannot_dispatch` (Invariant 5 end-to-end) -- a `VehicleContext` whose Wave-2 selection produced only a Partial match yields `ProfileState.selected == None`, so there is no `SelectedProfile` to pass to `execute_request` (type-enforced: the caller has no token). Additionally, fabricate a stale/foreign token and assert `execute_request` rejects it (`UnknownProfile` or `StaleGeneration`) with ZERO transport commands. Proves "a partial match is visible but cannot poll manufacturer-specific requests."

Architectural import (`crates/obd2-dash/tests/architectural_import.rs`):
- `live_code_cannot_reach_gated_framing_helpers` -- walk `crates/obd2-dash/src/**/*.rs`; assert the symbols quarantined THIS wave -- `class2_routed_request`, `class2_header`, `class2_dtc_all_request`, `class2_dtc_active_request`, and `GmDidDefinition::routed_request`/`request_header` framing builders -- appear ONLY in the allowlist `src/profiles/runtime.rs`, `src/profiles/probe.rs`, plus their definition/in-module-test sites (`src/gm_class2.rs`, `src/gm_enhanced.rs`) and `examples/**`. Fail with offending `file:line`.
- `pending_migration_allowlist` -- a SEPARATE, wave-tagged allowlist for symbols NOT yet quarantined, asserting they appear ONLY at their known legacy sites so a stray new caller still fails the build. Entries:
  - `find_lly_did`: allowed at `src/session_runner.rs:30,417`, `apps/obd2-gui/src-tauri/src/main.rs:21,649` (+ GUI tests `:1354-1413`), `src/gm_enhanced.rs` (def + tests), `examples/**`. Tag: "Wave 4 removes the session_runner + GUI enhanced callers."
  - `request_gm_node` / `.adapter_mut(`: allowed only at `apps/obd2-gui/src-tauri/src/main.rs`. Tag: "Wave 4 (enhanced) + Wave 5 ($19) remove callers; Wave 5 finalizes the gate."
  - `.raw_request(`: allowed at `src/profiles/runtime.rs` (the sanctioned path) AND the legacy sites `src/session_runner.rs:411` (enhanced; tag "Wave 4 removes") and `:512` ($19; tag "Wave 5 removes") AND the permanent generic-SAE site `src/session_runner.rs:564` (`append_dtc_probe`, Mode 03/07/0A). OWL: a naive "ban all `raw_request`" test is WRONG -- it would kill the legitimate generic DTC scan, which sends `service.service_id()` for `Stored/Pending/Permanent` = `03/07/0A` only. As Wave 4 and Wave 5 land, they DELETE their respective entries from this list, tightening the net to "runtime + generic-SAE" by end of Wave 5.
- `probe_helpers_do_not_compile_into_binary` (compile gate -- the real teeth, `crates/obd2-dash/tests/probe_gate/` via `trybuild`) -- a `compile_fail` fixture `use obd2_dash::gm_class2::class2_routed_request;` built WITHOUT the `probe` feature asserts it fails to compile (symbol is `pub(crate)`). This is stronger than the source scan, which is fragile to comments/strings/renames. NOTE: a `find_lly_did` compile-fail fixture is NOT added this wave (the symbol is still `pub`); Wave 4 adds it.

OWL on the architectural test: source scanning is brittle (matches in comments, doc strings, test names, or a rename slips past). Treat the `probe`-feature compile gate as primary for the lib symbols it can reach; treat the source scan as a coarse net, and emit `file:line` so failures are actionable. The cross-crate seams (`request_gm_node`, `adapter_mut`, `raw_request`) live in obd2-core's `pub` surface or in a separate crate (the GUI) and cannot be feature-gated from the dash side, so the source scan is the only available net for them until Wave 4/5 delete the callers. The pending-migration allowlist is deliberately explicit so that "this seam is known and owned by a later wave" is a tested fact, not a TODO comment.

### Acceptance criteria

- [ ] `cargo test -p obd2-dash` and `cargo test -p obd2-core` pass; the GUI crate builds without the `probe` feature (with `request_gm_node`, `find_lly_did`, and `adapter_mut` still reachable, because the GUI is unmodified this wave).
- [ ] `cargo build -p obd2-dash --features probe` and `cargo build -p obd2-dash --examples --features probe` succeed (the gated examples compile under `probe`); `cargo build -p obd2-dash --examples` WITHOUT `probe` fails only for the framing-using examples, confirming the gate bites.
- [ ] Existing pinned in-module unit tests UNCHANGED and green under default (no-`probe`) features, because they are same-crate callers of now-`pub(crate)` helpers: `gm_class2.rs::builds_class2_routed_request_header` (`class2_routed_request(0x18, 0x22, ..)` header at `:336`), `gm_enhanced.rs::builds_mode22_routed_request_from_definition` (`:919`, calls `definition.routed_request()` / `request_header()`), and the LLY DID-count / rejected-DID pins. Verify NO test referenced these helpers across a crate boundary (the GUI tests reference only `find_lly_did`, which stays `pub`).
- [ ] `resolve_route(ModuleMap, RouteDefinition, Protocol::J1850Vpw)` composes `[0x6C, node, 0xF1]` for `0x10/0x18/0x29`, matching `class2_header` and the LLY spec; `Protocol::Auto` never satisfies the family gate.
- [ ] Stale-generation, unknown-profile, capability-not-owned, route-not-owned, and protocol-family-mismatch all reject BEFORE any adapter write (assert the `MockTransport` saw zero commands), and surface as `DispatchError`, never `Obd2Error::Adapter`.
- [ ] `partial_match_cannot_dispatch` is green: a Partial-matched context yields no `SelectedProfile`, and a fabricated/stale token is rejected with zero transport commands (Invariant 5 end-to-end).
- [ ] Architectural test green: the framing helpers (`class2_routed_request`, `class2_header`, `class2_dtc_all_request`, `class2_dtc_active_request`, `routed_request`/`request_header`) appear only in `runtime.rs`/`probe.rs`/their definition+test sites/examples; the `probe`-off `compile_fail` fixture confirms `class2_routed_request` does not compile into the binary. The pending-migration allowlist documents the still-live `find_lly_did`/`request_gm_node`/`adapter_mut`/legacy-`raw_request` seams with their owning wave.
- [ ] No `match manufacturer { Gm => .., Ford => .. }` exists in `runtime.rs`; the only OEM-relevant resolution is `ModuleMap` lookup plus the protocol-shaped `match AddressTemplate` inside `resolve_route`.
- [ ] No new evidence files are written by tests (they pass `&mut NullEvidenceSink`).
- [ ] Shared-layer change is additive only: no obd2-core `Protocol`/`PhysicalAddress`/`BusFamily` variant or match arm modified; `bus_family`'s `_ =>` arm covers the `#[non_exhaustive]` enums. The integration tests use `Elm327Adapter` + `MockTransport`, NOT a modified `MockAdapter`.

Explicitly NOT acceptance criteria this wave (moved to their owning wave -- listed so a reviewer does not "fix" Wave 3 by smuggling them in):
- "No `find_lly_did` call remains in `session_runner.rs`; import at line 30 no longer lists it" -- **Wave 4**.
- "`$19` send no longer originates from `append_gm_class2_dtcs`; coverage unchanged via `dtc_scan_modules`; `GmClass2Backoff` preserved" -- **Wave 5**.
- "GUI VGT (`0x1543`/`0x1540`) and 8 injector-balance (`0x162F..=0x1636`) reads route through `execute_request` and still produce values" -- **Wave 4** (enhanced reads) and **Wave 5** ($19 scan).
- "LLY golden corpus zero-diff" -- `lly_enhanced_corpus_zero_diff` is **Wave 4**; `lly_dtc_corpus_zero_diff` is **Wave 5**.
- "`DiagnosticSnapshot` JSON shape byte-identical / capability-driven UI" -- **Wave 6**.
- "Generic SAE DTC scan (`append_dtc_probe`, Mode 03/07/0A) unchanged" -- this remains TRUE this wave only because Wave 3 does not touch `session_runner.rs` at all; it is formally pinned by Wave 5's `raw_request` allowlist tightening.

### Rollback notes

- Feature flag: the probe boundary is gated by the new `probe` Cargo feature. Reverting the gating (making `class2_routed_request`/`class2_header`/`class2_dtc_all_request`/`class2_dtc_active_request` and `routed_request`/`request_header` `pub` again) is a one-line-per-symbol change and does not touch the dispatcher. The dispatcher itself is purely additive (new `profiles/runtime.rs`, `profiles/probe.rs`, the three new test files); deleting those files plus reverting the `gm_class2.rs`/`gm_enhanced.rs` visibility lines and the `Cargo.toml` `[features]`/`[[example]]` additions restores pre-wave behavior exactly. Because Wave 3 makes NO live `session_runner.rs`/GUI edits, there is no live-path rollback to perform.
- Independent shippability: the entire wave is self-contained additive infrastructure (dispatcher + boundary + tests). It does not depend on Wave 4/5 to compile or pass, and it does not change any user-visible behavior -- the live TUI and GUI paths still run the legacy `find_lly_did`/`$19`/`request_gm_node` code unchanged. This is intentional: Wave 3 ships the engine; Wave 4/5 connect the drivetrain.
- What stays behind a flag after the wave: the raw J1850 framing builders remain reachable ONLY under `--features probe` (examples and probe tools). `find_lly_did`, `request_gm_node`, and `adapter_mut` are NOT yet gated -- they are gated/removed by Wave 4/5. This is the durable guardrail for the framing builders; it is not meant to be removed.
- Corpus safety: Wave 3 does not create or read corpus goldens. The Wave-0 frozen `tests/corpus/` files are untouched; rolling back Wave 3 cannot drift them. When Wave 4/5 add corpus zero-diff tests, the rule is the same -- do not "fix" a red corpus by editing the golden; revert the code.
- Known deferrals carried out of this wave (explicitly NOT done here, each a separate behavior change with its own corpus, and must not be smuggled into the dispatcher wave): `read_enhanced_target` enhanced-read rewire + `find_lly_did` import removal + GUI enhanced rewire (Wave 4); `$19` rewire + `GmClass2Backoff` interaction + `request_gm_node`/`adapter_mut` final gate (Wave 5); TCM-vs-`ecm` display-label fix (Phase 5); any `$19` route-set move from discovered modules to static profile nodes (later coverage-change wave); global `should_force_standard_poll`/poll-policy relocation into `profiles/scheduler.rs` + `plan_poll_cycle` (Wave 3.5); generalized evidence record (Phase 6); `DiagnosticSnapshot` capability-driven shape (Wave 6/UI).

## Wave 3.5: Poll Policy and Scheduler

### Objective

Move all polling POLICY out of global session/UI code into a profile-owned, manufacturer-agnostic scheduler. Create `ProfileRuntime::plan_poll_cycle`, which builds a per-cycle request plan from the selected profile's `signals()` + `dtc_services()` + cadence classes, with NO per-manufacturer branch. This is the migration plan's Phase 4 and the unblocker for Waves 4/5/6/9, which all consume `plan_poll_cycle`. No new vehicle behavior: the LLY's existing cadence, forced-standard-PID set, no-data/unsupported backoff, candidate-DID suppression, and `preferred_over` resolution are reproduced exactly, now as profile poll policy. The LLY golden corpus and the live poll order stay byte-identical (a poll-order parity test pins this).

### Depends on

- Wave 1 (`PollCadence`, `SignalDefinition.cadence`, `BackoffPolicy`, `FailurePolicy`, `preferred_over`, the `ModuleMap`/`RouteDefinition` the scheduler iterates).
- Wave 2 (`SelectedProfile`/`ProfileState`: the scheduler runs only for a selected profile; a no-profile context yields a generic-only plan).
- Wave 3 (`ProfileRuntime::execute_request`: the scheduler PLANS, the dispatcher EXECUTES each planned request).
- Hard ordering: MUST land before Waves 4, 5, 6, and 9 (all consume `plan_poll_cycle`). This is the wave that was previously an UNOWNED orphan dependency.

### Files touched

- CREATE `crates/obd2-dash/src/profiles/scheduler.rs` -- `ProfileRuntime::plan_poll_cycle`. Pure planning: no I/O, no manufacturer branch. Builds the ordered request list from cadence + backoff + candidate-suppression + `preferred_over`.
- ADD a `StandardPidPolicy` carrier to the profile (Wave 1 type, populated here for LLY): the forced-standard-PID set (the LLY Mode-01 bitmap workaround) becomes profile poll-policy data, NOT a global rule. A no-profile / unmatched vehicle uses the plain generic poll (no forced set) -- the deliberate behavior change already documented in migration-plan Phase 4.
- MODIFY `crates/obd2-dash/src/session_runner.rs` -- DELETE the global policy: `should_force_standard_poll` (`:131`, `:824`) and the hardcoded cadence (`cycle % 5/10/20/60` at `:207`/`:216`/`:217`/`:232`); route those decisions through `plan_poll_cycle`. Decrement the matching Wave 0 call-site allowlist bounds in the SAME commit.
- MODIFY `crates/obd2-dash/src/tui/ui.rs` (`:2403-2404`) and `crates/obd2-dash/src/main.rs` (`:322-343`) -- remove UI-local cadence/preference policy; the UI consumes the plan and never decides cadence.

### Exact APIs

```rust
pub struct PollPlan { pub requests: Vec<PlannedRequest> }   // ordered, ready for execute_request
pub struct PlannedRequest { pub capability: CapabilityId, pub route: RouteDefinition }

impl ProfileRuntime {
    // No manufacturer branch. Cadence / backoff / candidate-suppression / preferred_over only.
    pub fn plan_poll_cycle(&self, selected: &SelectedProfile, cycle: u64, coverage: &CoverageMap) -> PollPlan;
}

pub enum PollCadence { Fast, Medium, Slow, OnDemand }       // owned by Wave 1
pub struct StandardPidPolicy { pub forced: &'static [u8] }  // profile-owned; empty => generic-only
```

### Tests

- `plan_poll_cycle_reproduces_lly_order` -- with the LLY profile selected and a fixed `cycle` sequence, assert the planned standard-PID + enhanced + `$19` order is byte-identical to the pre-migration `session_runner` poll order (the parity pin; zero behavior change).
- `scheduler_has_no_manufacturer_branch` -- source scan: `scheduler.rs` does not import `gm_*`, has no `Manufacturer::Gm` match, and contains no DID literals.
- `generic_only_drops_forced_pids` -- a no-profile context yields a plan with NO forced-standard set (the deliberate Phase-4 behavior change) and generic OBD-II still reads.
- `backoff_and_candidate_suppression_parity` -- no-data/unsupported backoff and `Confidence::Candidate` suppression match the legacy `GmClass2Backoff`/candidate behavior.

### Acceptance criteria

- [ ] `scheduler.rs` + `plan_poll_cycle` exist; Waves 4/5/6/9 can consume them.
- [ ] `session_runner.rs` no longer owns cadence/forced-PID policy; the Wave 0 allowlist is decremented accordingly.
- [ ] LLY golden corpus zero-diff; the live poll-order parity test is green.
- [ ] A generic-only vehicle still reads generic OBD-II (with the documented loss of the LLY forced-PID set).
- [ ] `scheduler_has_no_manufacturer_branch` is green.

### Rollback notes

Revertible as a unit: restore the global cadence/forced-PID logic in `session_runner.rs` and delete `scheduler.rs`; the call-site allowlist returns to its prior counts. Because Waves 4/5 consume `plan_poll_cycle`, rolling this wave back requires rolling back 4/5 first (they sit above it in the graph). Independently shippable: `plan_poll_cycle` can run alongside the legacy path behind the existing gate until the legacy path is deleted in this wave's final commit.

---

## Wave 4: Migrate LLY Enhanced Reads

### Objective

Move the 24-entry GM LLY enhanced DID registry and its decode math behind the `gm.gmt800.lly.class2` profile as `SignalDefinition`s plus a profile-owned decoder, and route all GM enhanced polling through `ProfileRuntime::execute_request` so that `find_lly_did` is no longer reachable from generic enhanced execution. Decoded VGT / fuel-rail / MAP / injector output must be byte-for-byte and value-for-value identical to today; the LLY golden corpus is the proof and must stay green with exactly one intentional, reasoned diff (the 0x1940 `ecm -> tcm` module label, see below) and zero others.

### Depends on

This wave cannot land until the following are in place, because Wave 4 produces *profile data and a decoder* but owns no execution, scheduling, or selection machinery:

- **Wave 1 (Phase 1, neutral profile model).** Wave 4 expresses every LLY DID as `profiles::model::SignalDefinition` and returns them from `DiagnosticProfile::signals`. Without `SignalDefinition`, module-only `RouteDefinition`, `ModuleMap`/`ModuleDefinition`/`AddressTemplate`, `SourceFields` (and its `RxdSource`), `DecodedSignal`, `ProfileDecodeError`, `SignalCategory`, `PollCadence`, `Provenance`, `FailurePolicy`, the profile `Confidence`, and the `DiagnosticProfile` trait, there is nothing to migrate the data *into*. Wave 1 must also have created the `GmLlyClass2Profile` struct (id `gm.gmt800.lly.class2`) with `matches()` implemented; Wave 4 fills in only its `signals()` and `decode_signal()` bodies. CRITICAL: the exact field set of `DecodedSignal` and `SourceFields` is Wave 1's, not Wave 4's. Wave 4 conforms to Wave 1 additively and never introduces a new field on a Wave 1 type. In particular `DecodedSignal` MUST carry `raw: Vec<u8>` (the post-skip, echo-stripped bytes) in addition to `selected_raw`/`module`/`confidence` (invariant 8: preserve raw bytes for disputed manufacturer claims). If Wave 1's current `DecodedSignal` omits `raw`, or if its `SourceFields`/`RxdSource` shape differs from what Wave 4 needs, that is a Wave 1 model gap to be closed in Wave 1 before Wave 4 codes against it - it must NOT be silently patched in Wave 4.

- **Wave 2 (Phase 2, session-owned `SelectedProfile`).** Wave 4 deletes `append_dash_gm_targets` and its embedded LLY gate (`session_runner.rs:874`, one of the three scattered gates from inventory item 6). The replacement poll source is the session's single `SelectedProfile`. If Wave 2 has not collapsed `should_scan_gm_class2` / `append_dash_gm_targets` / `build_enhanced_targets` into one selection, removing one gate here re-introduces divergence. Wave 2 must also keep the `SelectedProfile` mint constructor at Wave 1's `pub(in crate::profiles)` visibility (do NOT widen to `pub(crate)`): registry.rs already lives inside `crate::profiles`, so `pub(crate)` would let `session_runner`/`app`/`gm_*` fabricate a `SelectedProfile` and defeat resolver-only minting. Wave 4 relies on that seal so that "no `SelectedProfile` => no GM read" is structurally true, not just convention.

- **Wave 3 (Phase 3, central dispatcher).** Wave 4 replaces the selector branch of `read_enhanced_target` (`session_runner.rs:398-423`, the `find_lly_did` call at `:417`) with `ProfileRuntime::execute_request`. Wave 3 must already own: `execute_request` (validates token + capability, resolves `ModuleMap + RouteDefinition -> ResolvedRoute`, calls the send path, dispatches to `decode_signal`, writes evidence) and the `ModuleMap` address resolution that composes J1850 headers from bus data. Wave 4 supplies correct `RouteDefinition { module }` values and the LLY `ModuleMap`; Wave 3 owns the mechanical resolution. If Wave 3 is incomplete, Wave 4 has no send path and must not be started.

- **Wave 3.5 (poll policy + scheduler) - OWNS `ProfileRuntime::plan_poll_cycle` and `profiles/scheduler.rs`.** `plan_poll_cycle` turns `selected.signals()` plus cadence (cycle % 5/10/20/60), forced-standard-PID policy (`should_force_standard_poll`), candidate-DID suppression (0x1542), and `preferred_over` into a request plan. Wave 4 must NOT invent `plan_poll_cycle` locally - doing so re-globalizes exactly the policy this migration is meant to move into profiles. The full poll-cycle test is required here and consumes Wave 3.5's scheduler.

Hard dependency note: Wave 3's dispatcher must reach the wire through a `Session` method that ends at `Session::send_request` (`session/mod.rs:1141`), not a bare `adapter.routed_request`, to preserve single-flight (`request_in_flight`) and the `BusNotAvailable` check (`resolve_module_address`, `session/mod.rs:1178`). The old selector path used `session.raw_request(0x22, data, Target::Module(module_id.0))` (`session_runner.rs:410`), which carried both guards. If Wave 3 dropped them, that is a Wave 3 defect Wave 4 inherits; flag it rather than papering over it.

Ownership note (cross-wave dedup): `read_enhanced_target` / the `find_lly_did` import (`:30`) / the `$19` append are each claimed by Waves 3, 4, and 5. This wave owns ONLY: deleting the `find_lly_did`/`LLY_ENHANCED_DIDS` import (`:29-30`), deleting the `read_enhanced_target` selector branch (`:398-423`), and deleting `append_dash_gm_targets` (`:873-898`) plus its call (`:868`). The `$19`/`$59` DTC append and the `should_force_standard_poll` policy are NOT Wave 4. Because Wave 0 froze EXACT call-site counts, Wave 4 must also DECREMENT the Wave 0 call-site allowlist for every site it removes (`find_lly_did` at `:417`, the append at `:868`/`:873-898`) in the same commit; removing a site without decrementing the allowlist fails the Wave 0 inventory test.

### Files touched

- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/gm_enhanced.rs`
  - Demote `find_lly_did` from `pub` to `pub(crate)` (it stays as the decoder backing lookup used only by the GM profile module and by `#[cfg(test)]`/examples; it must not be importable by `session_runner.rs`). Keep `LLY_ENHANCED_DIDS`, `LLY_REJECTED_DIDS`, `decode_did_value`, `selected_mode22_data`, `select_rxd_raw`, `apply_mth`, `GmDidDefinition`, `GmDecodeKind`, `GmRxd`, `GmMth`, and the `MTH_*`/`RXD_*` constants **unchanged byte-for-byte** -- these are the compiled decoder and changing any of them changes the golden output.
  - The existing tests `registry_contains_lly_foundation_entries` (`:892`), `rejected_dids_are_preserved` (`:905`), `decodes_full_positive_mode22_with_selector_byte` (`:928`), `decodes_adapter_stripped_mode22_payload` (`:939`), `decodes_rxd_16_bit_payload_and_signed_mth_offset` (`:948`), `rejects_mismatched_positive_did` (`:969`) stay in place and must keep passing.
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/lly.rs`
  - `pub const LLY_SIGNALS: &[SignalDefinition]` -- the 24-entry public projection (VGT 0x1543/0x1540, injector balance 0x162F-0x1636, fuel rail 0x163D/0x163E, desired MAP 0x1542, baro 0x1251, oil 0x1470, trans temp 0x1940 @ TCM, injector pulse width 0x1193-0x119A).
  - `pub const LLY_REJECTED_SIGNALS` (or reuse of `gm_enhanced::LLY_REJECTED_DIDS` projected to the model's rejected shape) -- 5 entries (0x1170, 0x1171, 0x1172, 0x1117, 0x119D), order preserved verbatim. These are negative knowledge; they MUST NOT appear in `signals()`.
  - `fn lly_backing(did: u16) -> Option<&'static gm_enhanced::GmDidDefinition>` -- module-private wrapper over `gm_enhanced::find_lly_did`, the only allowed call site.
  - The `DiagnosticProfile::signals()` and `DiagnosticProfile::decode_signal()` impl bodies for `GmLlyClass2Profile` (the struct itself is from Wave 1).
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/mod.rs` (created in Wave 1)
  - `mod lly; pub use lly::LLY_SIGNALS;` and wire `signals()`/`decode_signal()` to the `lly` module.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/session_runner.rs`
  - Remove the imports `find_lly_did` and `LLY_ENHANCED_DIDS` from the `use obd2_dash::gm_enhanced::{...}` block at `:29-30`, and decrement the Wave 0 allowlist accordingly.
  - Delete `append_dash_gm_targets` (`:873-898`) entirely; its LLY injection is replaced by the Phase-4 `ProfileRuntime::plan_poll_cycle` over the `SelectedProfile` (NOT a Wave-4-local re-implementation).
  - Delete the selector branch of `read_enhanced_target` (`:398-423`, including the `find_lly_did` call at `:417`). After migration no `EnhancedPollTarget` carries a selector, so that branch is dead; GM enhanced reads flow through `ProfileRuntime::execute_request`. Keep the non-selector branch (`session.read_enhanced(...)`, `:399-403`) untouched -- that path serves generic `VehicleSpec.enhanced_pids` and is out of scope for this wave.
  - In `build_enhanced_targets` (`:846`), drop the `append_dash_gm_targets(...)` call (`:868`); leave the spec-discovered generic enhanced loop as-is (its `loaded_lly_spec && !profile_matches_lly` gate at `:861` is a Wave 2 concern, not Wave 4 -- do not silently rewrite it here).
- **POPULATE** corpus entries for profile `gm.gmt800.lly.class2` under the **Wave 0-frozen corpus layout** (Wave 0 owns the directory layout, the `signal_key`+`module` schema, and the shared loader). Do NOT invent a Wave-4-local flat `tests/corpus/.../*.jsonl` layout -- that collides with Wave 0's frozen schema, and any divergence forces a rewrite of frozen files (a firewall violation). If Wave 0's frozen schema lacks `signal_key` or `module`, that is a Wave 0 gap to close in Wave 0 before Wave 4 freezes entries (see "Tests" for the required schema additions).
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/lly_profile_corpus.rs`
  - The golden replay harness, built on the Wave 0 shared loader (it does not define its own entry shape).
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/lly_enhanced_poll.rs`
  - Integration tests over **`Elm327Adapter` + `MockTransport::expect`** (NOT `MockAdapter`; see "Tests" for why).
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/profile_import_firewall.rs`
  - The architectural test forbidding live modules from importing `find_lly_did` / `LLY_ENHANCED_DIDS`.

No GUI files are touched in this wave. (The GUI `read_gm_did_value`/`read_enhanced_scalar` paths in `apps/obd2-gui/src-tauri/src/main.rs` are Wave 6; the inventory's CRITICAL OWL finding -- "the dash crate has no GUI `request_gm_node`" -- is consistent: the GUI lives in a separate crate and is migrated in its own wave. Do not chase a GUI GM path inside `crates/obd2-dash`.)

### Exact APIs

Types owned by Wave 1 (`profiles/model.rs`). These are NOT redefined in Wave 4: the canonical definition is Wave 1's. The shapes below are reproduced for reference and MUST match Wave 1's final shapes exactly. If any field here disagrees with Wave 1 -- notably `DecodedSignal.raw`, the `SourceFields` field set, the optionality of `txd`, and the `rxd`/`RxdSource` representation -- that is a Wave 1/Wave 4 model mismatch that MUST be fixed in Wave 1 (the model owner), not patched locally in Wave 4. These map the plan's Core Types onto concrete obd2-core types:

```rust
// profiles/model.rs (Wave 1) -- canonical; reproduced read-only
pub struct SignalDefinition {
    pub key: &'static str,            // e.g. "gm.lly.vgt_actual"
    pub label: &'static str,          // display, from GmDidDefinition.name
    pub category: SignalCategory,     // Turbo / FuelRail / Injector / Pressure / Transmission ...
    pub route: RouteDefinition,
    pub service_id: u8,               // 0x22 for every LLY DID
    pub request_data: &'static [u8],  // [did_hi, did_lo] ++ selector (e.g. &[0x15,0x43,0x01])
    pub decoder_id: &'static str,     // == key; used for evidence/recording audit + internal dispatch
    pub unit: &'static str,           // from GmDidDefinition.unit
    pub cadence: PollCadence,
    pub confidence: Confidence,       // PROFILE Confidence (see hazard 1), NOT obd2_core enhanced::Confidence
    pub provenance: &'static [Provenance],
    pub source_fields: SourceFields,  // TXD/RXF/RXD/raw MTH, inspectable
    pub evidence_policy: EvidencePolicy,
    pub failure_policy: FailurePolicy,
    pub preferred_over: Option<&'static str>, // 0x163E -> generic rail PID key; enforcement is Wave 5
}

// Wave-1-owned, module-only (shown for reference; Wave 4 does NOT redefine it).
// Bus + address live on the profile ModuleMap (ModuleDefinition), not on the route.
pub struct RouteDefinition {
    pub module: ModuleKey,            // ECM signals -> ModuleKey::Ecm; 0x1940 -> ModuleKey::Tcm
}

// Field set, optionality, and RxdSource shape are Wave 1's. Verify against profiles/model.rs
// before coding; the projection below mirrors GmDidDefinition.{txd,rxf,rxd,raw_mth}. For every LLY
// signal txd is always present in the backing def, so Wave 4 supplies it regardless of whether
// Wave 1 models it as Option. rxd is the Wave 1 type model::RxdSource (NOT a Wave-4 local type).
pub struct SourceFields {
    pub txd: <as defined in Wave 1>,        // GmDidDefinition.txd
    pub rxf: Option<&'static str>,          // GmDidDefinition.rxf
    pub rxd: Option<model::RxdSource>,      // { raw: &'static str, byte_index: usize, bit_width: u8 } (Wave 1)
    pub raw_mth: Option<&'static str>,      // GmDidDefinition.raw_mth
}

pub struct DecodedSignal {
    pub key: &'static str,
    pub value: f64,                   // MUST equal GmDecodedValue.value
    pub unit: &'static str,
    pub selected_raw: u32,            // from GmDecodedValue.selected_raw
    pub raw: Vec<u8>,                 // REQUIRED (invariant 8): post-skip, echo-stripped bytes exactly
                                      // as returned by routed_request; preserved for disputed
                                      // manufacturer claims. MUST NOT be dropped or recomputed.
    pub module: obd2_core::vehicle::ModuleId,
    pub confidence: Confidence,       // profile Confidence (5-variant), see hazard 1
}

pub enum ProfileDecodeError { /* maps from gm_enhanced::GmEnhancedDecodeError, see decode_signal */ }
```

New in Wave 4 (`profiles/gm/lly.rs`):

```rust
use crate::gm_enhanced::{self, GmDidDefinition, GmEnhancedDecodeError};
use crate::profiles::model::{DecodedSignal, ProfileDecodeError, SignalDefinition};

pub const LLY_SIGNALS: &[SignalDefinition]; // 24 entries, 1:1 with gm_enhanced::LLY_ENHANCED_DIDS

// Module-private; the ONLY caller of find_lly_did in non-test code after this wave.
fn lly_backing(did: u16) -> Option<&'static GmDidDefinition> {
    gm_enhanced::find_lly_did(did) // pub(crate)
}

// Profile decoder. Reuses the unchanged decode math so output cannot drift.
pub fn decode_lly_signal(
    signal: &SignalDefinition,
    payload: &[u8],
) -> Result<DecodedSignal, ProfileDecodeError> {
    let did = u16::from_be_bytes([signal.request_data[0], signal.request_data[1]]);
    let def = lly_backing(did).ok_or(ProfileDecodeError::UnknownSignal { key: signal.key })?;
    let decoded = gm_enhanced::decode_did_value(def, payload) // identical to legacy path
        .map_err(map_gm_decode_error)?;                       // GmEnhancedDecodeError -> ProfileDecodeError
    Ok(DecodedSignal {
        key: signal.key,
        value: decoded.value,
        unit: decoded.unit,
        selected_raw: decoded.selected_raw,
        raw: payload.to_vec(),        // invariant 8: preserve the post-skip bytes VERBATIM. Do not strip,
                                      // re-pad, or reconstruct - copy exactly what the dispatcher fed in.
        module: signal.route.module.to_core_module_id(),
        confidence: signal.confidence,
    })
}

fn map_gm_decode_error(e: GmEnhancedDecodeError) -> ProfileDecodeError; // total, no panics
```

`DiagnosticProfile` impl bodies for `GmLlyClass2Profile` (struct from Wave 1):

```rust
fn signals(&self) -> &[SignalDefinition] { LLY_SIGNALS }

fn decode_signal(&self, signal: &SignalDefinition, payload: &[u8])
    -> Result<DecodedSignal, ProfileDecodeError>
{
    lly::decode_lly_signal(signal, payload)
}
```

obd2-core / adapter signatures Wave 4 relies on (unchanged, from inventory):

```rust
// adapter/mod.rs
async fn routed_request(&mut self, req: &RoutedRequest) -> Result<Vec<u8>, Obd2Error>; // returns post-skip, echo-stripped payload
pub struct RoutedRequest { pub service_id: u8, pub data: Vec<u8>, pub target: PhysicalTarget }
// Elm327Adapter is the integration adapter: it formats ATSH <header> + the hex request onto the
// transport, so addressed J1850 actually reaches the wire. Tests drive it over MockTransport::expect.
// vehicle/mod.rs
pub enum PhysicalAddress { J1850 { node: u8, header: [u8; 3] }, /* ... */ } // #[non_exhaustive]
pub struct ModuleId(pub String); // consts ECM="ecm", TCM="tcm"
// session/mod.rs (Wave 3 routes through one of these)
async fn raw_request(&mut self, service: u8, data: &[u8], target: Target) -> Result<Vec<u8>, Obd2Error>;
```

Hard problems surfaced by these APIs:

1. **Confidence is a lossy collapse and MUST NOT be flattened to `obd2_core::protocol::enhanced::Confidence`.** `gm_enhanced::Confidence` is `{ Candidate, LiveObserved, Community, Verified, Rejected }` (5 variants). obd2-core's enhanced `Confidence` is `{ Verified, Community, Inferred, Unverified }` (4 variants, no `Candidate`/`Rejected`). The desired-MAP signal 0x1542 is `Confidence::Candidate` and the runtime candidate-suppression (Wave 5) keys off exactly that. If the profile `Confidence` reuses the obd2-core enum, `Candidate` becomes `Unverified` and the suppression semantics are silently lost. Wave 1 must have defined a profile-owned `Confidence` that preserves the GM 5-variant semantics. Wave 4 maps GM -> profile confidence 1:1; it must not map through the obd2-core enum. If Wave 1 did not provide this, Wave 4 is blocked -- raise it, do not approximate.

2. **`request_data` must embed DID bytes plus the selector verbatim.** Every LLY DID uses `SELECTOR_01 = &[0x01]`, so e.g. 0x1543 -> `&[0x15, 0x43, 0x01]`. The legacy live path built this in `read_enhanced_target` (`session_runner.rs:405-408`) as `[did_hi, did_lo] ++ selector`. Dropping the selector byte changes the wire request and the response, regressing every read. A careless implementer who sets `request_data = &[0x15, 0x43]` (DID only) will get different bytes on the wire.

3. **The decoder is defensive about adapter skip-bytes; do not "simplify" it.** For service 0x22 the ELM adapter uses skip=3 (`elm327.rs:310`), stripping `62 <did_hi> <did_lo>` before returning. `selected_mode22_data` (`gm_enhanced.rs:690`) then sees a payload that does *not* start with 0x62, so its echo-strip is a no-op and it strips only the selector. The unit tests `decodes_full_positive_mode22_with_selector_byte` (full `62 .. ` form) and `decodes_adapter_stripped_mode22_payload` (post-skip form) pin both shapes. `decode_signal` must call the unchanged `decode_did_value`, which handles both, so that whatever the dispatcher feeds it (post-skip payload via `routed_request`) decodes identically. The `MismatchedPositiveResponse` guard (`:697`) is exercised only when the un-stripped form reaches the decoder; preserve it for probe/corpus inputs.

4. **TCM 0x1940 must route to and be labeled `tcm`, and that flip MUST be corpus-visible.** The legacy `append_dash_gm_targets` hardcoded `module_label: "ecm"` (`session_runner.rs:890`) for every appended target, even 0x1940 whose backing `module` is `TCM` (node 0x18). Wave 4 sets `route.module = ModuleKey::Tcm` for 0x1940 and the LLY `ModuleMap` supplies TCM's J1850 node `0x18`; `DecodedSignal.module` follows from `route.module.to_core_module_id()`. This is a deliberate, plan-mandated change (Phase 5), and it is the ONE intentional golden diff allowed in this wave. OWL gap closed: Wave 0's signal goldens originally asserted only `selected_raw`/`value`/`unit` (no module), so this label flip would NOT have appeared as a golden diff and the "exactly one intentional diff with a written reason" firewall could not actually prove the correction. Therefore the corpus schema is extended (see "Tests") to carry `module` (and the resolved route/header), making the `ecm -> tcm` change a single, visible, reasoned diff. The decoded *value*/`selected_raw`/`raw` are unchanged. See "Tests" for the dedicated routing test and "Acceptance" for the snapshot-test fallout.

5. **`DecodedSignal.raw` is load-bearing (invariant 8) and is the easiest field to silently drop.** `routed_request` returns the post-skip, echo-stripped payload; `decode_lly_signal` must copy it into `DecodedSignal.raw` verbatim (`payload.to_vec()`), NOT reconstruct it from `selected_raw` and NOT re-strip it. This is what lets a disputed manufacturer claim be re-adjudicated against the actual bytes. A "tidy" implementer who keeps only `selected_raw` loses the dispute trail and breaks invariant 8 on a green value-only corpus -- which is exactly why the corpus now also round-trips `raw` (see "Tests").

### Tests

Unit (in `profiles/gm/lly.rs` and `gm_enhanced.rs`):

- `lly_signals_match_backing_registry` -- asserts `LLY_SIGNALS.len() == 24` and that for every `SignalDefinition` there is exactly one `gm_enhanced::find_lly_did(did)` whose `selector`, `module.node`, and `unit` equal the projection (DID parsed from `request_data[0..2]`). This is the firewall that keeps the public projection and the compiled decoder in lockstep; if someone edits one and not the other it fails.
- `lly_signals_exclude_rejected_dids` -- asserts none of `{0x1170, 0x1171, 0x1172, 0x1117, 0x119D}` appears in `LLY_SIGNALS`, and that the rejected set is still present and ordered exactly as `LLY_REJECTED_DIDS` (reuse/extend existing `rejected_dids_are_preserved`).
- `lly_signal_request_bytes` -- per signal, asserts `service_id == 0x22` and `request_data == [did_hi, did_lo, 0x01]` (selector preserved).
- `lly_tcm_signal_routes_to_tcm` -- 0x1940 has `route.module == ModuleKey::Tcm`; every other ECM signal has `route.module == ModuleKey::Ecm`. The LLY `ModuleMap` has TCM node `0x18` and ECM node `0x10`; the route itself carries no address.
- `lly_routes_resolve_to_expected_headers` (dedicated routing test mandated by OWL fix) -- resolves each signal route through Wave 3's `resolve_route(lly_module_map, signal.route, Protocol::J1850Vpw)` and asserts 0x1940 resolves to canonical module `"tcm"`, node `0x18`, header `[0x6C, 0x18, 0xF1]`, and every ECM signal resolves to module `"ecm"`, node `0x10`, header `[0x6C, 0x10, 0xF1]`. This is the test that proves the `ecm -> tcm` correction independently of the corpus.
- `decode_signal_parity_full_and_stripped` -- feeds both the full `62 19 40 01 ..` form and the post-skip `01 ..` form to `decode_lly_signal` and asserts the same `value`/`selected_raw` as `decode_did_value`, and that `DecodedSignal.raw == payload` for both forms (reuses the byte vectors from `decodes_full_positive_mode22_with_selector_byte` / `decodes_adapter_stripped_mode22_payload`). The `raw == payload` assertion catches a dropped/recomputed `raw` (hazard 5).
- `decode_signal_error_mapping` -- mismatched positive DID, payload-too-short, and zero-divisor produce the correct `ProfileDecodeError` variants (no panic), mirroring `rejects_mismatched_positive_did`.
- `source_fields_preserved` -- for 0x163E asserts `source_fields.rxd` is `Some` with `raw == "3008"` and the range-suspect/RXD caveat survives the projection (the plan's named regression at lines 226, 682: do not lose the fuel-rail RXD=3008 caveat). Field access follows Wave 1's `RxdSource` shape.

Integration (`tests/lly_enhanced_poll.rs`) -- **`Elm327Adapter` + `MockTransport::expect`, NOT `MockAdapter`.** OWL fix: `MockAdapter::routed_request` (`mock.rs:300-313`) downgrades `Addressed -> Broadcast` and returns canned `[0x80,0x00]` for 0x22, so an LLY read through `MockAdapter` either fakes a regression or requires an obd2-core change that contradicts the "obd2-core unchanged" acceptance. `Elm327Adapter` over `MockTransport::expect` formats the real `ATSH <header>` + hex request onto the transport, so the addressed J1850 header is genuinely exercised. This is the same harness Wave 9 standardizes on; all integration waves use it.

- `gm_lly_reads_through_execute_request_publish_expected_values` (headline; runnable on Wave 1+2+3, NO `plan_poll_cycle`) -- builds an `Elm327Adapter` over a `MockTransport::expect` programmed with the captured `62 15 43 ..` etc. response frames (and the expected request frames including `ATSH`), an LLY-matching `VehicleContext`/`SelectedProfile`, drives `ProfileRuntime::execute_request` per signal, and asserts the emitted `Message::EnhancedPidUpdate` values/`selected_raw` equal the legacy values.
- `gm_lly_tcm_routes_with_tcm_header_and_label` -- asserts the transport saw `ATSH6C18F1` (header `[0x6C,0x18,0xF1]`) for the 0x1940 request and that `EnhancedPidUpdate.module == "tcm"` (the bug fix), value unchanged. Sidesteps the `MockAdapter` Addressed downgrade entirely because the header is asserted as literal bytes on the transport.
- `no_selected_profile_issues_no_addressed_request` -- with a non-matching context (no `SelectedProfile`), `execute_request` is unreachable and is refused; assert `MockTransport` saw no `ATSH6C..` + 0x22 frame (invariants 3/4). Also assert a Partial-matched profile context yields no `SelectedProfile` and that any attempted routed request is rejected (invariant 5 end-to-end: visible-but-cannot-poll).
- `full_poll_cycle_through_plan_poll_cycle` -- consumes Wave 3.5's `ProfileRuntime::plan_poll_cycle`; this is a required test, not ignored. It proves the enhanced-read plan uses the migrated profile-owned cadence/forced-PID/preference policy instead of a Wave-4-local scheduler.

Golden-corpus (`tests/lly_profile_corpus.rs` over the Wave 0-frozen layout for `gm.gmt800.lly.class2`, via the Wave 0 shared loader):

- Entry shape (Wave 0-owned schema; the OWL fix adds `module`/`node`/`header` so the route change is visible): `{ "signal_key", "service_id", "request_data": [..], "response_payload": [..], "expected": { "value": f64, "unit": "..", "selected_raw": u32, "module": "ecm"|"tcm", "node": u8, "header": [u8;3] } | { "error": "VariantName" } }`. `response_payload` is the decoder-input form (post-skip), so the profile golden isolates Layer 3; the adapter skip is proven separately by the obd2-core protocol corpus. The seeder derives `response_payload` by running each captured raw `R 62 ..` line through `decode_elm_response_payload_for_command(.., BusFamily::J1850, skip=3, echo)` and freezes the result. NOTE: if Wave 0 froze a schema lacking `signal_key`/`module`, that schema must be amended in Wave 0 first; Wave 4 must not fork a parallel layout.
- `corpus_decodes_byte_and_value_identical` -- replays every entry through `decode_lly_signal` and asserts exact `value`/`selected_raw`/`unit`, asserts `DecodedSignal.raw == response_payload` (catches a dropped `raw`, hazard 5), and asserts `module` equals `DecodedSignal.module`. The `node`/`header` are cross-checked by resolving `signal.route` through the LLY `ModuleMap` with Wave 3's resolver. Rule: no existing entry's expected output may change, with EXACTLY ONE exception -- the 0x1940 entry's `module` flips `ecm -> tcm` (and `node 0x10 -> 0x18`, `header [0x6C,0x10,0xF1] -> [0x6C,0x18,0xF1]`), accompanied by a written reason "Phase 5 TCM identity fix". That is the single intentional diff; any other diff fails the wave.
- Seed sources (rec inventory section D, with its warnings): real positives exist for **0x1540, 0x1543, 0x162F** (and confirm 0x1542 before pinning) in `/Users/jared/Projects/HaulLogic/obd2-dash/raw-captures/1GTHK29294E391526-...-20260627T032948.obd2raw` (primary, 707 `62` responses) and the secondary captures listed. For **0x1940 (trans temp), 0x1470 (oil), the other injector-balance/pulse-width DIDs** there are NO observed positives -- seed those from **synthetic fixtures** (the byte vectors already in the `gm_enhanced.rs` unit tests), and label them `"source": "synthetic"`. Do NOT claim capture coverage for signals the captures never answered. There is NO `$19`/`$59` traffic in any capture, but DTC services are Wave 5, so no DTC golden is in scope here.
- `corpus_covers_every_polled_signal` -- asserts every `key` in `LLY_SIGNALS` has at least one corpus entry (real or synthetic), so a future edit cannot add a signal with no golden.

Architectural (`tests/profile_import_firewall.rs`):

- `live_modules_do_not_import_find_lly_did` -- scans `src/session_runner.rs` (and any non-`profiles`, non-`#[cfg(test)]` live module) source text and fails if it references `find_lly_did` or `LLY_ENHANCED_DIDS`. This is the Wave 4 slice of the plan's architectural import test (Phase 3, lines 345-346, 826, 831). Pair it with `find_lly_did` being `pub(crate)` so an external import is also a compile error.
- `selector_decode_does_not_call_find_lly_did` -- a compile-time/grep assertion that no selector-based execution path calls `find_lly_did` directly (plan "Decoder isolation", line 831). After Wave 4 the only caller is `profiles::gm::lly::lly_backing`.

### Acceptance criteria

- [ ] **LLY golden corpus stays green with exactly one intentional diff.** Every `gm.gmt800.lly.class2` corpus entry decodes value-for-value and byte-for-byte identical through `decode_lly_signal` (`value`/`selected_raw`/`unit`/`raw`); the ONLY changed expected field anywhere is the 0x1940 entry's `module`/`node`/`header` (`ecm -> tcm`, node `0x10 -> 0x18`, header `[0x6C,0x10,0xF1] -> [0x6C,0x18,0xF1]`), with the written reason "Phase 5 TCM identity fix". No other expected output is edited.
- [ ] All pre-existing `gm_enhanced.rs` unit tests still pass unchanged (`registry_contains_lly_foundation_entries`, `rejected_dids_are_preserved`, both `decodes_*` parity tests, `rejects_mismatched_positive_did`, the RXD-16/signed-MTH test).
- [ ] `LLY_SIGNALS.len() == 24` and `lly_signals_match_backing_registry` passes (public projection is 1:1 with the compiled decoder).
- [ ] `DecodedSignal` carries `raw` (post-skip bytes) AND `selected_raw`/`module`/`confidence`; `decode_lly_signal` sets `raw == payload.to_vec()` and `corpus_decodes_byte_and_value_identical` asserts `raw == response_payload` (invariant 8 holds).
- [ ] Rejected DIDs `{0x1170,0x1171,0x1172,0x1117,0x119D}` are preserved in order and never appear in `signals()`; nothing polls them.
- [ ] `find_lly_did` is `pub(crate)`; `session_runner.rs` no longer imports `find_lly_did` or `LLY_ENHANCED_DIDS`; `append_dash_gm_targets` and the `read_enhanced_target` selector branch are deleted; the Wave 0 call-site allowlist is decremented for the removed sites in the same commit; `live_modules_do_not_import_find_lly_did` passes.
- [ ] GM enhanced reads flow only through `ProfileRuntime::execute_request`; with no `SelectedProfile` (including a Partial match), zero addressed `0x22` requests are issued and the dispatcher refuses any attempted routed request (invariants 3, 4, 5).
- [ ] TCM signal 0x1940 routes to module `"tcm"`, node `0x18`, header `[0x6C,0x18,0xF1]` -- proven three ways: `lly_routes_resolve_to_expected_headers`, the corpus `module`/`node`/`header` diff, and `gm_lly_tcm_routes_with_tcm_header_and_label` (ATSH on the transport). **Behavior-change gate:** any existing TUI snapshot/UI test that pinned `module == "ecm"` for 0x1940 is updated in the SAME commit with a one-line note "Phase 5 module-label fix"; the decoded *value* is unchanged.
- [ ] `source_fields` for every signal carries TXD/RXF/RXD/raw-MTH; the 0x163E RXD=3008 range-suspect caveat is inspectable (`source_fields_preserved` passes).
- [ ] `preferred_over` is set on 0x163E (-> the generic rail PID key) and `failure_policy` mirrors the GM value for every signal; no preference *enforcement* logic is added in this wave (that is Wave 5) -- verified by the diff touching no poll-skip/forced-PID code.
- [ ] Confidence is mapped without collapsing through `obd2_core::protocol::enhanced::Confidence`; 0x1542 remains `Candidate`.
- [ ] Integration tests run on `Elm327Adapter` + `MockTransport::expect` (no dependency on a `MockAdapter` Addressed capability); `cargo test -p obd2-dash` and `cargo test -p obd2-core` are green with obd2-core UNCHANGED. `full_poll_cycle_through_plan_poll_cycle` is enabled and consumes Wave 3.5's scheduler.

### Rollback notes

- **Feature-flag the rewire, not the data.** Gate the `session_runner.rs` switch with a cargo feature, e.g. `profile-enhanced-reads` (default on once green). When off, `build_enhanced_targets` keeps calling `append_dash_gm_targets` and `read_enhanced_target` keeps the `find_lly_did` selector branch (the legacy path). When on, GM enhanced polling runs through `ProfileRuntime`. This lets the two paths coexist and lets a regression be reverted by flipping one flag rather than reverting a multi-file diff. Keep both paths until the golden corpus is green in CI. NOTE: while the flag is off, the legacy `append_dash_gm_targets` still hardcodes `module_label = "ecm"` for 0x1940, so the corpus 0x1940 entry must reflect whichever path the build exercises -- do not let the flag states disagree with the frozen corpus.
- **`profiles/gm/lly.rs` is purely additive and independently shippable.** It compiles and its unit tests pass without the `session_runner.rs` rewire, because `decode_lly_signal` just reuses `gm_enhanced::decode_did_value`. You can land the data + decoder + corpus first (firewall in place), then land the rewire behind the flag in a second commit. The corpus, `lly_signals_match_backing_registry`, and `lly_routes_resolve_to_expected_headers` guard the data + routing half even if the rewire is deferred.
- **Reverting the rewire is safe and total.** Re-add the two imports at `session_runner.rs:29-30` (and restore the Wave 0 allowlist counts), restore `append_dash_gm_targets` and the `read_enhanced_target` selector branch from git, and re-promote `find_lly_did` to `pub` (or just keep it `pub(crate)` and revert the import test). No obd2-core change is required by this wave (the integration harness is `Elm327Adapter` + `MockTransport`), so there is nothing to roll back in the core crate.
- **The one irreversible-by-intent change is the 0x1940 `ecm -> tcm` label.** If you must ship the rewire but defer the label fix, pin 0x1940's `route.module` to `"ecm"` temporarily, keep the corpus 0x1940 `module`/`node`/`header` at the legacy `ecm`/`0x10`/`[0x6C,0x10,0xF1]` values, and add a `// TODO(Phase 5): TCM identity` marker plus a failing-ignored `lly_routes_resolve_to_expected_headers` case, so the divergence is visible rather than silently re-buried. Do not leave it labeled `ecm` without a tracked marker -- that is exactly the "preserve current behavior preserves a bug" trap (inventory hazard 3).
- **Out of scope, do not let it leak in:** authoring `ProfileRuntime::plan_poll_cycle`/`profiles/scheduler.rs` (Wave 3.5 owns that); the global `should_force_standard_poll` and `is_stale_pid_response_error` (`session_runner.rs:824`, `:318`) and their pinned tests (`:1037-1112`) are consumed only through Wave 3.5's migrated policy; GM `$19` DTC services belong to Wave 5; recording `write_enhanced(module="ecm")` (which will start recording `"tcm"` for 0x1940 once the label is fixed) belongs to Wave 7. Touching any of these in Wave 4 widens the blast radius and breaks waves' independent shippability.

All references confirmed: `MockTransport::expect(command, response)` (transport/mock.rs:29), `Elm327Adapter::new(Box<dyn Transport>)` (elm327.rs:43), and the `cycle % 60` gate is passed into `poll_dtcs` as `include_gm_class2` (computed at :217, consumed at :432). Here is the corrected section.

---

## Wave 5: Migrate GM DTC Services

### Objective

Move the GM Class 2 `$19 FF FF 00` (all-status) and `$19 92 FF 00` (Tech2 active/history) DTC scans out of the hardcoded session loop and into the profile as two `DtcServiceDefinition`s owned by `gm.gmt800.lly.class2`, decoded through the unchanged `gm_class2.rs` decoder and executed only through `ProfileRuntime`. Generic SAE `03`/`07`/`0A` stay in the generic path untouched, and the MODULE SCAN UI states and per-module routing/labels must be byte-for-byte identical to today. Wave 5 is the SOLE owner of the `$19` send path (see Depends on / Wave 3): the `$19` request construction, the `GmClass2Backoff` 3-skip suppression, and the module fan-out all migrate together in this one wave so the backoff is never dropped between waves.

### Depends on

- **Wave 1 (neutral profile model)** - hard dependency, and this wave forces three Wave-1 final-model decisions that MUST land in Wave 1, not here:
  1. **`RouteSet`/`RouteScope` shape.** Wave 5 needs `RouteSet { scope: RouteScope }` with `RouteScope::DiscoveredOnBus { bus: BusKey }` to reproduce `dtc_scan_modules` for `$19` coverage. The original Wave-1 draft modeled `RouteSet` as `{ routes: &[RouteDefinition] }`. That is a Wave-1 model change; Wave 5 must NOT silently redefine it (a second definition in `profiles/model.rs` would conflict with Wave 9's `RouteSet::single` and fail to compile). Wave 1's final model must own `RouteSet`, `RouteScope` (`Single`, `Explicit`, and `DiscoveredOnBus`), and the `RouteSet::single` helper used by Wave 9.
  2. **`ProfileDecodeError` variant set.** Unified across Waves 1/5/9 in Wave 1's final model, including `Decode(String)` and `NegativeResponse { service, nrc }`. Wave 5 maps normal `GmClass2DecodeError` failures to `ProfileDecodeError::Decode(e.to_string())` and maps a leading `7F 19 <nrc>` directly to `ProfileDecodeError::NegativeResponse { service: 0x19, nrc }`. Do NOT introduce Wave-5-local error variants.
  3. The `DiagnosticProfile` trait, `ProfileId`, and the `dtc_services()` / `decode_dtc_response(...)` accessors must exist (stubbed is fine; Wave 5 fills them).
- **Wave 2 (session-owned profile selection)** - hard dependency. The `$19` gate today is `should_scan_gm_class2` = `selected_protocol == J1850Vpw && session_matches_lly_profile` (`session_runner.rs:622`). That local re-decision must already be replaced by a single session-owned `SelectedProfile`/`ProfileState`; otherwise this wave reintroduces a scattered gate (inventory hazard #2, plan invariant #1, Owl finding "TUI and GUI have separate GM paths"). `SelectedProfile` is resolver-minted only; Wave 5 never fabricates one.
- **Wave 3 (central dispatcher + route resolution + probe-only quarantine)** - hard dependency, with a HARD OWNERSHIP BOUNDARY. `ProfileRuntime::execute_request` must exist as the single send path, and the architectural import test must already be in place so this wave can assert `session_runner.rs` no longer constructs `$19`. Route resolution (`RouteDefinition`/`AddressTemplate` -> `PhysicalAddress`) must exist, BUT see the routing decision in Exact APIs: the LLY `$19` path must preserve `Target::Module` resolution, not switch to template-synthesized `6C <node> F1` headers. **Wave 3 MUST leave `append_gm_class2_dtcs`, `should_scan_gm_class2`, `gm_class2_scan_modules`, and `GmClass2Backoff` (`session_runner.rs:62`-`:71`, `:491`, `:622`, `:629`, `:633`-`:666`) completely untouched.** Wave 3 owns the generic dispatcher; Wave 5 owns the entire `$19` path. If both edit the `$19` send, the only mechanism preserving `$19` cadence/NoData suppression - the 3-skip `GmClass2Backoff` - can be silently dropped between waves. This wave is the single migration that removes the legacy `$19` send AND lands its replacement backoff in one atomic change.
- **Wave 4 (GM LLY signal migration) - NOT a blocker, sibling.** DTC services are an independent capability. `session_matches_lly_profile` (`session_runner.rs:900`) is shared with the signal path (`append_dash_gm_targets`, `build_enhanced_targets`); Wave 5 removes only the `$19`-specific gate (`should_scan_gm_class2`, `gm_class2_scan_modules`) and leaves the signal-path callers (including the `find_lly_did` import at `session_runner.rs:30`/`:417`) to Wave 4. Wave 5 can land before or after Wave 4. Do not touch `read_enhanced_target` or `find_lly_did` here; that rewire is Wave 4's alone.

**session_runner rewire ownership (cross-wave):** the `$19` send (`append_gm_class2_dtcs`) is Wave 5's exclusive territory; `read_enhanced_target` / `find_lly_did` (`:30`/`:417`) is Wave 4's; the generic dispatcher is Wave 3's. Because Wave 0 froze EXACT call-site counts, Wave 5 MUST also decrement the Wave 0 call-site allowlist for every `$19`/`gm_class2` site it removes (see Acceptance) - removing sites without updating the frozen allowlist fails the Wave 0 inventory test.

### Files touched

- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/model.rs` - add `DtcServiceDefinition`, `BackoffPolicy`, `BackoffClass`, `DtcServiceClassification`, and `DecodedDtc` (only if Wave 1/3 did not already define it). Add `fn dtc_services(&self) -> &[DtcServiceDefinition]` and `fn decode_dtc_response(...)` to the `DiagnosticProfile` trait if Wave 1 only stubbed them. **Do NOT define `RouteSet`/`RouteScope`/`RouteSet::single` or `ProfileDecodeError` here** - those are Wave 1's final-model types; Wave 5 only consumes them.
- **CREATE/EXTEND** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/class2.rs` - the two `$19` `DtcServiceDefinition`s as profile data plus `decode_class2_dtcs_checked(payload) -> Result<Vec<DecodedDtc>, ProfileDecodeError>`, a thin wrapper over the existing `decode_class2_dtcs` + `GmClass2DtcRecord::into_dtc`. No decode logic is rewritten EXCEPT one mandatory guard: the wrapper MUST detect a leading `0x7F` (a `7F 19 <nrc>` negative response) and return `ProfileDecodeError::NegativeResponse { service: 0x19, nrc }` BEFORE calling `decode_class2_dtcs`. Without it, `decode_class2_dtcs(&[0x7F, 0x19, nrc])` parses those 3 bytes as a phantom DTC triplet (`0x7F19` -> a fabricated `U..` code), because golden-corpus replay feeds payloads straight to the decoder and bypasses the adapter's NRC parsing (module-arch doc, verified leading-`0x7F` bug).
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/lly.rs` (or `gm/mod.rs`) - register the two `DtcServiceDefinition`s on the LLY profile and wire `decode_dtc_response` to dispatch `decoder_id == "gm.class2.dtc"` to `decode_class2_dtcs_checked`.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/runtime.rs` - add `execute_dtc_services(...)`: plan per-module `$19` requests from the selected profile's `dtc_services()`, send through the session module-resolution path, decode via `decoder_id`, classify, apply per-(module,service) backoff, and stamp `source_module` from the resolved discovered `ModuleId`. This is where `GmClass2Backoff`'s replacement (the `BackoffPolicy`-driven per-(module,service) cache) now lives.
- **MODIFY** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/session_runner.rs` - in `poll_dtcs` (`:425`), replace the `append_gm_class2_dtcs` call (`:433`) with a call to `ProfileRuntime::execute_dtc_services` keyed on `ProfileState`, **kept behind the existing `include_gm_class2` cadence gate** (computed as `cycle % 60 == 0` at `:217`, passed into `poll_dtcs`; see Exact APIs / cadence). Map each `DtcServiceOutcome` to a `DiagnosticScanEntry` carrying the unchanged domain `DtcService::GmClass2All`/`GmClass2Active`. DELETE (or feature-gate, see Rollback) `append_gm_class2_dtcs` (`:491`), `should_scan_gm_class2` (`:622`), `gm_class2_scan_modules` (`:629`), `GmClass2Backoff`/`GmClass2BackoffEntry` (`:62`-`:71`), and the `cached_result`/`observe` impls (`:633`-`:666`). Remove the `gm_class2` import block (`:25`-`:28`). **Decrement the Wave 0 frozen call-site allowlist** for each removed `$19`/`gm_class2` reference in the same commit.
- **KEEP UNCHANGED** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/gm_class2.rs` - the decoder, `GmClass2Status`, `into_dtc`, `GmClass2DecodeError`, and the request constants stay. `class2_header`/`class2_routed_request` (`:75`,`:79`) become referenced only by probe examples and the new profile data; live `session_runner.rs` no longer touches them. The known Layer-1/Layer-3 framing violation (inventory item 4) is NOT fixed here; defer.
- **KEEP UNCHANGED** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/domain.rs` - `DtcService` enum (`:64`, with `GmClass2All`/`GmClass2Active`, `service_id()`, `label()` returning `"19ff"`/`"1992"`), `DiagnosticScanEntry`, `DiagnosticScanResult`, `DiagnosticScanScope` all stay. Generalizing this GM-coupled enum is explicitly deferred to a later UI wave to guarantee zero UI diff (inventory item 8).
- **KEEP UNCHANGED** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/widget/renderers.rs` - `render_diagnostic_scan_panel` (`:1187`), `count_diagnostic_scan_targets` (`:1286`), `diagnostic_scan_result_label`/`_color` (`:1266`/`:1276`), and the MODULE SCAN title (`:238`-`:245`) are not edited.
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/` (directory does not exist yet) - `dtc_service_dispatch.rs` integration tests and `corpus_dtc.rs` golden replay.
- **CREATE** `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus/profile/gm.gmt800.lly.class2/dtc-*.jsonl` - synthetic `$19` decode goldens (see Tests; there is no real `$19` traffic to seed from). These files MUST conform to the single pinned corpus schema and be read through the shared corpus loader (record carries `signal_key` = the DTC service key, e.g. `"gm.class2.dtc.all"`, plus `module`), NOT a Wave-5-bespoke flat layout. If Wave 0 froze a divergent schema/layout, reconcile against the pinned schema before adding these files (do not introduce a second loader).

### Exact APIs

New profile-model types added by THIS wave (Layer 3 data; no transport framing). These reference obd2-core types verbatim: `obd2_core::protocol::dtc::{Dtc, DtcStatus}`, `obd2_core::vehicle::ModuleId`, `obd2_core::protocol::service::Target`, `obd2_core::error::{Obd2Error, NegativeResponse}`.

```rust
// profiles/model.rs (added by Wave 5)

/// Profile-owned DTC service. Mirrors the plan's DtcServiceDefinition.
pub struct DtcServiceDefinition {
    pub key: &'static str,            // capability id, e.g. "gm.class2.dtc.all"
    pub label: &'static str,          // human label for evidence/docs
    pub route_set: RouteSet,          // type owned by Wave 1's final model
    pub service_id: u8,               // 0x19 for GM Class 2
    pub request_data: &'static [u8],  // must equal CLASS2_DTC_ALL_REQUEST / _ACTIVE_REQUEST
    pub decoder_id: &'static str,     // "gm.class2.dtc"
    pub backoff_policy: BackoffPolicy,
    pub cadence: PollCadence,         // ADVISORY metadata only; see cadence note below
}

/// NoData/Unsupported suppression. LLY value: skip_count = 3.
pub struct BackoffPolicy {
    pub skip_count: u8,
    pub suppress: &'static [BackoffClass], // [NoData, Unsupported]
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BackoffClass { NoData, Unsupported }
```

`RouteSet`/`RouteScope` are NOT defined by Wave 5 - they are owned by Wave 1's final model. Reproduced here for reference only (a duplicate definition in `profiles/model.rs` will collide with Wave 1 and with Wave 9's `RouteSet::single`):

```rust
// profiles/model.rs - OWNED BY WAVE 1 (shown for reference; do not redefine here)

pub struct RouteSet {
    pub scope: RouteScope,
}

pub enum RouteScope {
    /// ZERO-DIFF default for LLY $19: every discovered module on the named bus.
    /// Runtime intersects with live discovery; the profile hardcodes no node list.
    /// This reproduces session_runner::dtc_scan_modules exactly.
    DiscoveredOnBus { bus: BusKey },
    /// Explicit static routes (e.g. a fixed Class 2 node list). Each route resolves
    /// through session module resolution; modules absent from discovery are skipped.
    /// NOT used by LLY $19 (would change module coverage -> regression).
    Explicit(&'static [RouteDefinition]),
}

impl RouteSet {
    // Used by Wave 9. IMPLEMENTER WARNING for Wave 1: RouteScope::Explicit holds
    pub const fn single(route: RouteDefinition) -> RouteSet;
    pub const fn explicit(routes: &'static [RouteDefinition]) -> RouteSet;
    pub const fn discovered_on_bus(bus: BusKey) -> RouteSet;
}
```

Decoder wrapper (Layer 3; reuses the unchanged `gm_class2.rs` functions). `DecodedDtc` is the Wave 1/3 decoded type; the only field this wave relies on is `pub dtc: obd2_core::protocol::dtc::Dtc`. The error maps into Wave 1's unified `ProfileDecodeError::Decode(String)` variant:

```rust
// profiles/gm/class2.rs
use obd2_dash::gm_class2::{decode_class2_dtcs, GmClass2DecodeError};
use crate::profiles::model::ProfileDecodeError; // Wave 1 final: { Decode(String), Other(String) }

pub fn decode_class2_dtcs_checked(payload: &[u8]) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
    if payload.first() == Some(&0x7F) {
        let service = payload.get(1).copied().unwrap_or(0x19);
        let nrc = payload.get(2).copied().unwrap_or(0);
        return Err(ProfileDecodeError::NegativeResponse { service, nrc });
    }
    let records = decode_class2_dtcs(payload)
        .map_err(|e: GmClass2DecodeError| ProfileDecodeError::Decode(e.to_string()))?;
    // into_dtc(None) sets status + the exact "GM Class 2 status 0x..: flags" notes.
    // The runtime stamps source_module afterward from the resolved discovered ModuleId.
    Ok(records.into_iter().map(|r| DecodedDtc { dtc: r.into_dtc(None) }).collect())
}
```

Runtime entry point (Layer 2; the single execution path). It must route through obd2-core `Session` so it keeps `resolve_module_address`'s `BusNotAvailable` guard, `record_visible_target`, and the single-flight `request_in_flight` guard (core inventory failure mode #3). It calls `Session::raw_request(service_id, data, Target::Module(id))` (`session/mod.rs:1020`) - permitted only because the call lives inside the runtime, satisfying the Wave 3 architectural test. The per-(module,service) backoff cache (the replacement for `GmClass2Backoff`) lives on `ProfileRuntime`:

```rust
// profiles/runtime.rs
pub struct DtcServiceOutcome {
    pub service_key: &'static str, // maps to domain DtcService in session_runner
    pub module: ModuleId,          // the discovered module routed to
    pub decoded: Vec<DecodedDtc>,  // source_module already stamped to `module`
    pub classification: DtcServiceClassification,
}

/// Layer-2 mirror of domain::DiagnosticScanResult (1:1; mapped in session_runner).
pub enum DtcServiceClassification {
    Codes(usize),
    Empty,
    NoData,
    Unsupported(String),
    Error(String),
}

impl ProfileRuntime {
    pub async fn execute_dtc_services<A: Adapter>(
        &mut self,                       // &mut: owns the per-(module,service) backoff cache
        session: &mut Session<A>,
        selected: &SelectedProfile,      // generation-validated by the dispatcher
    ) -> Vec<DtcServiceOutcome>;
}
```

Classification mapping that MUST be preserved byte-for-byte from the current `append_gm_class2_dtcs` (`session_runner.rs:519`-`:543`):

| Send result | `DtcServiceClassification` |
|---|---|
| `Ok(bytes)` + `decode_class2_dtcs_checked` Ok, 0 records | `Empty` |
| `Ok(bytes)` + decode Ok, n records | `Codes(n)` |
| `Ok(bytes)` + decode `Err(e)` | `Error(e.to_string())` (forward `GmClass2DecodeError` Display verbatim) |
| `Err(Obd2Error::NoData)` | `NoData` |
| `Err(NegativeResponse { nrc: ServiceNotSupported })` | `Unsupported(nrc.to_string())` |
| `Err(NegativeResponse { nrc: SubFunctionNotSupported })` | `Unsupported(nrc.to_string())` |
| any other `Err(e)` | `Error(e.to_string())` |

Note the asymmetry to preserve: the `Err(decode)` row maps to `Error(...)` carrying the `GmClass2DecodeError` Display string, whereas `decode_class2_dtcs_checked` returns `ProfileDecodeError::Decode(...)` wrapping that same string. `execute_dtc_services` must unwrap the `ProfileDecodeError::Decode(s)` back to `Error(s)` so the published string is identical to today (do not let the `ProfileDecodeError::` prefix or `Debug` leak into the UI string).

session_runner glue (the integration boundary). The cadence gate stays in `session_runner`; Wave 5 does NOT move it into the profile:

```rust
// session_runner.rs, inside poll_dtcs after scan_standard_dtcs.
//
// CADENCE: `include_gm_class2` is computed by the caller as `cycle % 60 == 0`
// (session_runner.rs:217) and passed into poll_dtcs. PollCadence (Fast/Medium/Slow/
// OnDemand) CANNOT express cycle%60, so the modulo gate must remain here until the
// Phase-4 scheduler exists. Do NOT delete this gate on the assumption that
// PollCadence::Slow reproduces it - it does not.
if include_gm_class2 {
    if let Some(selected) = profile_state.selected_dtc_capable() {
        for outcome in runtime.execute_dtc_services(session, selected).await {
            let service = match outcome.service_key {
                "gm.class2.dtc.all"    => DtcService::GmClass2All,
                "gm.class2.dtc.active" => DtcService::GmClass2Active,
                _ => continue,
            };
            scan.dtcs.extend(outcome.decoded.into_iter().map(|d| d.dtc)); // source_module preset
            scan.entries.push(DiagnosticScanEntry {
                scope: DiagnosticScanScope::Module(outcome.module.0),
                service,
                result: outcome.classification.into(), // From<DtcServiceClassification>
            });
        }
    }
}
// enrich_dtcs + dedup_dtcs run unchanged afterward (:435-:436)
```

LLY profile data (the two services). `request_data` MUST be the same byte slices as the constants in `gm_class2.rs` (`CLASS2_DTC_ALL_REQUEST = [0xFF,0xFF,0x00]` `:13`, `CLASS2_DTC_ACTIVE_REQUEST = [0x92,0xFF,0x00]` `:14`):

```rust
const LLY_DTC_ALL: DtcServiceDefinition = DtcServiceDefinition {
    key: "gm.class2.dtc.all",
    label: "GM Class 2 DTC (all status)",
    route_set: RouteSet::discovered_on_bus(BusKey::new("j1850vpw")),
    service_id: 0x19,                                   // SERVICE_REPORT_DTCS_BY_STATUS
    request_data: &[0xFF, 0xFF, 0x00],
    decoder_id: "gm.class2.dtc",
    backoff_policy: BackoffPolicy { skip_count: 3, suppress: &[BackoffClass::NoData, BackoffClass::Unsupported] },
    // ADVISORY ONLY. This does NOT produce the cycle%60 gate; the gate is in
    // session_runner (:217). See cadence note above. Wave 3.5 later translates this
    // enum into the exact modulo; until then this field is descriptive metadata for
    // docs/evidence and does not change behavior.
    cadence: PollCadence::Slow,
};
const LLY_DTC_ACTIVE: DtcServiceDefinition = DtcServiceDefinition {
    key: "gm.class2.dtc.active",
    label: "GM Class 2 DTC (Tech2 active/history)",
    route_set: RouteSet::discovered_on_bus(BusKey::new("j1850vpw")),
    service_id: 0x19,
    request_data: &[0x92, 0xFF, 0x00],
    decoder_id: "gm.class2.dtc",
    backoff_policy: BackoffPolicy { skip_count: 3, suppress: &[BackoffClass::NoData, BackoffClass::Unsupported] },
    cadence: PollCadence::Slow, // advisory only; see above
};
```

**Routing decision (the load-bearing OWL call).** Do NOT route `$19` via Wave 3's `AddressTemplate::J1850 { node }` -> `PhysicalAddress::J1850 { node, header: [0x6C, node, 0xF1] }` synthesis. The current TUI path (`session_runner.rs:511`) sends via `Target::Module(module.0)`, so the J1850 header comes from the SPEC's `Module.address` (`obd2-core vehicle/mod.rs:81`), resolved by `resolve_module_address`. That is a DIFFERENT header source than the GM Class 2 `class2_header` helper (which the GUI's `request_gm_node` uses, Wave 6). For the LLY spec these coincide (`6C <node> F1`, spec test `node:0x29 -> [0x6C,0x29,0xF1]`), but switching to synthesis (a) drops the `BusNotAvailable` and single-flight guards and (b) diverges silently if a discovered `ModuleId` is not in the synthesized node set. `RouteScope::DiscoveredOnBus` resolves to `Target::Module(discovered_id)` and preserves the exact current bytes and guards.

### Tests

**Integration-test harness (cross-wave standard): Elm327Adapter + MockTransport::expect, NOT MockAdapter.** `MockAdapter::routed_request` (`obd2-core adapter/mock.rs:300`-`:310`) downgrades `PhysicalTarget::Addressed(_)` to `Target::Broadcast` and returns canned `[0x80, 0x00]` for service `0x22` (and `0x21`/`0x22` at `:265`) - it physically cannot honor addressed J1850, and obd2-core is frozen ("`cargo test -p obd2-core` unchanged"), so it cannot be fixed here. All Wave 5 dispatch tests therefore drive an `Elm327Adapter::new(Box::new(mock_transport))` (`obd2-core adapter/elm327.rs:43`) wrapping a `MockTransport` whose `expect(command, response)` pairs (`obd2-core transport/mock.rs:29`) assert the exact ELM command stream (the `AT SH 6C <node> F1` header-set plus the `19 FF FF 00` / `19 92 FF 00` request) and return the canned `59 ...` frames. This is Wave 9's harness; use it verbatim so addressed J1850 is actually exercised.

Unit (in `profiles/gm/class2.rs` and `profiles/model.rs`):
- `test_gm_class2_dtc_service_definitions` - the LLY profile exposes exactly two DTC services with `key == "gm.class2.dtc.all"`/`"gm.class2.dtc.active"`, `service_id == 0x19`, `request_data == [0xFF,0xFF,0x00]`/`[0x92,0xFF,0x00]`, `decoder_id == "gm.class2.dtc"`. Asserts the bytes equal `gm_class2::CLASS2_DTC_ALL_REQUEST`/`_ACTIVE_REQUEST` so a future edit to either side fails loudly.
- `test_decode_class2_dtcs_checked_matches_legacy_decoder` - feeds `[0x59, 0x43, 0x79, 0x93]`; asserts one `DecodedDtc` with `dtc.code == "C0379"`, `dtc.status == DtcStatus::Stored`, `dtc.notes == Some("GM Class 2 status 0x93: mil|history|current|immature")`, `dtc.source_module == None` (runtime stamps it later). Mirrors `gm_class2.rs` tests `decodes_known_class2_payload_with_positive_service_byte` + `into_dtc_preserves_gm_status_in_notes`.
- `test_decode_class2_dtcs_checked_empty_and_zero` - `[]` and `[0x00,0x00,0x00]` -> zero `DecodedDtc`, `Ok` (mirrors `empty_and_zero_payloads_are_empty`).
- `test_decode_class2_dtcs_checked_bad_length_is_error` - `[0x43, 0x79]` -> `Err(ProfileDecodeError::Decode(s))` whose inner string `s` equals the `GmClass2DecodeError::UnexpectedPayloadLength` Display text (mirrors `rejects_unproven_payload_shape`; pins both the `Decode` variant and the inner string).
- `test_gm_class2_backoff_policy_is_three_skips` - `skip_count == 3`, suppress set is exactly `{NoData, Unsupported}`.

Integration (`crates/obd2-dash/tests/dtc_service_dispatch.rs`; Elm327Adapter + MockTransport::expect harness):
- `test_addressed_j1850_header_is_sent` - replaces the old MockAdapter guard. With the LLY spec selected, assert the `MockTransport` observes the addressed header-set command for the resolved module (`AT SH 6C <node> F1`) immediately before the `19 FF FF 00` request - proving the path actually addresses J1850 rather than broadcasting. Run this first; a red here (e.g. a missing/incorrect header) invalidates every other integration assertion. (This is why MockAdapter is unusable: it would silently broadcast and the header assertion could never be written.)
- `test_no_selected_profile_means_no_class2` - with no `SelectedProfile`, the scan entries contain only `Stored`/`Pending`/`Permanent` rows; zero `GmClass2All`/`GmClass2Active` rows (plan invariant #3/#4).
- `test_class2_sent_only_to_discovered_modules` - discovery (driven via the spec + MockTransport responses) exposes `ecm` + `tcm` on the J1850 bus plus one module on a different bus; assert `$19` is issued to `ecm` and `tcm` only (two `AT SH` headers), never an 11-node fan-out and never the off-bus module. Pins `RouteScope::DiscoveredOnBus` == `dtc_scan_modules` and guards the inventory item-3 coverage regression (fewer or more DTCs).
- `test_class2_source_module_and_scope_from_discovery` - mock returns `$59` codes for `tcm`; assert the resulting `Dtc.source_module == Some("tcm")` and `DiagnosticScanScope::Module("tcm")`, NOT a `DEFAULT_CLASS2_NODES` display label like `"TCM"` or `"ECM/PCM"`. Pins "labels from route, not hardcoded".
- `test_class2_classification_matches_legacy` - drive the transport through each branch (positive with codes, positive empty/all-zero, `NO DATA`, a `7F 19 11` serviceNotSupported frame, a `7F 19 12` subFunctionNotSupported frame, a transport-level error) and assert the published `DiagnosticScanResult` is `Codes(n)`/`Empty`/`NoData`/`Unsupported(..)`/`Unsupported(..)`/`Error(..)` respectively, matching the table above.
- `test_decode_rejects_negative_response_triplet` (DIRECT decoder test, bypasses transport) - `decode_class2_dtcs_checked(&[0x7F, 0x19, 0x11])` and the no-`0x59`-prefix raw form both return `Err(ProfileDecodeError::NegativeResponse { service: 0x19, nrc: 0x11 })`, NEVER `Ok(vec![<phantom DTC>])`. This is the test the transport-level case cannot cover: golden-corpus replay feeds payloads to the decoder directly and never exercises the adapter's NRC parsing, so a `7F 19 xx` frame would otherwise decode as a fabricated `U..` code. Add a corpus entry for a captured `7F 19 11` reply asserting the same.
- `test_class2_backoff_suppresses_three_cycles` - a `NoData` response is cached; the next 3 `execute_dtc_services` calls re-emit the cached `NoData` without writing to the transport (assert MockTransport sees no new command); the 4th writes again. Reproduces `GmClass2Backoff` (`:649`). This is the single regression guard for the 3-skip suppression that the Wave 3/Wave 5 ownership split exists to protect.
- `test_generic_dtc_services_unchanged` - a non-LLY/CAN vehicle with no profile still scans `03`/`07`/`0A` broadcast + per-module; this path never enters the runtime. Guards "keep generic 03/07/0A in the generic path."
- `test_scan_entry_ordering_standard_then_class2` - entry order is broadcast standard (3), per-module standard (3 each), then GM Class 2 per-module (2 each); `count_diagnostic_scan_targets` and the MODULE SCAN title count are unchanged. Guards dedup ordering and the renderer's grouping loop (`renderers.rs:1209`).

Golden-corpus (`crates/obd2-dash/tests/corpus_dtc.rs` over `tests/corpus/profile/gm.gmt800.lly.class2/dtc-*.jsonl`, read via the shared corpus loader):
- `corpus_gm_class2_dtc_goldens_replay_identically` - each entry is `{signal_key, module, request_bytes, response_bytes, expected_decoded[]/expected_error}` replayed through `decode_class2_dtcs_checked`; asserts byte-for-byte and value-for-value identical output. **These goldens are SYNTHETIC.** Per the recording inventory (section D), there is NO `$19`/`$59` traffic in any `raw-captures/*.obd2raw` file (the 407 `W 221948` hits are Mode 22 reads of DID `0x1948`, not service `0x19`). Seed from the synthetic payloads already proven in `gm_class2.rs` unit tests (`C0379`, `U1024`, multi-triplet, zero-skip). The commit message MUST state the DTC corpus is synthetic and that real-capture coverage is limited to the negative path.
- `corpus_lly_standard_dtc_negative_path` - real captures DO show `03`/`07`/`0A` returning `NO DATA` or `7F0311` (service 0x03, NRC 0x11 serviceNotSupported). Pin that the generic path classifies these as `NoData`/`Unsupported`, proving Wave 5 left the generic path alone.
- `corpus_lly_signal_goldens_unchanged` - re-run the existing LLY enhanced-signal golden corpus (frozen by Wave 4) and assert zero diffs. Wave 5 touches only the DTC path, so any signal-corpus diff is a regression in this wave.

Architectural (`crates/obd2-dash/tests/dtc_service_dispatch.rs` or the Wave 3 import test):
- `test_session_runner_does_not_construct_class2_requests` - source-scan asserting `session_runner.rs` no longer imports `decode_class2_dtcs`, `CLASS2_DTC_ALL_REQUEST`, `CLASS2_DTC_ACTIVE_REQUEST`, or `SERVICE_REPORT_DTCS_BY_STATUS`, and contains no `raw_request(0x19` / `SERVICE_REPORT_DTCS_BY_STATUS` send. Enforces Phase 3 "remove TUI/session ad hoc GM `$19` execution."
- `test_class2_routed_helpers_have_no_live_callers` - `class2_routed_request`/`class2_header`/`class2_dtc_all_request`/`class2_dtc_active_request` are referenced only by `examples/*`, `profiles/gm/*`, and `#[cfg(test)]`, never by `session_runner.rs`/`app.rs`/`tui/*`/`widget/*`.
- `test_dtc_decoder_selected_by_decoder_id` - decoding routes through `decoder_id` lookup on the selected profile, not a global function call. Enforces "DTC decoder is selected by profile service definition" (plan Decoder isolation).
- `test_wave0_callsite_allowlist_decremented` - the Wave 0 frozen call-site inventory for `$19`/`gm_class2`/`decode_class2_dtcs` is updated to the new (lower) counts in this commit; the inventory test is green with no stale frozen entries. Guards the cross-wave rule that migrating waves must decrement the Wave 0 allowlist as they remove sites.

### Acceptance criteria

- [ ] `$19 FF FF 00` and `$19 92 FF 00` exist as two `DtcServiceDefinition`s in `profiles/gm/class2.rs` with `service_id == 0x19`, `request_data` byte-identical to `CLASS2_DTC_ALL_REQUEST`/`CLASS2_DTC_ACTIVE_REQUEST`, `decoder_id == "gm.class2.dtc"`.
- [ ] `RouteSet`/`RouteScope`/`RouteSet::single` and `ProfileDecodeError` are defined ONLY in Wave 1's final model; Wave 5 adds no second definition. `decode_class2_dtcs_checked` returns `ProfileDecodeError::Decode(_)` for normal decode failures and `ProfileDecodeError::NegativeResponse { service, nrc }` for leading `7F 19 xx`.
- [ ] Wave 5 is the sole owner of the `$19` send: Wave 3 left `append_gm_class2_dtcs`/`GmClass2Backoff`/`should_scan_gm_class2`/`gm_class2_scan_modules` untouched, and the 3-skip backoff is preserved through the migration.
- [ ] Generic `03`/`07`/`0A` remain in `scan_standard_dtcs`/`append_dtc_probe`, unmodified; no profile is consulted for them.
- [ ] `$19` is sent only when a `SelectedProfile` owns the capability; `session_runner.rs` no longer calls `raw_request(0x19,..)` and no longer imports `gm_class2` constants or `decode_class2_dtcs`.
- [ ] Module coverage is identical to `dtc_scan_modules` (discovered modules on the active J1850 bus): no static 11-node fan-out, no narrowing.
- [ ] Per-module `Dtc.source_module` and `DiagnosticScanScope` derive from the discovered `ModuleId` (`ecm`/`tcm`/`bcm`/`ebcm`), not `DEFAULT_CLASS2_NODES` display labels.
- [ ] Routing still goes through `Target::Module` + `resolve_module_address` (spec-derived J1850 header), preserving `BusNotAvailable` and single-flight; no header re-synthesis for `$19`.
- [ ] `DiagnosticScanResult` classification and `DiagnosticScanEntry` ordering are byte-identical (the seven-branch table holds; the `Error` string is the raw `GmClass2DecodeError` Display, no `ProfileDecodeError` prefix; standard rows precede Class 2 rows).
- [ ] GM status notes string and status byte preserved exactly (`"GM Class 2 status 0x..: ..."`).
- [ ] Backoff caches `NoData`/`Unsupported` for exactly 3 skips per `(module, service)`.
- [ ] Cadence preserved by the existing `cycle % 60 == 0` gate in `session_runner.rs:217` (passed to `poll_dtcs` as `include_gm_class2`). Wave 5 does NOT move cadence into the profile and does NOT claim `PollCadence::Slow` reproduces the modulo; `PollCadence::Slow` is advisory metadata. The gate stays in `session_runner` until the (unowned) Phase-4 scheduler exists to translate `PollCadence` into the exact modulo.
- [ ] MODULE SCAN title count and the five render states (`N dtc`/`empty`/`no data`/`unsup`/`error`) render identically; domain `DtcService` enum and `.label()` (`"19ff"`/`"1992"`) unchanged.
- [ ] Integration dispatch tests run on `Elm327Adapter` + `MockTransport::expect` and assert the addressed J1850 header (`AT SH 6C <node> F1`) is sent. No Wave 5 test uses `MockAdapter` for addressed J1850 (it cannot honor it), and `cargo test -p obd2-core` is unmodified.
- [ ] The Wave 0 frozen call-site allowlist is decremented in this commit for every removed `$19`/`gm_class2` site; the Wave 0 inventory test is green.
- [ ] Synthetic `$19` golden corpus added under `tests/corpus/profile/gm.gmt800.lly.class2/`, conforming to the pinned shared corpus schema (`signal_key`+`module`) and read through the shared loader; commit states it is synthetic.
- [ ] **Existing LLY golden corpus stays green with zero diffs** (signal goldens and the standard-DTC negative-path goldens).
- [ ] All four architectural tests pass.
- [ ] `cargo test -p obd2-core` and `cargo test -p obd2-dash` are green.

### Rollback notes

- **Independently shippable** once Waves 1-3 have landed. It does not require Wave 4 (signals) or Wave 6 (GUI). The new DTC capability and runtime path are additive; the `gm_class2.rs` decoder and the domain `DtcService` enum are unchanged, so the blast radius is `poll_dtcs`'s GM branch plus the new profile data.
- **Flag during transition.** Gate the new path behind a `cfg(feature = "profile_dtc_services")` (default on) and keep `append_gm_class2_dtcs`, `should_scan_gm_class2`, `gm_class2_scan_modules`, and `GmClass2Backoff` compiled-but-`#[allow(dead_code)]` for one release. Reverting is then: turn the feature off (or restore the single `append_gm_class2_dtcs(session, &mut scan, gm_backoff).await` call at `session_runner.rs:433` inside the existing `if include_gm_class2 && should_scan_gm_class2(session)` gate at `:432`, and re-add the `gm_class2` import block at `:25`-`:28`). If you revert call sites, also restore the corresponding Wave 0 allowlist counts. No data migration, no recording-format change, no UI change is involved, so a revert is a pure call-site swap.
- **Cadence on rollback:** the `cycle % 60 == 0` gate at `:217` is untouched by both the forward change and the rollback, so cadence is identical either way. Do not let a rollback re-derive cadence from `PollCadence`.
- **What stays behind the runtime after rollback of THIS wave:** the `DtcServiceDefinition`s and `decode_class2_dtcs_checked` are dead config until re-enabled; they compile and are covered by unit tests, so they do not bit-rot.
- **Cross-wave coupling to flag for the reviewer:** the GUI's `scan_gm_class2_dtcs` (`apps/obd2-gui/src-tauri/src/main.rs:776`) loops `DEFAULT_CLASS2_NODES` (all 11 static nodes) while the TUI uses discovered modules. Wave 5 makes the TUI/profile the single source of truth with `RouteScope::DiscoveredOnBus`; when Wave 6 repoints the GUI at the same runtime, GUI `$19` coverage will deliberately CHANGE from 11 static nodes to discovered modules. That is a Wave 6 behavior change, not a Wave 5 one - Wave 5 must not silently alter GUI output, and the GUI path is out of scope here. Note it in the Wave 6 handoff so it is not mistaken for a regression.

Verification confirms the corrections: `should_force_standard_poll` is global at `session_runner.rs:131`/`:824` (tests `:1087`/`:1108`/`:1112`), cadence ladder `cycle % 5/10/20/60` at `:207`/`:216`/`:217`/`:232`, candidate `0x1542` at `renderers.rs:564`, fuel-rail `preferred_over` (`0x163E` -> `0x1170`) at `tui/ui.rs:2403`, and there is no `profiles/` dir and no `scheduler.rs` anywhere (the orphan is real). The GUI `main.rs` and plan doc exist. Applying the corrections now.

---

## Wave 6: GUI/TUI Unification

### Objective

Make `ProfileRuntime` the single source of every manufacturer-specific request and decode for both frontends: the TUI (`session_runner.rs`) and the Tauri GUI (`apps/obd2-gui/src-tauri/src/main.rs`) both consume the same `SelectedProfile`-derived poll plan and the same decoded result, with zero GM/LLY logic left in either UI layer. Delete the GUI's parallel GM polling/decoding/gating path (`request_gm_node`, `read_gm_did_value`, `read_enhanced_scalar`, `scan_gm_class2_dtcs`, `gm_lly_profile_enabled`, and the inline preference/cadence/fallback policy) so that "identical planned requests for the same `SelectedProfile`" is true by construction, not by reviewer vigilance.

CRITICAL SEQUENCING: the deletion of inline poll policy (fuel-rail `preferred_over`, baro-at-idle fallback, DTC cadence) is GATED on the dedicated Phase-4 poll-policy/scheduler wave having LANDED first and the frozen corpus being green for those exact behaviors (see Depends on). Wave 6 must NOT delete any inline policy ahead of that wave -- doing so produces a green build that silently shows wrong/blank fuel-rail and baro for the real LLY truck and loses cadence. There is no compile-time guard for this; only the corpus catches it.

This satisfies Invariant 9 ("TUI and GUI must consume the same profile runtime and planned request graph") and closes the plan's named #1 regression vector ("TUI and GUI have separate GM paths").

### Depends on (must land first, and why)

This wave is a pure consumer-unification wave. It builds almost no new manufacturer logic; it deletes duplicates and routes both UIs through machinery earlier waves produced. It cannot start until all of the following are merged and green:

- Phase 1 wave (neutral profile model + registry): `profiles::model` and `profiles::registry` must exist, exposing `ProfileId`, `SelectedProfile`, `VehicleContext`, `DecodedSignal`, `DecodedDtc`, `CapabilityId`, `RouteDefinition`, `AddressTemplate`, `PollCadence`. Wave 6 imports these; it does not define them.
- Phase 2 wave (session-owned profile selection): the dash-side session context must already own `ProfileState { generation, selected, exact_matches, partial_matches, ambiguity }` and expose a read accessor. This is the keystone: the GUI's local re-decision (`gm_lly_profile_enabled = lly_profile_matches(...)` at `main.rs:505`) is only deletable if the selected profile already lives in shared session state that both UIs read. Without Phase 2, deleting the GUI gate strands the GUI in generic-only mode.
- Phase 3 wave (central dispatcher): `ProfileRuntime::execute_request` must exist and be the validated send path that ends at `Session::send_request` (single-flight). The `$19` Class 2 scan must already be unified onto a profile `DtcServiceDefinition.route_set` in the TUI. If the TUI still uses `gm_class2_scan_modules` (discovered modules) while the GUI uses `DEFAULT_CLASS2_NODES`, the parity test cannot pass; that unification is Phase 3's job, not Wave 6's. NOTE: the dispatcher (`execute_request`) is Phase 3, but the SCHEDULER/PLANNER (`plan_poll_cycle` + `profiles/scheduler.rs`) and the policy it reads are Phase 4 (next bullet), not Phase 3.
- Phase 4 wave (poll policy + scheduler into profiles) -- DEDICATED, MUST LAND BEFORE WAVE 6 (newly inserted predecessor): this wave previously did not exist among the planned waves; it is now a hard, dedicated predecessor and the single highest-risk dependency of Wave 6. It creates `crates/obd2-dash/src/profiles/scheduler.rs` and `ProfileRuntime::plan_poll_cycle`, and re-homes ALL of today's global / UI-local poll policy into profile-owned policy. Verified that NONE of this is migrated by any other wave and all of it is still global/UI-local in source today:
  - Forced standard PID reads: `should_force_standard_poll` (called global at `session_runner.rs:131`, defined at `:824`, pinned by `test_should_force_barometric_standard_poll` and `test_should_force_dashboard_standard_polls` near `:1087`/`:1108`/`:1112`) becomes a per-profile `force_standard_pids` set. The plan's "LLY forced standard PIDs are not global" isolation test is written by Phase 4.
  - Enhanced/DTC cadence: the inline ladder `cycle % 5` (`session_runner.rs:207`), `cycle % 10` (`:216`), `cycle % 60` Class 2 (`:217`), `cycle % 20` (`:232`), and the GUI's `cycle % 12` (`main.rs:402`) become profile-owned `PollCadence` on each capability.
  - No-data / unsupported backoff generalization.
  - Candidate-DID suppression: the `0x1542` desired-MAP candidate currently gated in `renderers.rs:564`, plus the TUI rail fallback `enhanced_reading(.., 0x163E).or_else(.., 0x1170)` at `tui/ui.rs:2403`.
  - `preferred_over` enforcement: generic rail preferred over range-suspect enhanced `0x163E`.
  Wave 6 CONSUMES this wave's output and authors NONE of this policy. HARD GATE: Wave 6 must NOT delete any inline policy (GUI fuel-rail `.or(...)` `main.rs:322-343`, baro-at-idle fallback `main.rs:418-426`, `cycle % 12` cadence `main.rs:402`, and the TUI/`renderers.rs` equivalents) until this Phase-4 wave has LANDED and the frozen corpus exercises `preferred_over`, baro fallback, and the `$19` cadence GREEN. Deleting the GUI/TUI copies with no profile-owned replacement is the headline silent-regression vector: a green build that shows wrong/blank fuel-rail and baro for the real LLY truck and loses cadence, caught by no compile-time check.
- Phase 5 wave (LLY definitions migrated): the LLY profile must expose stable capability keys the GUI maps to its flat snapshot fields (VGT, injector balance, fuel rail, desired MAP, baro, TCM transmission temp). Wave 6's GUI mapping table is keyed on those `SignalDefinition.key` strings; they must be frozen by Phase 5 first. Phase 5 also fixes the TCM `0x1940` `module_label = "ecm"` bug; Wave 6 inherits the corrected route/module identity.
- Phase 6 wave (generalized evidence) STRONGLY recommended before this wave: the GUI snapshot carries `source_confidence: Vec<SignalEvidence>` built today by GM-specific `gm_signal_evidence`/`gm_definition_source`/`gm_definition_confidence`. If Phase 6's profile evidence record (with `profile_id`, `capability_id`, TXD/RXF/RXD/MTH source fields) is not available, Wave 6 must keep a thin GM-shaped evidence mapping and accept that the GUI's `SignalEvidence` JSON is populated from the profile result rather than from `gm_definition_source`. Treat Phase 6 as a soft dependency: if it slips, Wave 6 ships with evidence mapped from `DecodedSignal` + `SignalDefinition.source_fields` directly (still no GM symbols), and the richer evidence record arrives in Phase 6.
- Regression-firewall corpus (frozen `tests/corpus/profile/gm.gmt800.lly.class2/*.jsonl`): must exist and be green before Wave 6, because every deletion in this wave is behavior-bearing. The corpus is the only thing that proves the GUI's removed inline policy did not change the bytes on the wire. It must specifically exercise `preferred_over`, baro-at-idle fallback, and the `$19` cadence (this is the Phase-4 gate's proof artifact). Caveat (positive coverage): the corpus pins the `$19` NO-DATA/`7F0311`/negative path from real captures; positive `$19`/`$59` decode coverage is SYNTHETIC-ONLY (see Tests), because `raw-captures/` contains no positive `$19`/`$59` traffic.

Explicitly NOT a dependency: Phase 7 (recording v3) and Phase 8 (active tests). Wave 6 leaves the GUI active-test path (`request_active_test`, `active_tests_snapshot`, `write_active_test_evidence`, the `GmActiveTestCommand` Tauri command) entirely untouched. Migrating those is Phase 8 and changes the Tauri public contract; pulling it into Wave 6 would balloon the blast radius.

### Files touched

- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/apps/obd2-gui/src-tauri/src/main.rs`
  - DELETE `async fn request_gm_node(...)` (line 821) and its hardcoded `PhysicalAddress::J1850 { node, header: [0x6C, node, 0xF1] }` + `session.adapter_mut().routed_request(&request)` (line 836). This is the GUI's only raw manufacturer send; it must not survive.
  - DELETE `async fn read_gm_did_value(...)` (641), `async fn read_enhanced_scalar(...)` (617), `async fn scan_gm_class2_dtcs(...)` (776). These are the parallel poll path.
  - DELETE GM-shaped helpers: `gm_definition_source` (733), `gm_definition_confidence` (750), `gm_request_text` (839), `gm_signal_evidence` (858), `pressure_to_psi` (762, GM-unit-string dispatch), `decode_class2_dtc_result` (962), `build_pending_module_scan` (1121), `pending_module_scan` (1146), and `dtc_from_core` (1050) GM-status-string parsing.
  - DELETE the `gm_lly_profile_enabled: bool` field (160, default 177), its set/clear at `connect()` (505-506, 517), the `let gm_lly_enabled = ...` snapshot (213), and every `if gm_lly_enabled { ... }` block in `try_snapshot` (baro 256, desired MAP 272, fuel rail 302/322-343, VGT 364, injector balance 377, Class 2 402, module scan 430). Replace with consumption of the shared poll result.
  - DELETE inline poll policy: fuel-rail `preferred_over` `.or(...)` (322-343), baro-at-idle fallback (418-426), `cycle % 12 == 0` cadence (402). PRECONDITION (do NOT skip): these deletions are LEGAL only after the Phase-4 poll-policy/scheduler wave has landed and re-homed this policy into profile-owned `scheduler.rs`/`plan_poll_cycle`, AND the frozen corpus is green for `preferred_over`, baro fallback, and the `$19` cadence. If Phase 4 has not landed, leave these blocks in place and do not start Wave 6 -- deleting them here with no profile-owned replacement silently regresses LLY display values on a green compile. (The matching TUI-side policy in `session_runner.rs` `should_force_standard_poll`/cadence and `renderers.rs:564`/`tui/ui.rs:2403` is also Phase 4's to migrate; Wave 6 does not touch it beyond routing the TUI poll through `run_poll_cycle`.)
  - DELETE GUI-local unit tests calling `find_lly_did` directly (1354-1413). Their coverage moves to the frozen profile corpus. Removing them is intentional; note it in the commit.
  - REMOVE imports now unused: from `obd2_core::adapter` drop `Adapter, PhysicalTarget, RoutedRequest`; from `obd2_core::vehicle` drop `PhysicalAddress`; drop the entire `obd2_dash::gm_class2::{...}` import (line 17-20), `obd2_dash::gm_enhanced::{find_lly_did, lly_profile_matches, GmDidDefinition}` (21), and the four `LLY_*_DID` consts (31-35). KEEP `obd2_dash::gm_active::*` and `obd2_dash::gm_evidence::{GmEvidenceWriter, GmVehicleIdentity}` ONLY for the active-test path (Phase 8 removes them).
  - REWIRE `try_snapshot`: build/read the `SelectedProfile` from shared session state, call `ProfileRuntime::run_poll_cycle`, and map the returned `ProfilePollResult` into the EXISTING `DiagnosticSnapshot` flat fields via a new `apply_profile_result`. The serialized JSON shape stays byte-identical (frontend contract unchanged this wave).
  - REWIRE `connect()` cache invalidation: replace the `gm_lly_profile_enabled`-driven cache clear (522-526) with generation-keyed invalidation (clear `cached_dtc_count`/`cached_dtcs`/`cached_modules` when `profile_state().generation` changes or `selected` is `None`).
  - KEEP generic Mode 03 DTC decode (`decode_standard_dtc_result` 942, `decode_standard_dtcs` 1012, `dtc_snapshot` 1040, `scan_error_label` 988) for now: standard DTCs are generic, not manufacturer-routed. Flag as a duplicate-of-core to consolidate in a later wave; do not let scope creep into it here.

- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/frontend.rs`
  - The shared consumption seam: `PlannedRequest`, `PollPlan`, `ExecutedSignal`, `ProfilePollResult`, and the single shared loop `run_poll_cycle`. This is the one place both UIs call so they cannot drift. NOTE: `run_poll_cycle` does not author poll policy; it executes a `PollPlan` produced by the Phase-4 `plan_poll_cycle`/`scheduler.rs` (see Exact APIs).

- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/mod.rs`
  - `pub mod frontend;` and re-export the new public types.

- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/session_runner.rs`
  - Route the TUI's manufacturer-profile polling through `profiles::frontend::run_poll_cycle` so the TUI and GUI share the identical loop. Because the policy (`should_force_standard_poll` at `:131`/`:824`, cadence at `:207`/`:216`/`:217`/`:232`) is migrated by Phase 4, this rewire assumes that migration already landed; Wave 6 only swaps the call site to `run_poll_cycle`, it does not move the policy. The generic SAE polling (Mode 01 PIDs, VIN, voltage, generic Mode 03 DTCs) stays in `session_runner` unchanged; only the profile-owned portion moves behind `run_poll_cycle`.

- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/profile_request_parity.rs`
  - Plan-level determinism + execution-level request-stream parity via a recording mock.

- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/architecture_no_raw_routed_in_live.rs`
  - Source-level import firewall over both live UI modules.

- (RUN, do not create) `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus/profile/gm.gmt800.lly.class2/*.jsonl`
  - Owned by the firewall wave; Wave 6 only asserts it stays green with zero diffs.

### Exact APIs

New types and the shared loop in `crates/obd2-dash/src/profiles/frontend.rs`. These are the only genuinely new symbols this wave introduces; everything else is referenced from earlier waves or obd2-core.

```rust
use obd2_core::adapter::Adapter;
use obd2_core::protocol::service::Target;     // obd2-core/.../protocol/service.rs:149 (Eq)
use obd2_core::session::Session;
use crate::profiles::model::{
    CapabilityId, Confidence, DecodedDtc, DecodedSignal, ModuleKey,
    PollCadence, ProfileDecodeError, ProfileId, SelectedProfile,
};
use crate::profiles::runtime::{ProfileError, ProfileRuntime};

/// Deterministic, side-effect-free description of one manufacturer-profile
/// request. EQUALITY across frontends is the whole point of this wave, so it
/// derives Eq and is compared byte-for-byte in the parity test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedRequest {
    pub capability: CapabilityId,
    pub service_id: u8,
    pub request_data: Vec<u8>,   // resolved bytes (DID + command_suffix); NEVER a header
    pub target: Target,          // Target::Module(module_key) or Target::Broadcast
    pub module: ModuleKey,       // logical module identity from RouteDefinition.module
    pub decoder_id: &'static str,
    pub cadence: PollCadence,
}

/// The full ordered request graph for a given cycle. PartialEq is the artifact
/// the parity test pins; cadence-skipped capabilities are simply absent for
/// that cycle, so the Vec length varies by cycle and must be compared as an
/// ordered sequence, not a set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollPlan {
    pub generation: u64,
    pub profile_id: ProfileId,
    pub requests: Vec<PlannedRequest>,
}

#[derive(Clone, Debug)]
pub struct ExecutedSignal {
    pub capability: CapabilityId,
    pub key: &'static str,
    pub decoded: Result<DecodedSignal, ProfileDecodeError>,
    pub confidence: Confidence,
}

#[derive(Clone, Debug)]
pub struct ProfilePollResult {
    pub generation: u64,
    pub profile_id: ProfileId,
    pub signals: Vec<ExecutedSignal>,
    pub dtcs: Vec<DecodedDtc>,
}

impl ProfileRuntime {
    /// Pure planning: identical output for identical (selected, cycle). No I/O.
    /// OWNED BY THE PHASE-4 POLL-POLICY/SCHEDULER WAVE (crates/obd2-dash/src/
    /// profiles/scheduler.rs), NOT by Wave 6 and NOT by the Phase-3 dispatcher.
    /// This is where profile-owned cadence, preferred_over, candidate-DID
    /// suppression, forced-standard-PIDs, and no-data/unsupported backoff are
    /// resolved into the ordered PollPlan. Wave 6 only adds the
    /// `#[derive(PartialEq, Eq)]` on PollPlan/PlannedRequest (additive) so the
    /// parity test can compare plans; it does not author the policy.
    pub fn plan_poll_cycle(
        &self,
        selected: &SelectedProfile,
        cycle: u64,
    ) -> Result<PollPlan, ProfileError>;

    /// THE shared loop both UIs call. Plans the cycle (via plan_poll_cycle, which
    /// already applied profile-owned cadence/backoff/preferred_over/suppression),
    /// executes each PlannedRequest through `execute_request` (which validates
    /// token + generation, resolves ModuleMap + RouteDefinition -> ResolvedRoute,
    /// builds RoutedRequest, calls Session::send_request, decodes via decoder_id,
    /// writes evidence), and returns decoded results.
    pub async fn run_poll_cycle<A: Adapter>(
        &self,
        session: &mut Session<A>,
        selected: &SelectedProfile,
        cycle: u64,
    ) -> Result<ProfilePollResult, ProfileError>;
}
```

POLICY PROVENANCE (read before treating Wave 6 as "single source of truth"): `run_poll_cycle` is the single shared EXECUTION loop, but its policy INPUTS -- `preferred_over`, per-capability cadence, candidate-DID suppression, forced-standard-PIDs, and no-data/unsupported backoff -- are NOT authored here. They are read from the profile-owned policy that the Phase-4 poll-policy/scheduler wave installs into `plan_poll_cycle`/`scheduler.rs`. Until that wave lands, `run_poll_cycle` can be made to compile and run, but it will plan cycles WITHOUT preferred_over/cadence/suppression and therefore reproduce WRONG LLY display values (blank/wrong fuel-rail and baro, lost `$19` cadence). "Parity by construction" on that path is parity on a path that produces wrong values for BOTH UIs -- it makes the regression symmetric, not absent. Do not merge Wave 6 ahead of Phase 4. `run_poll_cycle` becomes the single FAITHFUL source for both UIs only after Phase 4 has re-homed the policy and the corpus is green for it.

Referenced from earlier waves (NOT defined here; signatures shown so the GUI call sites are unambiguous):

```rust
// Phase 2 wave: shared dash-side session context owns this. The GUI reads it
// instead of computing gm_lly_profile_enabled locally.
pub struct ProfileState {
    pub generation: u64,
    pub selected: Option<SelectedProfile>,
    pub exact_matches: Vec<ProfileId>,
    pub partial_matches: Vec<PartialProfileMatch>,
    pub ambiguity: Option<ProfileAmbiguity>,
}
// accessor the GUI uses (exact owner type named by Phase 2; e.g. ProfileSession<A>):
//   fn profile_state(&self) -> &ProfileState;

// Phase 3 wave dispatcher entry (single validated request):
//   pub async fn execute_request<A: Adapter>(
//       &self, session: &mut Session<A>, selected: &SelectedProfile,
//       planned: &PlannedRequest,
//   ) -> Result<ExecutedSignal, ProfileError>;

// Phase 4 wave scheduler (crates/obd2-dash/src/profiles/scheduler.rs):
//   owns the cadence ladder, preferred_over, candidate-DID suppression,
//   forced-standard-PIDs, and backoff that plan_poll_cycle reads. No
//   `match manufacturer` -- policy is data on the profile.
```

obd2-core symbols the dispatcher (not the GUI) touches, grounded in the core inventory:

- `Adapter::routed_request(&mut self, req: &RoutedRequest)` -- `obd2-core/crates/obd2-core/src/adapter/mod.rs:112`. The GUI must NOT call this directly anymore; only `execute_request` does, and it reaches it through `Session::send_request`.
- `Session::send_request` (private, `session/mod.rs:1141`) and `resolve_request` (`:1164`): preserves single-flight `request_in_flight`, `record_visible_target`, and the `BusNotAvailable` bus-availability check. Routing profile requests through `Target::Module(module_key)` here is REQUIRED so the TCM-vs-ECM address resolves correctly (`resolve_module_address`, `:1178`) and the `0x1940` TCM signal hits node `0x18`, not ECM.
- `Target::Module(String)` -- `protocol/service.rs:149`. `run_poll_cycle` emits this from `RouteDefinition.module`; the string must equal a discovered `ModuleId` (`vehicle/mod.rs:14`).

GUI mapping function (in `apps/obd2-gui/src-tauri/src/main.rs`, replaces all the deleted GM blocks):

```rust
/// Maps capability-keyed decoded results into the EXISTING DiagnosticSnapshot
/// flat fields. The serialized JSON contract is unchanged this wave. Keys MUST
/// equal the LLY profile SignalDefinition.key strings frozen in Phase 5.
fn apply_profile_result(snapshot: &mut DiagnosticSnapshot, result: &ProfilePollResult);

/// DecodedDtc -> DtcSnapshot, replacing dtc_from_core's "GM Class 2 status "
/// string-scraping. Status/flags come from typed DecodedDtc fields, not notes.
fn dtc_snapshot_from_decoded(module: &str, dtc: &DecodedDtc) -> DtcSnapshot;
```

The capability-key contract the GUI maps on (owned by the LLY profile, Phase 5; pin these in a shared `const` so the mapping and the corpus agree): `vgt_vane_actual`, `vgt_vane_desired`, `fuel_rail_actual`, `fuel_rail_desired`, `desired_map`, `barometric_pressure`, `injector_balance_1`..`injector_balance_8`, `injector_pulse_width_1`..`injector_pulse_width_8`, `transmission_temperature`, `oil_pressure`, and the `$19` DTC service key `gm_class2_dtcs`. If any key the GUI maps is absent from the profile, the build still compiles (it just renders no data) -- so a unit test must assert the GUI's mapped key set is a subset of the LLY profile's declared keys, or a Phase 5 rename silently blanks the GUI.

### Tests

- Unit (`profiles::frontend`, in `frontend.rs` `#[cfg(test)]`):
  - `plan_poll_cycle_is_deterministic`: same `(SelectedProfile, cycle)` yields byte-identical `PollPlan` across repeated calls. Asserts no nondeterministic ordering (e.g. HashMap iteration) leaked into the plan.
  - `plan_requests_carry_no_header_bytes`: every `PlannedRequest.request_data` for the LLY profile contains only DID + `command_suffix`, never a `6C <node> F1` header. Guards Layer 3/Layer 1 separation (the old `class2_header` leak).
  - `tcm_signal_targets_tcm_module`: the `transmission_temperature` planned request has `target == Target::Module("tcm")` and `module == tcm`, not `ecm`. Pins the Phase 5 TCM fix at the plan layer.
  - `preferred_over_is_applied_by_plan_not_gui`: with the Phase-4 policy in place, the plan for the fuel-rail capability reflects the `preferred_over` rule (generic rail preferred over range-suspect enhanced `0x163E`). This test FAILS if Wave 6 is merged before Phase 4, which is the intended tripwire -- it must be green before the GUI `.or(...)` deletion is allowed.

- Unit (GUI mapping, `main.rs` `#[cfg(test)]`):
  - `mapped_keys_are_subset_of_profile_keys`: the GUI's `apply_profile_result` key set is a subset of the LLY profile's `signals()` keys. Catches a Phase 5 key rename that would silently blank a GUI field.
  - `apply_profile_result_populates_existing_fields`: feeding a synthetic `ProfilePollResult` (VGT, injector balance, fuel rail, desired MAP, baro, TCM temp, one Class 2 DTC) produces the same `DiagnosticSnapshot` field values the pre-migration code produced for the same decoded inputs. Pins the serialized contract.

- Integration-with-mock (`tests/profile_request_parity.rs`):
  - `lly_request_stream_matches_frozen_sequence`: drive `run_poll_cycle` for `cycle in 0..=120` against a recording adapter that captures every `RoutedRequest` and returns canned bytes seeded from `tests/corpus`. Assert the recorded `(service_id, data, PhysicalTarget)` sequence equals a frozen expected sequence. HARNESS NOTE: use the Wave 9 `Elm327Adapter` + `MockTransport::expect` harness, NOT obd2-core's `MockAdapter` -- `MockAdapter::routed_request` (`mock.rs:300-313`) downgrades `Addressed` -> `Broadcast` and returns canned `[0x80,0x00]` for service `0x22`, which would fake a "profile broken" failure on addressed J1850. obd2-core is frozen, so do not "fix the mock"; standardize on the Wave 9 harness instead.
  - `parity_is_by_single_source`: assert that the request stream produced by the TUI's `session_runner` profile path and the request stream produced by the GUI both reduce to the identical `run_poll_cycle` call for the same `SelectedProfile`. Because both call the one generic `run_poll_cycle` and the architectural test (below) proves neither has another send path, parity is established without instantiating the GUI's `Session<Elm327Adapter>`-typed `LiveBackend` in a test. Document this: head-to-head UI execution is not done because `LiveBackend` is concretely typed to `Elm327Adapter`; making it generic is an optional larger refactor (see Rollback). CAVEAT: this proves both UIs take the SAME path; it does NOT prove the path is correct. Correctness of the LLY values on that shared path is guaranteed only once Phase 4 has supplied the policy and the golden corpus is green.
  - `cadence_stream_is_ordered_not_deduped`: assert the parity comparison is over the full ordered multi-cycle stream, so a frontend keeping a divergent cadence (old GUI `cycle % 12` vs the profile-owned Class 2 cadence) FAILS. A careless implementer who compares deduped capability sets would hide cadence drift; this test forbids that.

- Golden-corpus (firewall wave's corpus, run by Wave 6 CI):
  - `cargo test -p obd2-dash` replays `tests/corpus/profile/gm.gmt800.lly.class2/*.jsonl` through `run_poll_cycle` + the profile decoders and asserts identical `DecodedSignal`/`DecodedDtc`/error classification, byte-for-byte and value-for-value. Wave 6 must produce ZERO diffs here -- this is the proof that deleting the GUI inline fuel-rail preference, baro fallback, and per-node Mode 03 did not change LLY output. This proof is only meaningful if the corpus exercises `preferred_over`, baro-at-idle fallback, and the `$19` cadence; those cases are the Phase-4 gate.
  - `$19` REAL-CAPTURE coverage is NEGATIVE/NO-DATA ONLY: the corpus DTC entries from real captures are the `7F0311` / `NO DATA` responses; `raw-captures/` contains NO positive `$19`/`$59` traffic (Wave 5 and Wave 0 both confirm this). So real-capture data pins ONLY the no-data/unsupported/negative path. Assert the unified `$19` path classifies these as no-data/unsupported identically to the pre-migration GUI and TUI. Do NOT assert a positive `$19` golden from real capture -- none exists.
  - `$19` POSITIVE coverage is SYNTHETIC-ONLY: any assertion that the unified Class 2 path returns decoded DTCs on a successful response is exercised by synthetic goldens under the corpus `synthetic/` subdir, not by real capture. State this explicitly in the test module so a later reader does not assume real positive coverage exists. The `DEFAULT_CLASS2_NODES` -> discovered-modules behavior change is therefore validated on (a) the real no-data/`7F0311` negative path and (b) synthetic positive goldens only.

- Architectural (`tests/architecture_no_raw_routed_in_live.rs`):
  - `gui_main_has_no_raw_manufacturer_symbols`: read `apps/obd2-gui/src-tauri/src/main.rs` as text and assert it contains none of: `routed_request`, `raw_request`, `adapter_mut(`, `request_gm_node`, `find_lly_did`, `lly_profile_matches`, `class2_routed_request`, `class2_header`, `DEFAULT_CLASS2_NODES`, `CLASS2_DTC_ALL_REQUEST`, `CLASS2_DTC_ACTIVE_REQUEST`, `SERVICE_REPORT_DTCS_BY_STATUS`, `decode_class2_dtcs`, `PhysicalAddress::J1850`, `RoutedRequest {`.
  - `tui_session_runner_routes_profile_through_runtime`: assert `session_runner.rs` no longer calls `find_lly_did` from the selector decode path and no longer calls `class2_routed_request`/`raw_request` for `$19` outside `run_poll_cycle`. (This formalizes the Phase 3 expectation at the file boundary and prevents regression when Wave 6 lands.) NOTE: this test does NOT assert that `should_force_standard_poll`/cadence have left `session_runner.rs` -- that removal is Phase 4's, and adding the assertion here would make Wave 6 fail for a reason Wave 6 cannot fix.
  - ALLOW-LIST note encoded in the test: `gm_active::*` and `gm_evidence::*` imports are still permitted in the GUI (active tests are Phase 8). The deny-list is send/lookup symbols only. Comment this clearly so a future wave tightens it rather than a reviewer assuming the test is complete.

### Acceptance criteria

- [ ] PRECONDITION GATE: the dedicated Phase-4 poll-policy/scheduler wave has landed (`profiles/scheduler.rs` + `plan_poll_cycle` exist and own cadence/`preferred_over`/candidate-suppression/forced-standard-PIDs/backoff), and the corpus is green for `preferred_over`, baro fallback, and the `$19` cadence. Wave 6 is NOT mergeable until this is true.
- [ ] `apps/obd2-gui/src-tauri/src/main.rs` contains no raw manufacturer send (`routed_request`/`adapter_mut`), no `find_lly_did`/`lly_profile_matches`, no `gm_class2` routed/decode symbols, and no `LLY_*_DID` consts. (architectural test green)
- [ ] The GUI's only manufacturer-profile data source is `ProfileRuntime::run_poll_cycle`; the TUI's only manufacturer-profile data source is the same function. (parity tests green)
- [ ] `gm_lly_profile_enabled` is deleted and the disconnect/identity-change cache invalidation (former `main.rs:522-526`) is preserved, now keyed on `ProfileState.generation` change / `selected == None`. (cache-invalidation unit test green)
- [ ] The GUI's serialized `DiagnosticSnapshot` JSON shape is byte-identical to pre-Wave-6 for the same decoded inputs; the JS frontend is unchanged. (mapping unit test green)
- [ ] The active-test path (`request_active_test` Tauri command, `GmActiveTestCommand`, `active_tests_snapshot`, `write_active_test_evidence`) is untouched; the Tauri public contract is unchanged this wave.
- [ ] Existing LLY golden corpus stays green with zero diffs. Specifically: VGT desired/actual, all 8 injector balances, fuel-rail actual/desired (including the `preferred_over` outcome of generic rail over enhanced `0x163E`), desired MAP `0x1542`, baro `0x1251`, and TCM transmission temp `0x1940` decode to identical values; the `$19`/Mode 03 DTC path classifies the real no-data/`7F0311` captures identically.
- [ ] The unified `$19` route set equals the LLY profile's `DtcServiceDefinition.route_set`; the GUI's former per-node SAE Mode 03 to every `DEFAULT_CLASS2_NODES` entry is removed (deliberate change -- standard Mode 03 stays generic/broadcast). VALIDATION SCOPE: this `DEFAULT_CLASS2_NODES` -> discovered-modules change is validated ONLY on the real no-data/`7F0311`/negative path plus synthetic positive goldens; there is no real-capture positive `$19`/`$59` coverage, so do not claim the positive route is pinned by real capture. Called out in the commit and reflected in the corpus expectation, not silently absorbed.
- [ ] The GUI now goes through `Session::send_request`, gaining single-flight + `BusNotAvailable` handling it lacked. Verify no new spurious `BusNotAvailable` surfaces for the LLY truck on the active bus (corpus uses the resolved discovery profile).
- [ ] `cargo test -p obd2-core` and `cargo test -p obd2-dash` both green; `apps/obd2-gui` compiles with the GM send symbols no longer in scope. (`cargo test -p obd2-core` is genuinely unchanged because the integration harness uses Wave 9's `Elm327Adapter` + `MockTransport::expect`, not a modified `MockAdapter`.)
- [ ] No `match manufacturer { ... }` introduced anywhere; `run_poll_cycle` and the GUI mapping are profile-neutral (the mapping is keyed on capability strings, not on `Manufacturer::Gm`).

### Rollback notes

- Independently shippable because it changes no public Tauri command signature and no serialized JSON shape: the frontend (JS/HTML) needs zero changes. The only externally visible behavior delta is the intentional removal of the GUI's per-node Mode 03 scan, which is pinned only on the real no-data/negative `$19` path plus synthetic positive goldens (there is no real positive `$19`/`$59` capture to pin against -- so the positive route's regression risk is covered by synthetic coverage, not real capture).
- Single-commit revert: this wave is additive in `obd2-dash` (`profiles/frontend.rs` + tests) and deletion-heavy in `apps/obd2-gui/src-tauri/src/main.rs`. Reverting the GUI file restores `request_gm_node` and the local GM path; reverting `profiles/mod.rs` and deleting `frontend.rs` removes the shared loop. The TUI `session_runner` change is a thin delegation; reverting it restores the prior call site. Nothing in obd2-core changes, so there is no Layer 1 rollback. (Reverting Wave 6 does NOT revert the Phase-4 policy migration; that is a separate earlier wave and stays in place.)
- Flag-gating option if the cutover is risky: keep the deleted GUI functions behind `#[cfg(feature = "gui-legacy-gm")]` (default off) for one release, with a runtime assertion that the legacy path and `run_poll_cycle` produce identical request streams. Remove the feature once the corpus + parity tests have soaked. Do NOT leave it on by default -- a live legacy path reintroduces the exact dual-path drift this wave removes.
- The optional `LiveBackend<A: Adapter>` generic refactor (to enable true head-to-head UI execution in tests) is explicitly OUT of this wave and can be deferred indefinitely; the single-source + architectural-firewall strategy already proves both UIs take the same path. If a later wave wants it, it is a self-contained refactor of `apps/obd2-gui/src-tauri/src/main.rs` with no obd2-dash impact.
- HARD non-rollback hazard to record (headline silent-regression vector): if Wave 6 ships before the dedicated Phase-4 poll-policy/scheduler wave, the GUI (and TUI) lose fuel-rail `preferred_over`, baro-at-idle fallback, forced-standard-PIDs, candidate-DID suppression, and correct `$19` cadence with a GREEN COMPILE. There is no compile-time guard -- only the golden corpus catches it, and only if the corpus exercises those exact behaviors. Do NOT merge Wave 6 until (1) Phase 4 has re-homed `should_force_standard_poll` (`session_runner.rs:131`/`:824`), the cadence ladder (`:207`/`:216`/`:217`/`:232` and GUI `:402`), candidate suppression (`renderers.rs:564`, `tui/ui.rs:2403`), and `preferred_over` into profile-owned `scheduler.rs`/`plan_poll_cycle`, and (2) the corpus exercises `preferred_over`, baro fallback, and the `$19` cadence and is green. That ordering + corpus coverage is the only rollback insurance.

## Wave 7: Evidence + Replay v3

### Objective

Generalize the GM-only evidence record into a protocol-neutral `ProfileEvidenceRecord` (profile_id, capability_id, route, source_fields, identity confidence, manual-confirm flag, raw request/response bytes), and add six additive typed recording frames (`FRAME_PROFILE_REQUEST/RESPONSE/VALUE/DTC`, `FRAME_PASSIVE_BUS_FRAME`, `FRAME_ACTIVE_TEST_ATTEMPT`) behind a new `MAGIC_V3` recording format so old recordings still read, unknown future frames skip, and replay reproduces decoded LLY values with no hardware.

Evidence emission is reconciled with Wave 3 (see "Evidence ownership" below): Wave 3 owns the single `execute_request` emission point via its `ProfileEvidenceSink`/`DispatchEvidence` contract; Wave 7 builds `ProfileEvidenceRecord` ON TOP of that sink (it does NOT add a second emission channel inside `execute_request`). The `DomainMessage::ProfileEvidence` variant is a downstream recording-layer transport owned by Wave 7, not a competing producer.

### Evidence ownership (reconciliation with Wave 3 - read first)

There were two candidate evidence mechanisms in the draft plan: Wave 3's borrowed `ProfileEvidenceSink` trait + `DispatchEvidence` + `NullEvidenceSink`, and a Wave 7 `DomainMessage::ProfileEvidence` + `ProfileEvidenceRecord` channel. Two parallel producers invite drift and orphan Wave 3's API. This wave pins ONE design:

- Wave 3 OWNS the emission point. `ProfileRuntime::execute_request` step 7 calls `self.evidence_sink.record(&DispatchEvidence { .. })` exactly once per request. `DispatchEvidence<'_>` is a borrowed view of one dispatch outcome; `NullEvidenceSink` is the default no-op. Wave 7 does NOT redesign or re-plumb `execute_request`'s evidence step.
- Wave 7 BUILDS ON TOP of that sink. It supplies a concrete `RecordingEvidenceSink: ProfileEvidenceSink` that, on each `record()` call, projects the borrowed `DispatchEvidence` into an owned, serializable `ProfileEvidenceRecord`, then (a) appends it to JSONL via `ProfileEvidenceWriter` per evidence policy and (b) forwards `DomainMessage::ProfileEvidence(Box<ProfileEvidenceRecord>)` to the domain/recording layer.
- The `DomainMessage::ProfileEvidence` variant is therefore strictly DOWNSTREAM of the single Wave 3 sink call. It is the transport that carries the owned record from the dispatcher context to the centralized recording interception in `domain.rs`. It is not a parallel emission of evidence from `execute_request`.

Net effect: a single owner of `execute_request` evidence emission (Wave 3's sink), a single borrowed->owned conversion boundary (Wave 7's `RecordingEvidenceSink`), and no orphaned API. `DispatchEvidence` is the borrowed contract; `ProfileEvidenceRecord` is its owned/serializable projection.

### Depends on

- Wave 1 (neutral profile model): needs `ProfileId`, `CapabilityId`, `SignalDefinition`, `DtcServiceDefinition`. Evidence/frames carry `profile_id`/`capability_id` strings sourced from these. Without them the evidence fields are unpopulated.
  - BLOCKING for `SourceFieldsEvidence`: the projection below cannot be written until Wave 1 PINS the final `SourceFields` shape. Wave 1 and Wave 4 currently disagree on `SourceFields` field names/shape (part of the cross-wave PROFILE MODEL TYPE INSTABILITY issue), and there is NO dedicated `range_caveat` source field - the fuel-rail "RXD=3008" caveat is embedded inside the `rxd` string. The exact field-by-field projection (including how `range_caveat` is derived from `rxd`) is specified in "Exact APIs" and is conditional on Wave 1 finalizing `SourceFields` to the shape shown there. If Wave 1 ships a different `SourceFields`, this projection must be re-pinned before any source-fields test can compile.
- Wave 2 (session-owned selection / identity lifecycle): needs `IdentityConfidence` and the manual-confirmation flag from `VehicleContext.vin_confidence`. The plan (Phase 6, lines 700-708; Phase 2, lines 298-306) requires evidence to record identity confidence and mark manual confirmation. These do not exist before Wave 2.
- Wave 3 (central dispatcher): owns the SINGLE evidence emission point. Wave 7 requires Wave 3 to define and call, at `execute_request` step 7 ("writes evidence", plan lines 421-435):
  - `pub trait ProfileEvidenceSink: Send + Sync { fn record(&self, ev: &DispatchEvidence<'_>); }`
  - `pub struct DispatchEvidence<'a> { .. }` (borrowed view; exact shape in "Exact APIs"),
  - `pub struct NullEvidenceSink;` (default),
  - a `ProfileRuntime` that accepts a sink at construction (defaulting to `NullEvidenceSink`).
  Wave 7 implements a concrete sink against this trait. The probe-only boundary and the `probe: bool` label (plan lines 336-346) also come from Wave 3 and ride in `DispatchEvidence.probe`. If Wave 3 chose a different sink signature (for example `&mut self` or a different field set), Wave 7's `RecordingEvidenceSink` and `ProfileEvidenceRecord::from_dispatch` adapt to it; the names below are the contract Wave 7 needs.
- Wave 5 (migrate GM LLY definitions): needs `SignalDefinition.source_fields` (TXD/RXF/RXD/MTH, plan lines 222-226) and the corrected TCM route for `0x1940`. Wave 7's evidence/frames project these; the "RXD=3008 range caveat" regression guard (plan line 226) and the "TCM not ecm" guard (plan lines 677-678) are only meaningful once Wave 5 landed. Wave 5 must also confirm the EXACT in-`rxd` embedding of the 3008 caveat so `derive_range_caveat` can extract it verbatim. Wave 5 also froze the first LLY decoder golden corpus under `tests/corpus/profile/gm.gmt800.lly.class2/`, which Wave 7 must leave byte-identical.
- Wave 6 (GUI quarantine): removed `request_gm_node` and the GUI's local `GmEvidenceRecord` construction (GUI inventory section H). Wave 7 assumes evidence is no longer produced by hand in `apps/obd2-gui/src-tauri/src/main.rs`; if Wave 6 did not land, the GUI keeps writing the old GM record and the "single emission point" invariant is false.

Hard dependency callout (message-channel enrichment ownership): the recording layer needs the decoded result PLUS the resolved route, raw write/read text, and parsed response bytes. The existing recording write site (`domain.rs:288-332`) only sees scalar domain messages; the literal write at `domain.rs:320` is `writer.write_enhanced` with only `{did, module, name, unit, value}` (`EnhancedPidUpdate` also carries `confidence`/`evidence`, but NOT capability_id, route, or raw bytes - and its `module` is the literal-`"ecm"` leak, rec-inventory finding #5). That scalar message therefore CANNOT carry what v3 frames need.

This `DomainMessage` enrichment is owned UNAMBIGUOUSLY by Wave 7, not by Wave 3 or Wave 4. The reason: Wave 3's emission uses a borrowed `DispatchEvidence` delivered to an in-process sink (a function call), NOT a message on the domain channel. Nothing in Wave 3/4 needs or adds a `DomainMessage` variant. Wave 7 introduces `DomainMessage::ProfileEvidence(Box<ProfileEvidenceRecord>)` purely as the recording-layer transport, and the v3 frames are populated FROM the owned `ProfileEvidenceRecord` (which has the real `module`, e.g. `"tcm"`, the `RouteEvidence`, and the raw bytes) - NEVER from the literal-`"ecm"` scalar `EnhancedPidUpdate`. The scalar `EnhancedPidUpdate` continues to flow on its existing path to drive UI gauges; it is not the source of any v3 frame.

### Files touched

- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/recording/format.rs`
  - Add `pub const MAGIC_V3: &[u8; 8] = b"OBD2REC\x03";`.
  - Add profile frame-type constants `FRAME_PROFILE_REQUEST=0x10 .. FRAME_ACTIVE_TEST_ATTEMPT=0x15` (do NOT reuse `0x01..0x05`).
  - Add `pub const MAX_PROFILE_FRAME_BYTES: usize = 16 * 1024 * 1024;` (desync/oversize guard).
  - Add `RecordingFrame::write_to_v3<W: Write>` and extend `RecordingFrame::read_from<R: Read>(reader, version)` with a `version == 3` arm using a `u32` raw-length envelope. Leave the v1/v2 arms and `write_to` byte-for-byte unchanged.
  - Add `write_file_header_v3` (writes `MAGIC_V3`); extend `read_file_header` with a `MAGIC_V3 -> 3` arm (keep the `else` reject).
  - Extend `SessionHeader` with `#[serde(default)] pub profile_id: Option<String>` and `#[serde(default)] pub identity_confidence: Option<String>`.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/recording/writer.rs`
  - Add `RecordingWriter::new_v3(...)` (writes the v3 header) and `write_profile_frame(&mut self, offset_ms: u32, frame: &RecordingFrame)`; route profile-frame writes through `write_to_v3`. Keep `new(...)` and the v2 `write_*` methods unchanged.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/recording/reader.rs`
  - No loop-shape change (`read_from_reader` already passes `version`); add a bound check so a `version==3` frame with `raw_len > MAX_PROFILE_FRAME_BYTES` triggers a graceful stop (return frames-so-far) instead of allocating/erroring the whole file.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/recording/replay.rs`
  - Add `is_profile_request_frame/is_profile_response_frame/is_profile_value_frame/is_profile_dtc_frame/is_passive_bus_frame/is_active_test_attempt_frame` predicates. No change to `next_frames`.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/recording/index.rs`
  - `SessionEntry` gains `#[serde(default)] pub profile_id: Option<String>`. Additive, serde-default so old `sessions.json` still loads.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/recording/mod.rs`
  - Module doc note for v3; re-export the new predicates if needed.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/domain.rs`
  - Add the `DomainMessage::ProfileEvidence(Box<ProfileEvidenceRecord>)` variant (this enrichment is owned here, by Wave 7).
  - Recording interception (`update()`, lines 288-332): when the active writer is v3 AND the message is `DomainMessage::ProfileEvidence(rec)`, write `FRAME_PROFILE_REQUEST` + `FRAME_PROFILE_RESPONSE` + (`FRAME_PROFILE_VALUE` | `FRAME_PROFILE_DTC`) populated FROM `rec` (so `module` is `rec.module`, route is `rec.route`, bytes are `rec.parsed_response_bytes` - never the literal-`"ecm"` scalar). Suppress the legacy `write_enhanced` (line 320) for profile-sourced values when v3 is active, to avoid double-recording (FRAME_ENHANCED AND FRAME_PROFILE_VALUE for the same read). Standard `FRAME_PID/VOLTAGE/O2` writes are unchanged.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/main.rs`
  - Replay dispatch (lines 665-702): add `else if` arms for `is_profile_value_frame` -> `Message::EnhancedPidUpdate{..}`, `is_profile_dtc_frame` -> `Message::DtcUpdate(..)` + `Message::DiagnosticScanUpdate(..)`, `is_active_test_attempt_frame` -> `Message::ActiveTestResult(..)`. REQUEST/RESPONSE/PASSIVE frames are evidence-only on replay (surface in Raw/Evidence view; they do not drive gauges). Unknown frame types fall through the if/else and are dropped without panic.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/ai/summary.rs`
  - Line 206 reads `decode_dtc_code` (FRAME_DTC only). Add a branch for `FRAME_PROFILE_DTC` so v3 recordings' DTCs appear in summaries. Without this, v3 DTCs silently vanish from AI summaries.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/recording.rs`
  - The six typed payload structs + `RouteEvidence` + `SourceFieldsEvidence` (incl. `SourceFieldsEvidence::project` and `derive_range_caveat`) + per-type `to_frame`/`from_frame` codecs. (Plan target layout line 113.)
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/evidence.rs`
  - `ProfileEvidenceRecord`, `DecodedEvidence`, `ProfileEvidenceError`, `VehicleIdentityEvidence`, `ProfileEvidenceWriter`, `ProfileEvidenceRecord::from_dispatch(&DispatchEvidence, ..)`, the `RecordingEvidenceSink` impl of Wave 3's `ProfileEvidenceSink`, and `From<GmEvidenceRecord>`. (Plan target layout line 112.)
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/mod.rs`
  - `pub mod evidence; pub mod recording;`.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/runtime.rs` (created in Wave 3)
  - NO change to `execute_request`'s body or to its single `evidence_sink.record(&DispatchEvidence)` call - that emission point is owned by Wave 3 and is the single owner of execute_request evidence emission. Wave 7's only edit here is to inject the concrete sink: swap the `ProfileRuntime` construction from `NullEvidenceSink` to `RecordingEvidenceSink` (below). If the construction site for `ProfileRuntime` lives outside `runtime.rs` (for example in `session_runner.rs`), this is a one-line sink-injection change at that existing site, NOT a re-plumb of `execute_request` and NOT a new `session_runner` rewire of `read_enhanced_target`/`find_lly_did`/`raw_request` (those rewires belong to whichever wave owns them per the cross-wave SESSION_RUNNER ownership decision). No manufacturer branch added.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/gm_evidence.rs`
  - Mark `GmEvidenceRecord`/`GmEvidenceWriter` `#[deprecated(note = "use profiles::evidence::ProfileEvidenceRecord")]`. Keep them compiling for probe examples during the deprecation window; no deletion this wave.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/replay_compatibility.rs` (integration).
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/profile_evidence.rs` (integration; uses `MockAdapter`).
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus/recording/` -> committed `v2-baseline.obd2rec`, `v3-lly-profile.obd2rec`, `v3-unknown-frame.obd2rec`, plus `*.expected.json` message lists.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus/evidence/gm.gmt800.lly.class2/*.jsonl` -> expected `ProfileEvidenceRecord`s seeded from the Wave 5 captures.
- No DELETE this wave. `gm_evidence.rs` deletion is a later cleanup once probes adopt `ProfileEvidenceWriter`.

### Exact APIs

Wave 3 contract consumed by Wave 7 (defined in Wave 3 - shown for reference only; Wave 7 does NOT define these, it implements against them):

```rust
// profiles/runtime.rs (Wave 3). Borrowed view of one dispatch outcome.
pub struct DispatchEvidence<'a> {
    pub profile_id: Option<&'a str>,
    pub capability_id: Option<&'a str>,
    pub module: &'a str,                 // RouteDefinition.module (e.g. "tcm"); never literal "ecm"
    pub bus: Option<&'a str>,
    pub route: &'a obd2_core::vehicle::PhysicalAddress, // resolved address (session::resolve_request)
    pub request_service: u8,
    pub request_data: &'a [u8],
    pub raw_write_text: &'a str,
    pub raw_read_text: &'a str,
    pub parsed_response_bytes: &'a [u8], // post-skip, echo-stripped (codec.rs:234)
    pub decoder_id: &'a str,
    pub decoded: Option<DecodedView<'a>>,// borrowed decoded outcome (value/dtcs/active-test)
    pub decode_confidence: Option<&'a str>,
    pub source_fields: Option<&'a SourceFields>, // borrowed from the matched SignalDefinition
    pub probe: bool,
    pub error: Option<&'a ProfileEvidenceError>, // or Wave 3's dispatch-error type
}

pub trait ProfileEvidenceSink: Send + Sync {
    fn record(&self, ev: &DispatchEvidence<'_>);
}
pub struct NullEvidenceSink; // default no-op; ProfileRuntime::new takes Box<dyn ProfileEvidenceSink>
```

New recording-format surface (`recording/format.rs`), all additive:

```rust
pub const MAGIC_V3: &[u8; 8] = b"OBD2REC\x03";

pub const FRAME_PROFILE_REQUEST: u8 = 0x10;
pub const FRAME_PROFILE_RESPONSE: u8 = 0x11;
pub const FRAME_PROFILE_VALUE: u8 = 0x12;
pub const FRAME_PROFILE_DTC: u8 = 0x13;
pub const FRAME_PASSIVE_BUS_FRAME: u8 = 0x14;
pub const FRAME_ACTIVE_TEST_ATTEMPT: u8 = 0x15;

/// Hard ceiling on a single v3 frame payload to bound allocation and detect desync.
pub const MAX_PROFILE_FRAME_BYTES: usize = 16 * 1024 * 1024;

impl RecordingFrame {
    /// v3 on-disk frame: [type u8][offset_ms u32 LE][pid_code u8][value f64 LE][raw_len u32 LE][raw].
    /// Same fixed 14-byte body as v2; the ONLY change is a u32 raw length (vs u8) so profile
    /// payloads larger than 255 bytes (multi-frame J1850 response + decoder metadata) fit.
    pub fn write_to_v3<W: std::io::Write>(&self, writer: &mut W) -> std::io::Result<()>;

    // read_from gains a `version == 3` arm; signature unchanged:
    // pub fn read_from<R: Read>(reader: &mut R, version: u8) -> io::Result<Option<Self>>;
}

pub fn write_file_header_v3<W: std::io::Write>(
    writer: &mut W,
    header: &SessionHeader,
) -> std::io::Result<()>;
// read_file_header gains a `MAGIC_V3 -> version 3` arm; signature unchanged.
```

`SessionHeader` and `SessionEntry` additive fields (both `#[serde(default)]`):

```rust
// format.rs SessionHeader:
pub profile_id: Option<String>,
pub identity_confidence: Option<String>,

// index.rs SessionEntry:
pub profile_id: Option<String>,
```

Writer (`recording/writer.rs`):

```rust
impl RecordingWriter {
    pub fn new_v3(
        recordings_dir: &Path,
        session_id: &str,
        vin: Option<String>,
        vehicle_name: Option<String>,
        poll_interval_ms: u64,
        profile_id: Option<String>,
        identity_confidence: Option<String>,
    ) -> std::io::Result<Self>;

    pub fn write_profile_frame(
        &mut self,
        offset_ms: u32,
        frame: &RecordingFrame, // built via ProfileValueFrame::to_frame etc.
    ) -> std::io::Result<()>;
}
```

Replay predicates (`recording/replay.rs`):

```rust
impl ReplayController {
    pub fn is_profile_request_frame(frame: &RecordingFrame) -> bool;   // == FRAME_PROFILE_REQUEST
    pub fn is_profile_response_frame(frame: &RecordingFrame) -> bool;  // == FRAME_PROFILE_RESPONSE
    pub fn is_profile_value_frame(frame: &RecordingFrame) -> bool;     // == FRAME_PROFILE_VALUE
    pub fn is_profile_dtc_frame(frame: &RecordingFrame) -> bool;       // == FRAME_PROFILE_DTC
    pub fn is_passive_bus_frame(frame: &RecordingFrame) -> bool;       // == FRAME_PASSIVE_BUS_FRAME
    pub fn is_active_test_attempt_frame(frame: &RecordingFrame) -> bool;// == FRAME_ACTIVE_TEST_ATTEMPT
}
```

Typed frame payloads (`profiles/recording.rs`). Each is serialized with `serde_json` (already a dependency; no new crate) into `RecordingFrame.raw_bytes`; `to_frame` sets `frame_type`, `offset_ms`, and `value` (scalar VALUE only) with everything canonical in the JSON payload:

```rust
/// Protocol-agnostic address evidence. Mirrors obd2_core::vehicle::PhysicalAddress
/// (J1850 { node, header:[u8;3] }, Can11Bit { request_id, response_id },
///  Can29Bit { request_id, response_id }, J1939) but is serializable and NOT node-only.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteEvidence {
    J1850 { node: u8, header: Vec<u8> },
    Can11 { request_id: u16, response_id: u16 },
    Can29 { request_id: u32, response_id: u32 },
    Broadcast,
}

/// Vendor-auditable source fields (ScanGauge TXD/RXF/RXD/MTH + provenance), projected
/// from SignalDefinition.source_fields (Wave 5). MUST retain the fuel-rail RXD=3008 caveat.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct SourceFieldsEvidence {
    pub txd: Option<String>,
    pub rxf: Option<String>,
    pub rxd: Option<String>,
    pub rxd_width: Option<u8>,
    pub raw_mth: Option<String>,
    pub source_url: Option<String>,
    pub document_id: Option<String>,
    pub range_caveat: Option<String>, // e.g. "RXD=3008 range-suspect"
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DecodedDtcEvidence {
    pub code: String,
    pub status_raw: Vec<u8>,        // NOT a single GM u8: CAN/UDS DTCs carry >1 status byte
    pub status_flags: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProfileRequestFrame {
    pub seq: u64,                   // monotonic per recording; links REQUEST->RESPONSE->VALUE
    pub profile_id: String,
    pub capability_id: String,
    pub module: String,            // from RouteDefinition.module, never a hardcoded "ecm"
    pub bus: Option<String>,
    pub address: RouteEvidence,
    pub service_id: u8,
    pub request_data: Vec<u8>,
    pub probe: bool,
    pub raw_write_text: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProfileResponseFrame {
    pub seq: u64,
    pub parsed_response_bytes: Vec<u8>, // post-skip, echo-stripped adapter output (core inv #10)
    pub raw_read_text: Option<String>,
    pub error: Option<crate::profiles::evidence::ProfileEvidenceError>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProfileValueFrame {
    pub seq: u64,
    pub capability_id: String,
    pub key: String,               // SignalDefinition.key
    pub did: Option<u16>,          // for replay -> Message::EnhancedPidUpdate mapping
    pub module: String,
    pub label: String,
    pub value: f64,
    pub unit: String,
    pub confidence: Option<String>,
    pub source_fields: Option<SourceFieldsEvidence>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProfileDtcFrame {
    pub seq: u64,
    pub service_key: String,       // DtcServiceDefinition.key (e.g. gm class2 all/active)
    pub module: String,
    pub dtcs: Vec<DecodedDtcEvidence>,
    pub raw_response: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PassiveBusFrame {
    pub bus: Option<String>,
    pub address: RouteEvidence,
    pub raw_frame: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ActiveTestAttemptFrame {
    pub seq: u64,
    pub profile_id: String,
    pub test_key: String,
    pub command: String,
    pub accepted: bool,
    pub status: String,
    pub evidence_path: Option<String>,
}

impl ProfileValueFrame {
    pub fn to_frame(&self, offset_ms: u32) -> RecordingFrame; // FRAME_PROFILE_VALUE, value=self.value
    pub fn from_frame(frame: &RecordingFrame) -> Option<Self>; // guards frame_type, serde_json::from_slice
}
// identical to_frame/from_frame pairs for the other five payload types.
```

SourceFields projection (`profiles/recording.rs`) - resolves the "projection cannot be written until SourceFields is pinned" blocker. This is the EXACT field-by-field mapping; it is valid only against the pinned Wave 1 `SourceFields` shown here. Coordinate with Wave 1 before implementing; if Wave 1's final field names differ, update the right-hand side only (the `SourceFieldsEvidence` shape stays fixed):

```rust
// Pinned Wave 1 SourceFields shape REQUIRED by this projection (Wave 1 owns final authority;
// Wave 4 must conform to these names, not redefine them):
//   pub struct SourceFields {
//       pub txd: String,            // ScanGauge TXD request command, e.g. "07E0221543"
//       pub rxf: String,            // RXF response-format spec
//       pub rxd: String,            // RXD response-decode spec; MAY embed a range caveat VERBATIM
//       pub mth: String,            // MTH math expression, stored RAW (never pre-evaluated)
//       pub source_ref: Option<String>,  // citation URL / forum permalink
//       pub document_id: Option<String>, // stable document identifier
//   }

impl SourceFieldsEvidence {
    /// Pure field-by-field projection. NO MTH evaluation, NO rxd reformatting/trim/case-change.
    /// The RXD caveat survives TWICE: verbatim in `rxd` (full string) AND extracted verbatim
    /// into `range_caveat`. This is what makes the RXD=3008 regression guard (plan line 226) hold.
    pub fn project(sf: &SourceFields) -> Self {
        Self {
            txd:         Some(sf.txd.clone()),
            rxf:         Some(sf.rxf.clone()),
            rxd:         Some(sf.rxd.clone()),                 // full string, byte-identical
            rxd_width:   parse_rxd_width(&sf.rxf, &sf.rxd),    // derived; None if unparseable
            raw_mth:     Some(sf.mth.clone()),                 // MTH copied RAW (hence "raw_mth")
            source_url:  sf.source_ref.clone(),                // source_ref -> source_url
            document_id: sf.document_id.clone(),
            range_caveat: derive_range_caveat(&sf.rxd),        // verbatim substring or None
        }
    }
}

/// There is NO dedicated caveat source field. The documented fuel-rail caveat (the
/// "RXD=3008 range-suspect" note for 0x163E) is embedded inside the `rxd` string by Wave 5.
/// This extracts it VERBATIM (no normalization). When the documented `3008` marker is present,
/// returns Some(<verbatim caveat clause exactly as stored in rxd>); otherwise None. The full
/// rxd is always retained in SourceFieldsEvidence.rxd, so the caveat survives even if the marker
/// format changes. Coordinate the exact embedding format with Wave 5.
fn derive_range_caveat(rxd: &str) -> Option<String>;

/// Derives the response width (bytes) from the RXF/RXD pair; None when not derivable.
fn parse_rxd_width(rxf: &str, rxd: &str) -> Option<u8>;
```

Generalized evidence (`profiles/evidence.rs`), the Phase 6 schema (plan lines 689-709):

```rust
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct VehicleIdentityEvidence {
    pub vin: Option<String>,
    pub year: Option<u16>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub engine: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DecodedEvidence {
    Value { signal: String, value: f64, unit: String }, // raw bytes live in parsed_response_bytes
    Dtcs { records: Vec<super::recording::DecodedDtcEvidence> },
    ActiveTest { test_id: String, command: String, accepted: bool, status: String },
    Empty,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileEvidenceErrorKind {
    NoData, UnsupportedService, UnsupportedSubfunction, MalformedPayload,
    StaleResponse, NegativeResponse, Transport, Decode, Adapter,
    UnverifiedCommand, InvalidCommand, ProfileNotSelected, CapabilityNotOwned,
    StaleProfileGeneration,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ProfileEvidenceError { pub kind: ProfileEvidenceErrorKind, pub detail: String }

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ProfileEvidenceRecord {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub adapter_port: Option<String>,
    pub protocol: Option<String>,
    pub vehicle: Option<VehicleIdentityEvidence>,
    pub identity_confidence: Option<String>, // from VehicleContext.vin_confidence (Wave 2)
    pub manual_confirmation: bool,
    pub profile_id: Option<String>,
    pub capability_id: Option<String>,
    pub module: String,
    pub bus: Option<String>,
    pub route: super::recording::RouteEvidence, // replaces node:u8 + request_header:[u8;3]
    pub request_service: u8,
    pub request_data: Vec<u8>,
    pub raw_adapter_write_text: String,
    pub raw_adapter_read_text: String,
    pub parsed_response_bytes: Vec<u8>,
    pub decoder_id: String,
    pub decoded: Option<DecodedEvidence>,
    pub decode_confidence: Option<String>,
    pub source_fields: Option<super::recording::SourceFieldsEvidence>,
    pub probe: bool,
    pub error: Option<ProfileEvidenceError>,
}

impl ProfileEvidenceRecord {
    /// PRIMARY construction path: project the borrowed Wave 3 DispatchEvidence into the owned,
    /// serializable record. This is the single borrowed->owned boundary. source_fields is filled
    /// via `ev.source_fields.map(SourceFieldsEvidence::project)`; route from `ev.route`; module
    /// from `ev.module` (the real RouteDefinition module, never literal "ecm").
    pub fn from_dispatch(
        ev: &crate::profiles::runtime::DispatchEvidence<'_>,
        vehicle: Option<VehicleIdentityEvidence>,
        identity_confidence: Option<String>,
        manual_confirmation: bool,
    ) -> Self;

    // Secondary builders, retained for probe examples and unit tests (NOT used by execute_request):
    pub fn routed_request_outcome(
        profile_id: Option<String>,
        capability_id: Option<String>,
        module: impl Into<String>,
        route: super::recording::RouteEvidence,
        request_service: u8,
        request_data: Vec<u8>,
        decoder_id: impl Into<String>,
    ) -> Self;
    pub fn with_adapter_context(self, adapter_port: Option<String>, protocol: Option<String>) -> Self;
    pub fn with_identity(self, vehicle: Option<VehicleIdentityEvidence>,
                         identity_confidence: Option<String>, manual_confirmation: bool) -> Self;
    pub fn with_raw_text(self, write: impl Into<String>, read: impl Into<String>) -> Self;
    pub fn with_response_bytes(self, parsed_response_bytes: Vec<u8>) -> Self;
    pub fn with_decoded(self, decoded: DecodedEvidence, confidence: Option<String>) -> Self;
    pub fn with_source_fields(self, source_fields: super::recording::SourceFieldsEvidence) -> Self;
    pub fn as_probe(self) -> Self;
    pub fn with_error(self, kind: ProfileEvidenceErrorKind, detail: impl Into<String>) -> Self;
}

// Upgrade path so existing GM JSONL and probe examples migrate incrementally.
impl From<crate::gm_evidence::GmEvidenceRecord> for ProfileEvidenceRecord {
    // node + request_header -> RouteEvidence::J1850 { node, header }; profile_id/capability_id = None.
}

pub struct ProfileEvidenceWriter { /* path + BufWriter<File>, JSONL, same shape as GmEvidenceWriter */ }
impl ProfileEvidenceWriter {
    pub fn create(path: impl AsRef<std::path::Path>) -> std::io::Result<Self>;
    pub fn create_raw_capture(prefix: &str) -> std::io::Result<Self>; // raw-captures/{prefix}-{ts}.jsonl
    pub fn path(&self) -> &std::path::Path;
    pub fn append(&mut self, record: &ProfileEvidenceRecord) -> std::io::Result<()>;
    pub fn flush(&mut self) -> std::io::Result<()>;
}

/// Wave 7's concrete sink. This is the ONLY place that turns the Wave 3 borrowed DispatchEvidence
/// into an owned ProfileEvidenceRecord and dispatches it. It is injected into ProfileRuntime in
/// place of NullEvidenceSink. It is NOT a second emission point: it only runs when Wave 3's
/// execute_request calls `record()`.
pub struct RecordingEvidenceSink {
    domain_tx: std::sync::mpsc::Sender<crate::domain::DomainMessage>, // app's domain channel sender
    writer: Option<std::sync::Mutex<ProfileEvidenceWriter>>,          // JSONL sink, per policy
    policy: crate::profiles::EvidencePolicy,                          // gates JSONL writes
    identity: std::sync::Mutex<Option<(VehicleIdentityEvidence, Option<String>, bool)>>, // cached id ctx
}
impl crate::profiles::runtime::ProfileEvidenceSink for RecordingEvidenceSink {
    fn record(&self, ev: &crate::profiles::runtime::DispatchEvidence<'_>) {
        // 1. borrowed -> owned
        let (veh, conf, manual) = self.identity.lock().unwrap().clone().map(|(v,c,m)| (Some(v),c,m))
            .unwrap_or((None, None, false));
        let rec = ProfileEvidenceRecord::from_dispatch(ev, veh, conf, manual);
        // 2. JSONL append per evidence policy
        if let Some(w) = &self.writer { /* if policy permits */ let _ = w.lock().unwrap().append(&rec); }
        // 3. forward to the recording/domain layer (transport only; downstream of the single emit)
        let _ = self.domain_tx.send(crate::domain::DomainMessage::ProfileEvidence(Box::new(rec)));
    }
}
impl RecordingEvidenceSink {
    pub fn new(domain_tx: std::sync::mpsc::Sender<crate::domain::DomainMessage>,
               writer: Option<ProfileEvidenceWriter>,
               policy: crate::profiles::EvidencePolicy) -> Self;
    /// Updates the cached identity context when Wave 2 reports an identity/confidence change.
    pub fn set_identity(&self, vehicle: VehicleIdentityEvidence,
                        identity_confidence: Option<String>, manual_confirmation: bool);
}
```

Note on `&self` vs `&mut self`: Wave 3's trait method is `record(&self, ..)` so a single shared sink can be held by the runtime. `RecordingEvidenceSink` therefore uses interior mutability (`Mutex`) for the writer and a cloneable `Sender` for the channel. If Wave 3 chose `&mut self`, drop the `Mutex` and hold the writer directly; the rest is unchanged.

obd2-core references grounding the above (from inventory): `RouteEvidence` mirrors `obd2_core::vehicle::PhysicalAddress` (`vehicle/mod.rs:33`); `parsed_response_bytes` is the post-skip output of `decode_elm_response_payload_for_command` (`protocol/codec.rs:234`), NOT `decode_frame` headers-on output; `identity_confidence` derives from `VehicleContext.vin_confidence` (plan Core Types). Wave 3's `execute_request` places the resolved `PhysicalAddress` (from `session::resolve_request`, `session/mod.rs:1164`) into `DispatchEvidence.route`; the Wave 7 sink projects it into `ProfileEvidenceRecord.route`, so display module and routing module agree (fixes the `0x1940` TCM-vs-ecm disagreement, dash-inventory section 2).

### Tests

Unit (`recording/format.rs`):
- `test_v3_header_roundtrip` - `write_file_header_v3` -> `read_file_header` returns version 3 and a `SessionHeader` with `profile_id`/`identity_confidence` preserved.
- `test_v3_frame_large_raw_roundtrip` - a frame with `raw_bytes.len() == 4096` survives `write_to_v3` -> `read_from(.., 3)`. Proves the u32 length defeats the 255-byte cap.
- `test_v3_unknown_frame_skips_large_payload` - frame_type `0x7F` with a 2 KiB payload followed by a `FRAME_PID`; both read, the PID is intact. Proves forward-skip holds at u32 width.
- `test_v3_oversize_frame_stops_gracefully` - a v3 frame claiming `raw_len > MAX_PROFILE_FRAME_BYTES` makes the read loop stop and return prior frames rather than panic/OOM.
- `test_v2_path_unchanged` - the existing `test_frame_roundtrip_no_raw` (15 bytes), `test_frame_roundtrip_with_raw` (17 bytes), `test_v1_frame_read`, `test_unknown_frame_type_roundtrip`, `test_header_roundtrip` (version==2), `test_full_file_roundtrip_all_frame_types` (len==5) stay green WITHOUT edits. Asserted by leaving `write_to`/v1+v2 `read_from` arms untouched.

Unit (`profiles/recording.rs`):
- `test_profile_value_frame_roundtrip` - `ProfileValueFrame::to_frame` -> `from_frame` is identity, including `did`, `module`, `value`, `unit`, `confidence`, `source_fields`.
- `test_profile_value_frame_records_tcm_module` - a `0x1940` transmission-temp value frame records `module == "tcm"`, not `"ecm"`. Pins the Wave 5 leak fix (plan line 678); fails if any code bakes ecm into the recording.
- `test_profile_dtc_frame_preserves_status` - `FRAME_PROFILE_DTC` carries the GM Class 2 status bytes structurally in `DecodedDtcEvidence.status_raw` and decodes back identical; value:f64 is unused.
- `test_profile_dtc_frame_long_code` - a code/status combination that would overflow the legacy `FRAME_DTC` 8-char `f64` hack roundtrips losslessly via raw bytes.
- `test_request_response_value_seq_link` - request/response/value frames written with the same `seq` reconstruct one chain.
- `test_source_fields_projection_field_by_field` - `SourceFieldsEvidence::project` maps a fully-populated pinned `SourceFields`: `txd/rxf/rxd` copied byte-identical, `mth -> raw_mth`, `source_ref -> source_url`, `document_id` preserved. Guards the correction-2 projection contract.
- `test_source_fields_rxd_caveat_preserved` - a fuel-rail (`0x163E`) `SourceFields` whose `rxd` embeds the documented "3008" caveat projects to `range_caveat == Some("RXD=3008 ...")` VERBATIM AND retains the full caveat inside `rxd` (plan line 226 regression guard). A second case with no marker yields `range_caveat == None` while `rxd` is still the full string.

Unit (`profiles/evidence.rs`):
- `test_profile_evidence_has_profile_and_capability` - record carries `profile_id` and `capability_id` (plan line 842).
- `test_profile_evidence_marks_manual_confirmation` - `manual_confirmation == true` when identity was manually confirmed (plan line 844).
- `test_profile_evidence_marks_probe_traffic` - `as_probe()` sets `probe == true` (plan line 344).
- `test_from_dispatch_projects_route_and_module` - `ProfileEvidenceRecord::from_dispatch` copies `DispatchEvidence.module` ("tcm") and `DispatchEvidence.route` into the owned record without mutation, and projects `source_fields` via `SourceFieldsEvidence::project`. Pins the single borrowed->owned boundary.
- `test_from_gm_evidence_record` - `From<GmEvidenceRecord>` maps `node`+`request_header` -> `RouteEvidence::J1850` and preserves `request_data`/`parsed_response_bytes`; round-trips a real GM JSONL line from `raw-captures/`.
- `test_route_evidence_can_variants_serialize` - `RouteEvidence::Can11`/`Can29` serialize and deserialize. Proves the schema is not J1850-locked (rec-inventory finding #6).
- `test_decoded_value_keeps_multibyte_raw` - injector-balance 6-byte raw lands in `parsed_response_bytes`, not truncated into a scalar (rec-inventory: `GmDecodedEvidence.raw:u32` cannot hold it).

Integration-with-mock (`tests/profile_evidence.rs`):
- Precondition test `test_mock_adapter_honors_addressed_routed_requests` - assert `MockAdapter` overrides `Adapter::routed_request` for `PhysicalTarget::Addressed` (core-inventory failure-mode #1). If it falls through to the default impl, every J1850-addressed read returns `Obd2Error::Adapter` and produces a fake regression; this test catches that before the corpus runs. (NOTE: the cross-wave MockAdapter limitation is unresolved at plan level - if the MockAdapter addressed-J1850 fix is not owned by an obd2-core wave, this suite must move to the Wave 9 `Elm327Adapter`+`MockTransport::expect` harness; this is a standing risk, not resolved by Wave 7.)
- `test_single_evidence_emission_point` - drive one `execute_request` with `NullEvidenceSink`: assert ZERO `DomainMessage::ProfileEvidence` sent and ZERO JSONL lines written. Then drive the same request with `RecordingEvidenceSink`: assert EXACTLY ONE `DomainMessage::ProfileEvidence` and (policy permitting) exactly one JSONL line. Guards against the two-mechanism drift the OWL review flagged - there is one emission point (Wave 3's sink call) and one downstream transport, never a parallel channel.
- `test_dispatcher_emits_evidence_with_profile_id` - drive an LLY VGT (`0x1543`) read through `ProfileRuntime::execute_request` with a `RecordingEvidenceSink` (or a capturing test sink) on a mock seeded from `1GTHK29294E391526-...-20260627T032948.obd2raw`; assert the forwarded `ProfileEvidenceRecord` has `profile_id == "gm.gmt800.lly.class2"`, a non-None `capability_id`, `module == "ecm"`, byte-identical `request_data` and `parsed_response_bytes`, and populated `source_fields`.
- `test_dispatcher_evidence_tcm_route` - same path for `0x1940`; assert `module == "tcm"` and `route == RouteEvidence::J1850 { node: 0x18, header: vec![0x6C,0x18,0xF1] }`.
- `test_probe_request_labeled_probe` - a probe-only execution writes evidence with `probe == true` (sourced from `DispatchEvidence.probe`).

Integration replay (`tests/replay_compatibility.rs`):
- `test_old_v2_recording_replays_identically` - load committed `tests/corpus/recording/v2-baseline.obd2rec`, run the replay-dispatch mapping, assert the produced `DomainMessage` list equals `v2-baseline.expected.json`. Pins "old recordings still replay identically" (plan line 543).
- `test_v3_profile_value_replays_to_enhanced_update` - load `v3-lly-profile.obd2rec`; a `FRAME_PROFILE_VALUE` replays into `Message::EnhancedPidUpdate` with identical `did`/`module`/`value`/`unit` as the live read.
- `test_v3_profile_dtc_replays_to_dtc_update` - a `FRAME_PROFILE_DTC` replays into `Message::DtcUpdate` + `Message::DiagnosticScanUpdate` with GM Class 2 status preserved.
- `test_v3_unknown_frame_does_not_crash_replay` - `v3-unknown-frame.obd2rec` (unknown type `0x7F`, large payload, surrounded by known frames); replay reads, skips the unknown, and still emits the known messages. No panic (plan line 847).
- `test_v3_recording_no_duplicate_enhanced` - recording one LLY enhanced value in v3 mode writes exactly one `FRAME_PROFILE_VALUE` and zero `FRAME_ENHANCED` for that value. Also asserts the v3 frame's `module` came from `ProfileEvidenceRecord.module`, not from the scalar `EnhancedPidUpdate` (so the literal-"ecm" leak cannot reach the frame).

Golden-corpus:
- `test_lly_decoder_corpus_unchanged` - re-run the Wave 5 decoder goldens under `tests/corpus/profile/gm.gmt800.lly.class2/` and assert zero diffs. Wave 7 touches no decoder, so this must pass byte-for-byte; it is the firewall that proves Evidence/Replay v3 changed nothing about decoded LLY output (plan lines 545-547).
- `test_profile_evidence_corpus` - replay seeded captures through the dispatcher with `RecordingEvidenceSink` and diff the forwarded `ProfileEvidenceRecord`s against committed `tests/corpus/evidence/gm.gmt800.lly.class2/*.jsonl`.

Architectural:
- `test_recording_layer_is_profile_neutral` - `src/recording/*` and `src/profiles/recording.rs` contain no reference to `gm_enhanced`, `gm_class2`, `find_lly_did`, or LLY DID literals; they carry opaque `profile_id`/`capability_id`/`module` strings only (plan lines 358-366, Layer 2 has no manufacturer knowledge).
- `test_replay_dispatch_constructs_no_request_bytes` - the `main.rs` replay arms call only `state.update(Message::..)` and never `routed_request`/`raw_request`/`adapter_mut` (extends the Wave 3 architectural import test, plan line 346).
- `test_profile_evidence_record_no_node_only_field` - compile-time guard that the generalized record uses `RouteEvidence` (not a bare `node: u8` + `[u8;3]`), so CAN profiles are representable.
- `test_no_second_evidence_emitter` - source-level guard that `execute_request` is the only function that calls `ProfileEvidenceSink::record`, and that `DomainMessage::ProfileEvidence` is only SENT from inside `RecordingEvidenceSink::record` (grep-style assertion over `src/profiles/` and `src/`). Locks the single-owner decision so a future edit cannot quietly add a parallel evidence path.

### Acceptance criteria

- [ ] Existing LLY decoder golden corpus (Wave 5) stays green with ZERO diffs (`test_lly_decoder_corpus_unchanged`). Wave 7 changes no decoded value.
- [ ] All pre-existing `recording/format.rs` v1/v2 tests pass unedited (15-byte, 17-byte, v1 read, unknown-type skip, header version==2, full-file len==5).
- [ ] Old v1/v2 `.obd2rec` recordings replay to an identical `DomainMessage` sequence (`test_old_v2_recording_replays_identically`).
- [ ] v3 frames roundtrip with payloads > 255 bytes; an unknown v3 frame type with a large payload is skipped, not desynced; oversize length stops gracefully.
- [ ] `FRAME_PROFILE_VALUE`/`DTC`/`ACTIVE_TEST_ATTEMPT` replay into the same UI-driving messages as live execution; REQUEST/RESPONSE/PASSIVE frames are evidence-only and never crash replay.
- [ ] EXACTLY ONE evidence emission point: `execute_request` calls Wave 3's `ProfileEvidenceSink::record` once per request; `DomainMessage::ProfileEvidence` is sent only from `RecordingEvidenceSink::record` (`test_single_evidence_emission_point`, `test_no_second_evidence_emitter`). No parallel evidence channel exists; Wave 3's sink API is not orphaned.
- [ ] `DomainMessage::ProfileEvidence` enrichment is owned by Wave 7, and v3 frames are populated from `ProfileEvidenceRecord` (real `module`, `route`, raw bytes), NEVER from the literal-`"ecm"` scalar `EnhancedPidUpdate` (`test_v3_recording_no_duplicate_enhanced`).
- [ ] `ProfileEvidenceRecord` carries `profile_id`, `capability_id`, `module`, `route` (RouteEvidence), `identity_confidence`, `manual_confirmation`, `probe`, `source_fields`, `decoder_id`, `request_data`, `parsed_response_bytes`, raw write/read text, decoded value/DTCs, and error classification.
- [ ] `SourceFieldsEvidence::project` is implemented field-by-field against the pinned Wave 1 `SourceFields` (`mth -> raw_mth`, `source_ref -> source_url`, `document_id` preserved) and `derive_range_caveat` extracts the RXD=3008 caveat VERBATIM while `rxd` retains the full string (`test_source_fields_projection_field_by_field`, `test_source_fields_rxd_caveat_preserved`). Blocked until Wave 1 finalizes `SourceFields`.
- [ ] TCM signal `0x1940` records and evidences `module == "tcm"` and node `0x18`; no recording bakes `"ecm"` for it.
- [ ] Multi-byte raw responses (injector balance) are preserved in `parsed_response_bytes`, not truncated to a scalar.
- [ ] `RouteEvidence` represents CAN11/CAN29 (not J1850-only); evidence is reusable by a future non-GM profile.
- [ ] `MockAdapter` honors `PhysicalTarget::Addressed` (precondition test green) before any corpus replay runs - OR the integration suite is moved to the Wave 9 `Elm327Adapter`+`MockTransport::expect` harness if the MockAdapter fix is not owned by an obd2-core wave.
- [ ] Default build still writes v2 recordings (v3 writing is flag-gated, default off); zero behavior change for users who do not opt in.
- [ ] No new third-party dependency (serde_json, chrono, flate2 already present).
- [ ] Corpus fixtures are committed under `crates/obd2-dash/tests/corpus/`; CI never reads live `raw-captures/` (subject to `StorageManager` FIFO deletion and gitignore).

### Rollback notes

- v3 RECORDING is gated behind a config flag (`recording.format_v3`, default `false`). Reverting in production is flipping the flag off: the app writes v2 again, and no v3 files are created. This makes the wave independently shippable - merged-but-dormant changes nothing until the flag is on.
- v3 READING is always compiled in but inert for v1/v2 files (it only activates on the `MAGIC_V3` header, which no v2-only writer ever produces). Removing the v3 read arm cannot affect v1/v2 decoding because those arms and `write_to` are left byte-for-byte unchanged.
- Asymmetry to document, not hide: a pre-Wave-7 binary CANNOT read a v3 file (`read_file_header` rejects unknown magic). That is acceptable because v3 writing is opt-in; do not enable the flag on a fleet running mixed binaries, or older clients will fail to open new recordings. There is no in-place downgrade of a v3 file to v2.
- `SessionHeader`/`SessionEntry` gained only `Option` + `#[serde(default)]` fields, so reverting those struct changes still parses both old and new files; nothing becomes unreadable on rollback.
- Evidence reconciliation is additive and revertible at the sink seam: `ProfileEvidenceRecord`/`ProfileEvidenceWriter`/`RecordingEvidenceSink` and the `DomainMessage::ProfileEvidence` variant are new, and `GmEvidenceRecord` stays (deprecated) with a `From` conversion. Because the emission point is Wave 3's `ProfileEvidenceSink` call (not a redesigned `execute_request`), reverting the dispatcher's evidence behavior is a one-line construction swap from `RecordingEvidenceSink` back to `NullEvidenceSink`; `execute_request` itself is untouched by the revert. Existing GM JSONL under `raw-captures/` remains valid and upgradable via `From`.
- The replay-dispatch and `ai/summary.rs` edits are pure additive `else if`/branch additions; removing them drops v3-frame handling but leaves the five legacy frame paths intact.
- If the v3 frame design proves wrong mid-implementation, the fallback is the rejected "stay v2, cap raw at 255 bytes" option (no `MAGIC_V3`, new frame types reuse the v2 u8-length envelope). It is shippable but knowingly violates invariant #8 (raw request/response preservation) for multi-frame J1850 responses, so it is documented as a degraded fallback, not the target.

## Wave 8: Active Tests

### Objective

Move the locked VGT vane active-test card out of the hardcoded GM module and under profile capability ownership (`ActiveTestDefinition`), with a profile-neutral runtime that evaluates preconditions, owns timeout/auto-cancel/hold-to-command state, and writes evidence per attempt and per transport response. Execution stays REFUSED by construction: the GM profile publishes only an `Unverified` command profile, so the dispatcher cannot build a `RoutedRequest` for VGT until trusted bytes (Tech2 / EFILive DVT / HP Tuners / Snap-on) are added later.

### Depends on (must land first)

- Profile model + registry + sealed `SelectedProfile` (Phase 1, the model/registry wave). The active-test gate keys off the selected profile token and its `context_generation`; without it there is no profile to own `ActiveTestDefinition` and no generation to invalidate against. Plan invariant 7 ("locked unless a profile provides verified command bytes") cannot be expressed otherwise. CRITICAL OWNERSHIP NOTE: Wave 1 owns and ships the final, richer shapes of `SafetyClass` (`{ReadOnly, Actuator}`), `ActiveCommandProfile` (`{Unverified{lock_reason}, Verified{source, enter}}`), `VerifiedSource`, and `ProfileRequestDefinition`. These are the Phase 8 forms, but they must land as Wave 1's definitions so Wave 1's tests and the model's exhaustiveness checks compile against them. Wave 8 CONSUMES these types and must never redefine them.
- Profile runtime + dispatcher `ProfileRuntime::execute_request` (Phase 3 wave). The (future) enter/cancel commands must route through the single execution path, not through a GM-specific send. Wave 8 adds `execute_active_test` ON TOP of that dispatcher and reuses its `ModuleMap + RouteDefinition -> ResolvedRoute` resolution (plan step 3). The new runtime methods hang off the SAME `ProfileRuntime` defined in Wave 3 - not a parallel runtime - and take Wave 3's `CapabilityId` (including the `CapabilityId::ActiveTest` variant) by value; Wave 8 must not mint a second `CapabilityId`. If Wave 8 lands before the dispatcher, it would have to reintroduce a GM send path - forbidden.
- GM LLY definition migration (Phase 5 wave) provides `profiles/gm/`, the ECM `RouteDefinition { module: ModuleKey::Ecm }`, and the LLY `ModuleMap` entry that resolves ECM to header `[0x6C, 0x10, 0xF1]`. The active-test route must be the SAME route object the signal migration produced, not a second hardcode of `0x10`/`[0x6C,0x10,0xF1]`.
- Evidence generalization (Phase 6 wave) SHOULD land first so active-test evidence carries `profile_id`, `capability_id`, and the manual-confirmation flag (plan Phase 6 list; inventory item 11 confirms `GmEvidenceRecord` has none of these today). If Wave 8 must ship before Phase 6, it keeps emitting `GmEvidenceRecord` and the new fields are deferred - call this out in the commit; it is the only acceptable temporary.

Wave 8 does NOT depend on Recording v3 (Phase 7): a refused test has no transport response, so there is no `FRAME_ACTIVE_TEST_ATTEMPT` payload worth recording yet. Wire that only once Verified bytes exist.

### Files touched

- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/active.rs` - the VGT vane control as profile DATA: one `ActiveTestDefinition` with `command_profile: ActiveCommandProfile::Unverified{..}`, the precondition list (verified-profile, stationary, idle, warm-coolant, battery-voltage, operator-confirmation), `cancel_command: None`, `timeout`, `evidence_policy`. Houses the relocated `vgt_vane_control_definition()`, `blocked_active_test_result()`, and the GM command shape validation moved from `gm_active.rs`.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/model.rs` - add ONLY the Phase 8-specific types: `ActiveTestDefinition`, `ActivePrecondition`, `PreconditionKind`, `ActiveTestRefusal`, `ActiveTestOutcome`, `ActiveTestConditions`. Do NOT define `SafetyClass`, `ActiveCommandProfile`, `VerifiedSource`, or `ProfileRequestDefinition` here - those are owned by Wave 1 (the model wave) and ship in their final richer shapes; Wave 8 imports them. Do NOT mint a second `CapabilityId` - reuse the `CapabilityId::ActiveTest` variant already defined in Wave 3. Redefining any of these in Wave 8 silently shadows Wave 1's model (breaking its exhaustiveness tests) and Wave 3's dispatcher. The Phase 8 types are fully specified below.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/runtime.rs` - add `ProfileRuntime::evaluate_active_test`, `ProfileRuntime::execute_active_test`, `ProfileRuntime::active_test_tick`, and `ProfileRuntime::cancel_active_test`. These methods hang off the SAME `ProfileRuntime` defined in Wave 3 (not a new runtime type) and take Wave 3's `CapabilityId` by value. This is where REFUSE-vs-PLAN is decided; it reuses the dispatcher's route resolution (`ProfileRuntime::execute_request`) and never branches on manufacturer.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/profiles/gm/mod.rs` - implement `DiagnosticProfile::active_tests(&self) -> &[ActiveTestDefinition]` to return the slice from `gm/active.rs` (was empty / unimplemented before this wave).
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/gm_active.rs` - REDUCE to the serialization/wire types only: keep `GmActiveTestId`, `GmActiveTestCommand` (+ `validate_shape`, `label`, `summary`, `test_id`), `GmActiveTestBlockReason`, `GmActiveTestPrecondition`, `GmActiveTestResult`. MOVE `vgt_vane_control_definition`, `blocked_active_test_result`, `active_test_evidence_record`, `write_active_test_evidence` out to `profiles/gm/active.rs` (re-export from here only if needed for the Tauri/Message boundary). Rationale: `GmActiveTestCommand`/`GmActiveTestResult` are the public Tauri contract (GUI inventory G) and the `Message::ActiveTestResult` payload (app.rs:137); changing their shape ripples to the JS frontend, so they stay put.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/session_runner.rs` - rewrite `handle_gm_active_test` (currently :772) to call `runtime.execute_active_test(&selected_profile, capability_id, command)` instead of calling `blocked_active_test_result` + `write_active_test_evidence` directly. The `DiagnosticCommand::GmActiveTest(command)` arm (:765) is unchanged in shape; its body now consults `ProfileState`. If no `SelectedProfile` exists, it returns a refusal (`NoSelectedProfile`) without touching any GM helper. Remove the direct `gm_active::{blocked_active_test_result, write_active_test_evidence}` imports from live execution.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/app.rs` - no functional change to `Message::ActiveTestResult` (:137) or `DiagnosticCommand::GmActiveTest` (:44); only update import paths if `GmActiveTestCommand`/`GmActiveTestResult` move. The `Message::ActiveTestResult` handler (:421) keeps mapping `result.accepted` -> "accepted"/"blocked"; it must keep showing "blocked" for the refused VGT.
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/apps/obd2-gui/src-tauri/src/main.rs` - `LiveBackend::request_active_test` (:534) and `write_active_test_evidence` (:552) route through `ProfileRuntime::execute_active_test` against the GUI's `SelectedProfile`; delete the GUI-local `blocked_active_test_result` call. `active_tests_snapshot` (:1184) and `active_precondition` (:1233) read precondition DEFINITIONS from the selected profile capability and evaluate them against live `ActiveTestConditions`, instead of the inline hardcoded list (lines 1191-1222). The Tauri command `request_active_test(command: GmActiveTestCommand)` (:585) signature is preserved (frontend contract).
- MODIFY `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/src/lib.rs` - adjust `pub mod` exports so live code reaches active tests only via `profiles::runtime`, and `gm_active` exposes types but not an execution entry point.
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/active_tests.rs` - integration-with-mock + architectural tests (named below).
- CREATE `/Users/jared/Projects/HaulLogic/obd2-dash/crates/obd2-dash/tests/corpus/profile/gm.gmt800.lly.class2/active-test-vgt-refused.jsonl` - refusal golden (expected outcome + evidence shape + zero routed writes).

### Exact APIs

These four types are OWNED BY WAVE 1 (the model wave) and OWNED BY WAVE 3 (`CapabilityId`). They are reproduced here for coder reference ONLY - Wave 8 imports them and must NOT declare them. They are shown so the active-test types below read in context; the canonical declarations live in Wave 1 / Wave 3.

```rust
// ---- DEFINED IN WAVE 1 (profiles/model.rs). DO NOT REDEFINE IN WAVE 8. ----
// These are the final, richer Phase 8 shapes, but they ship as Wave 1's model
// so Wave 1's exhaustiveness tests bind to them.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyClass {
    /// Read-only functional test; no actuator output.
    ReadOnly,
    /// Bidirectional actuator control; full gate (verified bytes + preconditions
    /// + timeout + cancel + evidence) is mandatory. VGT is Actuator.
    Actuator,
}

/// The command bytes for an active test. The ONLY way to make a test sendable
/// is to construct `Verified`. The GM profile constructs ONLY `Unverified` for
/// VGT, which is what keeps execution refused by construction.
#[derive(Clone, Debug)]
pub enum ActiveCommandProfile {
    Unverified { lock_reason: &'static str },
    Verified {
        source: VerifiedSource,
        enter: ProfileRequestDefinition,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerifiedSource {
    Tech2,
    EfiLiveDvt,
    HpTuners,
    SnapOn,
}

/// Shares the request shape the dispatcher already builds (service_id + data +
/// route). Resolves to `obd2_core::adapter::RoutedRequest { service_id, data,
/// target }` (adapter/mod.rs:29) via the route resolver. Never carries a header.
#[derive(Clone, Debug)]
pub struct ProfileRequestDefinition {
    pub route: RouteDefinition,       // RouteDefinition from profiles/model.rs (route wave)
    pub service_id: u8,
    pub request_data: &'static [u8],
}

// ---- DEFINED IN WAVE 3 (profiles/runtime.rs / model.rs). DO NOT REDEFINE. ----
// Wave 8 uses the existing `CapabilityId::ActiveTest` variant. No second
// CapabilityId enum.
// enum CapabilityId { .., ActiveTest, .. }  // Wave 3 owns this.
```

NEW in Wave 8 (`profiles/model.rs`) - the Phase 8 active-test types only. All reference obd2-core and Wave 1/Wave 3 types by their inventoried paths:

```rust
#[derive(Clone, Debug)]
pub struct ActivePrecondition {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: PreconditionKind,
}

#[derive(Clone, Copy, Debug)]
pub enum PreconditionKind {
    /// Satisfied only when `ActiveCommandProfile::Verified`. Always unsatisfied
    /// for VGT today. Mirrors GUI "Verified command profile" (main.rs:1192).
    VerifiedCommandProfile,
    Stationary { max_speed_kph: f32 },        // GUI: speed_kph < 0.5
    Idle { min_rpm: f32, max_rpm: f32 },      // GUI: 500.0..=900.0
    WarmCoolant { min_c: f32 },               // GUI threshold 104.0 F == 40.0 C
    BatteryVoltage { min_v: f32 },            // GUI: voltage >= 12.0
    /// Not observable from data; requires explicit operator confirmation.
    /// Mirrors GUI "Park/Neutral and A/C off" (main.rs:1217), satisfied=false.
    OperatorConfirmation,
}

#[derive(Clone, Debug)]
pub struct ActiveTestDefinition {
    pub key: &'static str,                    // "gm.vgt_vane_control"
    pub label: &'static str,                  // "VGT vane control"
    pub safety_class: SafetyClass,            // Wave 1 type
    pub command_profile: ActiveCommandProfile,// Wave 1 type
    pub preconditions: &'static [ActivePrecondition],
    pub timeout: std::time::Duration,
    pub cancel_command: Option<ProfileRequestDefinition>, // Wave 1 type
    pub evidence_policy: EvidencePolicy,      // from model.rs (Phase 6)
}

/// Live values the runtime evaluates preconditions against. Populated from the
/// current domain snapshot (rpm/speed/coolant/voltage). Unit is explicit to
/// avoid the F-vs-C ambiguity (see OWL).
#[derive(Clone, Copy, Debug)]
pub struct ActiveTestConditions {
    pub speed_kph: Option<f32>,
    pub rpm: Option<f32>,
    pub coolant_c: Option<f32>,
    pub battery_v: Option<f32>,
    pub operator_confirmed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActiveTestRefusal {
    NoSelectedProfile,
    CapabilityNotOwned,
    StaleGeneration,
    UnverifiedCommandProfile,
    InvalidCommandShape { detail: String },
    PreconditionUnmet { key: &'static str },
    /// Profile reached only via manual confirmation; actuators stay locked
    /// until a normal Exact match is established (plan Vehicle Identity
    /// Lifecycle, lines 303-304).
    ManualConfirmationOnly,
}

#[derive(Clone, Debug)]
pub enum ActiveTestOutcome {
    Refused(ActiveTestRefusal),
    /// Unreachable today (no Verified profile). Defined so the gate is complete.
    Planned { request: ProfileRequestDefinition, deadline: std::time::Instant },
}
```

New runtime API in `profiles/runtime.rs` (methods on the Wave 3 `ProfileRuntime`; `CapabilityId` is Wave 3's):

```rust
impl ProfileRuntime {
    /// Pure evaluation: validates the token, capability ownership, command
    /// shape, command-profile verification, and every precondition. NEVER sends.
    /// Returns the first failing reason or (for Verified profiles only) a plan.
    pub fn evaluate_active_test(
        &self,
        selected: &SelectedProfile,
        capability: CapabilityId,           // Wave 3's CapabilityId::ActiveTest
        command: &GmActiveTestCommand,
        conditions: &ActiveTestConditions,
        manual_confirmed: bool,
    ) -> ActiveTestOutcome;

    /// The single execution entry point for active tests. Calls
    /// `evaluate_active_test`; on `Refused` it writes attempt evidence and
    /// returns a refused `GmActiveTestResult` WITHOUT any adapter call. On
    /// `Planned` (Verified only) it would route through `execute_request`
    /// (adapter/mod.rs:112) and write attempt + per-response evidence. Today the
    /// `Planned` arm is unreachable because the GM profile is `Unverified`.
    pub async fn execute_active_test<A: obd2_core::adapter::Adapter>(
        &self,
        session: &mut obd2_core::session::Session<A>,
        selected: &SelectedProfile,
        capability: CapabilityId,           // Wave 3's CapabilityId::ActiveTest
        command: GmActiveTestCommand,
        conditions: ActiveTestConditions,
        manual_confirmed: bool,
    ) -> GmActiveTestResult;

    /// Watchdog: if an actuator test is Holding and `now >= deadline`, plan and
    /// send `cancel_command`, write cancel evidence, transition to Idle. Dead
    /// today (nothing ever reaches Holding) but defined so timeout-cancel exists.
    pub async fn active_test_tick<A: obd2_core::adapter::Adapter>(
        &self,
        session: &mut obd2_core::session::Session<A>,
        now: std::time::Instant,
    ) -> Option<GmActiveTestResult>;

    /// Force-cancel on disconnect / generation change. On generation bump the
    /// `SelectedProfile` is already invalid, so this fires cancel_command (if any
    /// Holding) using the still-resolvable route, then drops active-test state.
    pub async fn cancel_active_test<A: obd2_core::adapter::Adapter>(
        &self,
        session: &mut obd2_core::session::Session<A>,
        reason: ActiveTestRefusal,
    ) -> Option<GmActiveTestResult>;
}
```

These four methods are added to Wave 3's `ProfileRuntime` impl and consume Wave 3's `CapabilityId`. There is exactly one `ProfileRuntime` and one `CapabilityId` in the crate; Wave 8 extends them, it does not fork them.

GM profile data in `profiles/gm/active.rs`:

```rust
use std::time::Duration;

pub const VGT_PRECONDITIONS: &[ActivePrecondition] = &[
    ActivePrecondition { key: "verified_command_profile", label: "Verified command profile",
        kind: PreconditionKind::VerifiedCommandProfile },
    ActivePrecondition { key: "stationary", label: "Stationary",
        kind: PreconditionKind::Stationary { max_speed_kph: 0.5 } },
    ActivePrecondition { key: "idle", label: "Idle speed",
        kind: PreconditionKind::Idle { min_rpm: 500.0, max_rpm: 900.0 } },
    ActivePrecondition { key: "warm_coolant", label: "Warm coolant",
        kind: PreconditionKind::WarmCoolant { min_c: 40.0 } },  // == 104.0 F
    ActivePrecondition { key: "battery_voltage", label: "Battery voltage",
        kind: PreconditionKind::BatteryVoltage { min_v: 12.0 } },
    ActivePrecondition { key: "operator_confirmation", label: "Park/Neutral and A/C off",
        kind: PreconditionKind::OperatorConfirmation },
];

pub fn vgt_vane_control_active_test() -> ActiveTestDefinition {
    ActiveTestDefinition {
        key: "gm.vgt_vane_control",
        label: "VGT vane control",
        safety_class: SafetyClass::Actuator,            // Wave 1 type/variant
        command_profile: ActiveCommandProfile::Unverified {  // Wave 1 type/variant
            lock_reason:
                "no verified GM Class 2 actuator command bytes are available for this test",
        },
        preconditions: VGT_PRECONDITIONS,
        timeout: Duration::from_millis(5_000),   // == GmActiveTestCommand hold_ms ceiling
        cancel_command: None,                     // unknown until Verified; see invariant
        evidence_policy: EvidencePolicy::Always,  // attempt evidence even when refused
    }
}
```

The relocated builders in `profiles/gm/active.rs` keep the pinned evidence shape; only the route source changes (resolved from the ECM `RouteDefinition`, not a second literal):

```rust
// Reproduces module_label "ECM/PCM", node 0x10, header [0x6C, 0x10, 0xF1],
// request_service 0x00, empty request_data, decoder "gm-active-test-vgt-vane-control"
// (gm_active.rs:181-188). Header MUST match the GM Class 2 convention 6C <node> F1
// confirmed by GUI request_gm_node (main.rs:821) and apply_target (elm327.rs:241).
pub fn active_test_evidence_record(
    command: &GmActiveTestCommand,
    result: &GmActiveTestResult,
    profile_id: ProfileId,        // NEW (Phase 6): "gm.gmt800.lly.class2"
    capability_id: CapabilityId,  // Wave 3 type; active-test "gm.vgt_vane_control"
    manual_confirmed: bool,       // NEW: marks manual confirmation in evidence
) -> /* GmEvidenceRecord or generalized ProfileEvidenceRecord per Phase 6 */ ;
```

Refusal -> `GmActiveTestResult.status` string mapping MUST preserve the existing contract: `ActiveTestRefusal::UnverifiedCommandProfile` -> `"unverified_command_profile"`, `InvalidCommandShape` from `VgtManualPercent` out-of-range -> `"invalid_value"` / `"invalid_duration"` (the existing `GmActiveTestBlockReason::as_str` values, gm_active.rs:82-88). This keeps the pinned `gm_active.rs` tests and the JS frontend green.

The GM command shape validation (`GmActiveTestCommand::validate_shape`, gm_active.rs:57-70: percent `0.0..=100.0`, hold_ms `250..=5000`) stays on the command type and is called first inside `evaluate_active_test`; an invalid shape returns `Refused(InvalidCommandShape{..})` before any precondition or route work - identical ordering to today's `blocked_active_test_result` (gm_active.rs:154-157).

### Tests

Unit (in `profiles/gm/active.rs` and `profiles/model.rs`):

- `vgt_active_test_is_unverified_and_actuator` - asserts `command_profile` is `ActiveCommandProfile::Unverified` (Wave 1 variant), `safety_class == SafetyClass::Actuator` (Wave 1 variant), `cancel_command.is_none()`. Pins "locked by construction".
- `actuator_verified_requires_cancel_command` - constructs (in test only) a `Verified` actuator `ActiveTestDefinition` with `cancel_command: None` and asserts the definition constructor / validator rejects it. Encodes the invariant: an actuator that can send MUST have a release command (hold-to-command safety).
- `precondition_kinds_map_to_gui_thresholds` - asserts `VGT_PRECONDITIONS` thresholds equal the GUI's inline values (stationary 0.5 kph, idle 500-900, coolant 40C==104F, voltage 12.0) so the migration changes no gate threshold. OWL guard against the F/C rounding slip.
- `validate_shape_*` - keep the three existing `gm_active.rs` tests (`manual_percent_shape_rejects_out_of_range_value`, `blocked_result_defaults_to_unverified_profile_for_valid_shape`, `evidence_record_marks_unverified_active_test`) green; they pin `status == "unverified_command_profile"`, `module_label == "ECM/PCM"`, `node == 0x10`, `request_service == 0x00`, error `UnverifiedCommand`, decoded `ActiveTest{accepted:false}`.
- `refusal_status_strings_are_stable` - maps every `ActiveTestRefusal` to its `GmActiveTestResult.status` string and asserts the unverified/invalid values are byte-identical to `GmActiveTestBlockReason::as_str`.

Integration-with-mock (in `tests/active_tests.rs`, using `MockAdapter`):

- `active_test_attempt_sends_zero_routed_requests` - THE headline safety test. Build a session with an LLY-matched `SelectedProfile`, a `MockAdapter` whose `routed_request` (and `request`) increment a shared counter. Call `runtime.execute_active_test(.., VgtManualPercent{percent:35.0, hold_ms:1000}, ..)`. Assert: outcome status `"unverified_command_profile"`, `accepted == false`, and the adapter counter == 0. Asserting count==0 (not "no error") is required because the default `Adapter::routed_request` (adapter/mod.rs:112) ERRORS on addressed targets - a careless send would surface as a transport error and could be mistaken for a benign block.
- `active_test_with_no_selected_profile_refuses` - no `SelectedProfile` in `ProfileState`; assert `Refused(NoSelectedProfile)` and zero adapter calls. Pins plan invariant 3.
- `active_test_stale_generation_refuses` - hand a `SelectedProfile` whose `context_generation` no longer matches `ProfileState.generation` (simulate disconnect/VIN change); assert `Refused(StaleGeneration)` and zero adapter calls. Pins token invalidation (plan lines 307-309).
- `active_test_capability_not_owned_refuses` - pass a `CapabilityId` that the selected GM profile does not own; assert `Refused(CapabilityNotOwned)`, zero adapter calls.
- `manual_confirmed_profile_keeps_actuator_locked` - selected profile reached via manual confirmation; assert `Refused(ManualConfirmationOnly)` even if all other preconditions are forced satisfied. Pins plan lines 303-304.
- `invalid_command_shape_refuses_before_route` - `VgtManualPercent{percent:101.0, hold_ms:1000}`; assert `Refused(InvalidCommandShape)` / status `"invalid_value"`, zero adapter calls.
- `every_attempt_writes_one_evidence_record` - point evidence at a `tempfile::TempDir`; run one refused attempt; assert exactly one record appended, `accepted == false`, and (Phase 6) `profile_id == "gm.gmt800.lly.class2"`, `capability_id == "gm.vgt_vane_control"`, manual-confirmation flag present. Pins plan invariant 8 + Phase 8 "evidence for every attempt".
- `active_test_tick_is_noop_when_idle` - call `active_test_tick(now)` with no Holding state; assert `None` and zero adapter calls. Guards the dead-but-defined watchdog from accidentally sending.

Golden-corpus (`tests/corpus/profile/gm.gmt800.lly.class2/active-test-vgt-refused.jsonl`):

- One entry: input `{capability: "gm.vgt_vane_control", command: {kind: "vgt_manual_percent", percent: 35.0, hold_ms: 1000}, conditions: {...all satisfied...}}`, expected `{outcome: "refused", reason: "unverified_command_profile", routed_writes: 0, evidence: {module_label: "ECM/PCM", node: 16, request_header: [108,16,241], request_service: 0, profile_id: "gm.gmt800.lly.class2", capability_id: "gm.vgt_vane_control", manual_confirmation: false}}`. The corpus replay asserts this byte/value-for-value. This is a REFUSAL golden, not a decode golden - there is no positive `$22`/`$19`/actuator traffic for active tests in any `raw-captures/*.obd2raw` file (rec inventory D: no `$19` traffic exists at all), so a positive active-test golden cannot be seeded from real capture and must not be claimed.

Architectural (in `tests/active_tests.rs`):

- `live_code_has_no_active_test_send_path` - source-scan test (same mechanism as the plan's "architectural import test", plan line 541): assert that outside `profiles::runtime`, no live module (`session_runner.rs`, `app.rs`, GUI `main.rs`) calls `Session::raw_request` (session/mod.rs:1020), `Adapter::routed_request`, or constructs a `RoutedRequest` for an active test. Concretely: grep the live module set for those symbols and fail if any appear in an active-test code path.
- `unverified_profile_cannot_produce_a_plan` - construct the GM `ActiveTestDefinition`, force ALL `ActiveTestConditions` satisfied and `manual_confirmed == false`, and assert `evaluate_active_test` returns `Refused(UnverifiedCommandProfile)`, NEVER `Planned`. This is the compile-adjacent proof that "execution stays refused" survives even a fully-satisfied gate, because the block is the `Unverified` command profile itself.

### Acceptance criteria

- [ ] VGT vane control is published as an `ActiveTestDefinition` via `DiagnosticProfile::active_tests` on the GM profile; no live code reads `vgt_vane_control_definition()` outside the profile module.
- [ ] Wave 8 defines NO second `SafetyClass`, `ActiveCommandProfile`, `VerifiedSource`, `ProfileRequestDefinition`, or `CapabilityId`. `SafetyClass`/`ActiveCommandProfile` resolve to the Wave 1 model types in their final richer shapes; the active-test capability uses `CapabilityId::ActiveTest` from Wave 3; the new runtime methods are added to Wave 3's single `ProfileRuntime`. Wave 1's exhaustiveness/match tests stay green (a Wave 8 redefinition would shadow them and either fail to compile or bind the wrong type).
- [ ] `ActiveCommandProfile::Verified` is constructed NOWHERE in non-test code; the GM profile emits only `Unverified`. `unverified_profile_cannot_produce_a_plan` passes.
- [ ] `active_test_attempt_sends_zero_routed_requests` passes: a full attempt with an exact LLY profile and all preconditions satisfied makes ZERO adapter `request`/`routed_request` calls.
- [ ] All four refusal gates (no profile, stale generation, capability-not-owned, manual-confirmation-only) refuse with zero adapter calls.
- [ ] Command shape validation (`percent 0..=100`, `hold_ms 250..=5000`) is unchanged; the three existing `gm_active.rs` unit tests pass with no edits to their assertions.
- [ ] Refused `GmActiveTestResult.status` strings are byte-identical to today (`"unverified_command_profile"`, `"invalid_value"`, `"invalid_duration"`); the JS frontend and `app.rs` "blocked" path (app.rs:421) render identically.
- [ ] Active-test evidence record preserves `module_label == "ECM/PCM"`, `node == 0x10`, `request_header == [0x6C,0x10,0xF1]`, `request_service == 0x00`, decoder `"gm-active-test-vgt-vane-control"`; the J1850 header is produced via the shared `RouteDefinition { module: ModuleKey::Ecm }` plus LLY `ModuleMap` resolution, not a new literal.
- [ ] Evidence carries `profile_id`, `capability_id`, and a manual-confirmation flag (if Phase 6 has landed); otherwise the commit explicitly states these are deferred and `GmEvidenceRecord` is retained as a temporary.
- [ ] Exactly one evidence record is written per attempt; refused attempts still write evidence.
- [ ] GUI `request_active_test` Tauri command signature (`GmActiveTestCommand` in, `GmActiveTestResult` out) is unchanged; the JS frontend payload shape does not change.
- [ ] GUI `active_tests_snapshot` precondition labels/satisfied/detail values match the pre-migration output for the same live inputs (no visible UI diff for the LLY truck), now sourced from profile capability data.
- [ ] Existing LLY golden corpus (signals + DTCs) stays green with zero diffs: `cargo test -p obd2-dash` and the protocol/profile corpora pass unchanged. Active tests touch no signal/DTC decode path, so any diff there is a Wave 8 regression and blocks merge.
- [ ] `cargo test -p obd2-core` stays green (Wave 8 adds no obd2-core change).
- [ ] The architectural test fails the build if any live module gains an active-test send path outside `ProfileRuntime`.

### Rollback notes

- Independently shippable: Wave 8 changes only the active-test surface, which is REFUSED before and after. No signal or DTC behavior changes, so it can ship on its own once its dependency waves (model, dispatcher, GM definition migration) are in. There is no observable runtime behavior change for the LLY truck beyond evidence gaining `profile_id`/`capability_id` fields (additive).
- Behind a flag: gate the runtime routing of active tests behind a cargo feature, e.g. `profile-active-tests`. With the feature OFF, `session_runner::handle_gm_active_test` and the GUI `request_active_test` fall back to the original direct `gm_active::blocked_active_test_result` + `write_active_test_evidence` calls (kept compiled). This lets the wave merge while the dispatcher/evidence waves stabilize, with zero risk because both paths refuse.
- Revert procedure: because `GmActiveTestCommand`/`GmActiveTestResult`/`GmActiveTestId`/`GmActiveTestBlockReason` stay in `gm_active.rs` unchanged, reverting Wave 8 is: restore `vgt_vane_control_definition`/`blocked_active_test_result`/`active_test_evidence_record`/`write_active_test_evidence` into `gm_active.rs`, repoint `handle_gm_active_test` (session_runner.rs:772) and GUI `request_active_test` (main.rs:534) back to those functions, and drop `profiles/gm/active.rs` plus the new Phase 8 `model.rs` types and `runtime.rs` methods. Because Wave 8 added the active-test runtime methods to Wave 3's `ProfileRuntime` (not a separate type) and added no `SafetyClass`/`ActiveCommandProfile`/`CapabilityId` of its own, the revert removes only the Phase 8 additions and leaves Wave 1's model and Wave 3's runtime intact. The Tauri contract and `Message::ActiveTestResult` never changed, so the GUI/TUI compile and behave identically after revert.
- What must NOT be rolled back casually: the `active_test_attempt_sends_zero_routed_requests` and `unverified_profile_cannot_produce_a_plan` tests are the safety floor. If a later wave adds `Verified` bytes, those tests change deliberately and in a single reviewed commit (per the plan's golden-correction rule). Do not delete them as part of a Wave 8 revert.

OWL flags to carry forward:

1. Default `Adapter::routed_request` ERRORS on addressed targets (adapter/mod.rs:112). The zero-writes test must assert call-count == 0, not "no error", or a stray send hides as a transport error. Verify `MockAdapter` either overrides `routed_request` or that the test counter is hit BEFORE the default impl runs.
2. Coolant unit trap: GUI gates on `coolant_f >= 104.0`; profile data stores `min_c: 40.0`. 104 F == 40 C exactly, but a careless `(104-32)*5/9` rounding can drift to 40.0000001; pin the threshold in the test and document the unit on `PreconditionKind::WarmCoolant`.
3. Hold-to-command release is mandatory: any future `Verified` actuator MUST set `cancel_command: Some(..)`; the `actuator_verified_requires_cancel_command` test enforces it. VGT stays `Unverified` with `cancel_command: None`, which is consistent only because it can never enter Holding.
4. The `Planned`/`Holding`/`active_test_tick`/`cancel_active_test` paths are dead today (no `Verified` profile). They are scaffolding; the `active_test_tick_is_noop_when_idle` test guards against the watchdog ever sending while no test is enabled. Do not let a reviewer "simplify" them away - they are the timeout/auto-cancel contract from Phase 8. Note: these methods, `Planned`, and `Holding` must reference the SAME Wave 3 `ProfileRuntime` and `CapabilityId` runtime - they are scaffolding ON the existing runtime, not a parallel active-test runtime API. A second copy would diverge from the dispatcher.
5. Manual confirmation keeps actuators locked even at full precondition satisfaction (plan lines 303-304). The `manual_confirmed_profile_keeps_actuator_locked` test pins this; it is easy to drop when wiring the GUI manual-confirm path.
6. No positive active-test golden can come from `raw-captures/` (no actuator/`$19` traffic exists). The corpus entry is a REFUSAL golden only; do not claim active-test execution coverage from real capture.
7. Type-ownership trap: `SafetyClass`, `ActiveCommandProfile`, `VerifiedSource`, and `ProfileRequestDefinition` are WAVE 1 types, and `CapabilityId` (with its `ActiveTest` variant) is a WAVE 3 type. The richer Phase 8 shapes (`SafetyClass{ReadOnly,Actuator}`, `ActiveCommandProfile{Unverified{lock_reason}, Verified{source,enter}}`) are correct, but they must be Wave 1's declarations - not copied into `profiles/model.rs` as fresh enums during Wave 8. A careless implementer redeclares them (or mints a second `CapabilityId`), silently shadowing Wave 1's model and Wave 3's dispatcher; Wave 1's exhaustiveness tests then fail to compile or bind to the wrong type, and the dispatcher's `CapabilityId` match diverges. Wave 8 IMPORTS these types and ADDS its runtime methods to the existing `ProfileRuntime`; it declares only the active-test-specific types (`ActiveTestDefinition`, `ActivePrecondition`, `PreconditionKind`, `ActiveTestConditions`, `ActiveTestRefusal`, `ActiveTestOutcome`).

Source claims verified exactly as cited (renderers.rs:564 `0x1542`, :582/:597 `0x1251`; ui.rs:2392-2404 `0x1170/0x1171/0x163D/0x163E`, :2544-2545 `0x1543/0x1540`, :2679-2685 `0x162E`+cylinder / `0x162F..=0x1636`). Here is the corrected section.

## Wave 9: Non-GM Proof Profile

### Objective

Add a tiny, read-only, feature-gated non-GM "fixture" profile on a CAN 11-bit route and prove by test that the profile runtime is manufacturer-neutral: the scheduler, dispatcher, decoder isolation, evidence, recording/replay, and the data-layer UI tab mapping all work for a non-GM profile with zero changes to LLY behavior. This wave is the gate that must pass before any real Ford/Ram profile is written.

Scope note (see correction in Depends on + Tests): the renderer-level "UI tabs render from capabilities" invariant is explicitly OUT of scope for Wave 9 and is marked UNMET, because no wave migrates the hardcoded LLY DID literals out of `renderers.rs`/`tui/ui.rs`. Wave 9 proves the UI mapping only at the data layer.

### Depends on

This wave consumes the entire profile runtime and cannot start until the following have landed. Each dependency is named by the capability it must deliver (the plan phase that defines it) because the wave-to-phase numbering is not 1:1.

- Neutral profile model, FINALIZED SHAPES (plan Phase 1): `crates/obd2-dash/src/profiles/{mod,model,registry}.rs` exist and export `ProfileId`, `Manufacturer`, `VehicleContext`, `ProfileMatch`, `MatchConfidence`, `IdentityConfidence`, `SelectedProfile`, the `DiagnosticProfile` trait, `SignalDefinition`, `SignalCategory`, `RouteDefinition`, `AddressTemplate`, `BusKey`, `ModuleKey`, `DtcServiceDefinition`, `RouteSet`, `ActiveTestDefinition`, `PassiveMonitorDefinition`, `SourceFields`, `EvidencePolicy`, `FailurePolicy`, `PollCadence`, `Confidence`, `Provenance`, `StandardPidOverride`, `DecodedSignal`, `DecodedDtc`, `ProfileDecodeError`, the `ProfileRegistry`, plus the framework match floor and the cross-profile ambiguity test harness. Wave 1 MUST pin these as the single source of truth; Wave 9 conforms to them and does not redeclare any of them. Without the floor and the ambiguity test, the fixture's "no two profiles claim the same vehicle" proof has nowhere to plug in.

- Wave 1 ADDITIVE PIECES Wave 9 requires (these do not exist in the current Wave 1 draft; Wave 1 must add them, additively, before Wave 9 can compile -- see the cross-wave "PROFILE MODEL TYPE INSTABILITY" and "REGISTRY + KEY API SURFACE" notes):
  - `Provenance::LocalFixture` -- a new enum variant (additive; no existing arm changes).
  - `EvidencePolicy::BoundedLive` -- a new enum variant.
  - `SourceFields::NONE` -- an associated const on `SourceFields` (the empty set).
  - `RouteSet::single(RouteDefinition) -> RouteSet` -- a `const fn` constructor (see const-context hazard below). If `RouteSet` is currently `&'static [RouteDefinition]`-backed it should become an enum (`Single(RouteDefinition)` / `Multi(&'static [RouteDefinition])`) or otherwise expose a `const fn single`.
  - `BusKey::new(&'static str) -> Self` + `BusKey::as_str(&self) -> &'static str` (both `const fn`, used inside `const FIXTURE_SIGNALS`/`FIXTURE_DTC_SERVICES`). `ModuleKey` is the CLOSED enum; fixture routes use variants directly (`ModuleKey::Ecm`) and read the string via `const fn canonical()`. There is no `ModuleKey::new`/`as_str`.
  - `IdentityConfidence::is_trusted(&self) -> bool` -- the gate used by `matches()`. `VehicleContext.vin_confidence` is an `IdentityConfidence`; Wave 1 has no such method today.
  - `ProfileRegistry::register(&mut self, profile: &'static dyn DiagnosticProfile)` (or an equivalent builder that accepts a `&'static dyn DiagnosticProfile`) plus storage that can hold a registered entry. Wave 1's storage is `&'static dyn DiagnosticProfile` with NO register method; Wave 9 hands it a `&'static` reference, never a `Box`.
  These are the ONLY model changes Wave 9 leans on from Wave 1; everything else Wave 9 conforms to as-is. If any of these is missing when Wave 9 starts, Wave 9 does not compile -- block on Wave 1, do not fork a parallel type.

- Session-owned selection (plan Phase 2): `ProfileState`, generation-bound `SelectedProfile` minted ONLY by the resolver (`pub(in crate::profiles)` constructor -- do NOT widen to `pub(crate)`; see cross-wave "SELECTEDPROFILE SEAL VISIBILITY REGRESSION"), and a `VehicleContext` builder that runs off `Session::vehicle()` / `Session::discovery()` / `Session::adapter_info()`. Needed so the fixture can be selected and so the stale-token proof is real.

- Central dispatcher (plan Phase 3): `ProfileRuntime::execute_request`, the probe-only API boundary, and the architectural import test. The fixture's end-to-end execution and the "no new bypass" arch assertion extend this.

- Profile scheduler / poll policy -- OWNED BY WAVE 3.5 (Poll Policy and Scheduler). `ProfileRuntime::plan_poll_cycle` builds plans from `signals()`/`dtc_services()` + cadence with no manufacturer branch, in `crates/obd2-dash/src/profiles/scheduler.rs`. Wave 3.5 moves the forced-standard-PID / cadence / candidate-DID-suppression / `preferred_over` policy out of `session_runner.rs` (`should_force_standard_poll` :131/:824, cadence `cycle % 5/10/20/60` :207/:216/:217/:232) and out of `tui/ui.rs:2403-2404` + `main.rs:322-343`. Wave 9's scheduler-neutrality proof and its `scheduler_has_no_manufacturer_branch` scan run against Wave 3.5's scheduler. Wave 3.5 lands before Waves 4/5 in the graph, so this dependency is satisfied before Wave 9.

- GM LLY migrated under the profile (plan Phase 5): `profiles/gm/` exists and `gm.gmt800.lly.class2` is a registered `DiagnosticProfile`. The fixture must sit beside a real profile; the ambiguity, decoder-isolation, and "LLY unchanged" proofs are meaningless without it.

- Generalized evidence (plan Phase 6): `ProfileEvidenceRecord` carrying `profile_id` + `capability_id`. The fixture evidence assertion targets this record, not `GmEvidenceRecord`.

- Recording/replay v3 (plan Phase 7): `MAGIC_V3` and the `FRAME_PROFILE_VALUE` / `FRAME_PROFILE_DTC` / `FRAME_PROFILE_REQUEST` / `FRAME_PROFILE_RESPONSE` frame types in `crates/obd2-dash/src/recording/format.rs`, plus a replay consumer that dispatches profile frames by frame type and `profile_id` (not by a hardcoded GM check). The non-GM replay proof depends on this; see Hazards for the silent-drop trap.

- Capability-driven UI refactor -- NOT IMPLEMENTED BY ANY WAVE (correction): the plan's UI Model (lines 771-802) and Phase-9 proof "UI tabs render from capabilities" (line 766) require the TUI tab/section list to derive from the selected profile's `SignalCategory` set. It does not. Verified hardcoded LLY DID literals remain: `renderers.rs:564` (`enhanced_reading(state, 0x1542)`), `:582/:597` (`0x1251`); `tui/ui.rs:2392-2404` (`0x1170 | 0x1171 | 0x163D | 0x163E`, `0x1540 | 0x1543`), `:2544-2545` (`0x1543`/`0x1540`), `:2679-2685` (`0x162E` + cylinder, `0x162F..=0x1636`). Wave 6 keeps `DiagnosticSnapshot` byte-identical and does NOT migrate this rendering to `SignalCategory`. Therefore the renderer-level proof CANNOT be met by Wave 9. Proper fix: insert a dedicated UI-migration wave (migrate `renderers.rs`/`tui/ui.rs` from DID literals to `SignalCategory`/capability-driven rendering) BEFORE Wave 9. Until that wave lands, Wave 9 downgrades its UI proof to the data-layer mapping only and explicitly records the renderer-level invariant as UNMET. See Tests and Acceptance criteria.

- Active tests under profiles (plan Phase 8): `ActiveTestDefinition` ownership lives on the profile. The fixture proves this negatively (a read-only profile exposes zero active tests, so no active-test controls render).

- Frozen golden-corpus harness with a SINGLE pinned schema + shared loader (Regression Firewall #1; cross-wave "CORPUS LAYOUT/SCHEMA DIVERGENCE"): one globbing corpus runner exists that reads `signal_key`+`module`-keyed JSONL entries (the pinned schema -- not DID-keyed and not a per-wave flat layout) under `crates/obd2-dash/tests/corpus/profile/<id>/*.jsonl` and `crates/obd2-dash/tests/corpus/protocol/<family>/*.jsonl`, replays each entry through the real decoders via one shared loader, and asserts byte-for-byte / value-for-value identity. The LLY baseline corpus under `tests/corpus/profile/gm.gmt800.lly.class2/` and `tests/corpus/protocol/j1850-vpw/` must already be green under that schema. Wave 9 adds files this runner auto-discovers; it adds no runner code and introduces no second schema.

GUI parity (the `apps/obd2-gui` quarantine wave) is NOT a hard dependency: the fixture proof runs entirely through the TUI/library runtime. Add the GUI fixture assertion only if the GUI already consumes `ProfileRuntime` (plan invariant 9); otherwise defer it and note it in the wave that quarantines `request_gm_node`.

### Files touched

- CREATE `crates/obd2-dash/src/profiles/fixture/mod.rs` -- the entire fixture profile: `FixtureProfile` struct, `impl DiagnosticProfile`, `FIXTURE_SIGNALS`, `FIXTURE_DTC_SERVICES`, fixture-local decoders, synthetic identity constants, and the inline unit tests. Must NOT import `crate::gm_enhanced`, `crate::gm_class2`, `crate::gm_active`, or `crate::gm_evidence`.
- MODIFY `crates/obd2-dash/src/profiles/mod.rs` -- add `#[cfg(any(test, feature = "proof-profile"))] pub mod fixture;`. The fixture is invisible in default release builds.
- MODIFY `crates/obd2-dash/src/profiles/model.rs` -- add one additive enum variant `Manufacturer::Fixture`. Additive only; no existing variant or arm changes. (The `Provenance::LocalFixture`, `EvidencePolicy::BoundedLive`, `SourceFields::NONE`, `RouteSet::single`, `BusKey::new`+`as_str`, the closed-enum `ModuleKey`+`canonical()`, and `IdentityConfidence::is_trusted` additions are owned by Wave 1 per Depends on, NOT added here.)
- MODIFY `crates/obd2-dash/src/profiles/registry.rs` -- register `FixtureProfile` only under `#[cfg(any(test, feature = "proof-profile"))]`, as a `&'static dyn DiagnosticProfile` (NOT `Box`), so the synthetic profile can never match a customer vehicle in a shipped binary.
- MODIFY `crates/obd2-dash/Cargo.toml` -- add `[features]` with `proof-profile = []` (default off). CI must pass `--features proof-profile`.
- CREATE `crates/obd2-dash/tests/corpus/protocol/can-11bit/fixture-frames.jsonl` -- synthetic CAN 11-bit `BusFamily` frame-decode goldens (seeds the non-J1850 protocol path required by Firewall #1), in the pinned `signal_key`+`module` schema.
- CREATE `crates/obd2-dash/tests/corpus/profile/fixture.can11.readonly.v1/signals.jsonl` -- `signal_key`+`module`-keyed entries of request bytes + response bytes -> expected `DecodedSignal` goldens, consumed by the shared loader.
- CREATE `crates/obd2-dash/tests/corpus/profile/fixture.can11.readonly.v1/dtcs.jsonl` -- synthetic SAE DTC goldens decoded by the fixture DTC decoder (NOT `gm_class2`).
- CREATE `crates/obd2-dash/tests/proof_profile.rs` -- integration tests for selection neutrality, dispatcher ownership/generation, scheduler neutrality, decoder cross-isolation at runtime, and evidence fields. Gated `#![cfg(feature = "proof-profile")]`.
- CREATE `crates/obd2-dash/tests/proof_profile_replay.rs` -- v3 recording round-trip + replay-surfacing tests for non-GM frames, plus the old-recording-still-replays firewall. Gated `#![cfg(feature = "proof-profile")]`.
- MODIFY (or CREATE if absent) `crates/obd2-dash/tests/architecture.rs` -- add source-scan assertions that the fixture module does not reference GM symbols and that the scheduler has no manufacturer branch. Reuse the Phase 3 architectural import test if it already exists as a file. (Note: `scheduler_has_no_manufacturer_branch` cannot run until the Phase-4 wave creates `src/profiles/scheduler.rs`.)
- CREATE `crates/obd2-dash/tests/proof_profile_ui.rs` -- data-layer tab-mapping test ONLY (see Tests). The renderer-level test is deliberately absent until the UI-migration wave lands. Gated `#![cfg(feature = "proof-profile")]`.
- MODIFY `crates/obd2-dash/src/recording/replay.rs` ONLY IF the v3 consumer dispatch from Phase 7 does not already surface profile frames generically. If a non-GM `FRAME_PROFILE_VALUE` is dropped because dispatch is keyed on GM, that is a Phase 7 defect; fix it additively here (extend the dispatch, do not branch on manufacturer) and note it in the commit. No edit if Phase 7 already dispatches by frame type + `profile_id`.

No file is deleted in this wave.

### Exact APIs

obd2-core signatures consumed (from the inventory; do not reimplement):

```rust
// obd2_core::vehicle
pub enum Protocol { /* ... */ Can11Bit500, Can11Bit250, J1850Vpw, /* ... */ Auto } // #[non_exhaustive], Copy
pub enum PhysicalAddress {                                                          // #[non_exhaustive], no Copy
    J1850 { node: u8, header: [u8; 3] },
    Can11Bit { request_id: u16, response_id: u16 },
    Can29Bit { request_id: u32, response_id: u32 },
    J1939 { source_address: u8 },
}
pub struct ModuleId(pub String);                       // ECM/TCM/... string consts

// obd2_core::adapter
pub struct RoutedRequest { pub service_id: u8, pub data: Vec<u8>, pub target: PhysicalTarget }
pub enum PhysicalTarget { Broadcast, Addressed(PhysicalAddress) }

// obd2_core::adapter::elm327
impl Elm327Adapter { pub fn new(transport: Box<dyn Transport>) -> Self }

// obd2_core::transport::mock
impl MockTransport { pub fn new() -> Self; pub fn expect(&mut self, command: &str, response: &str) }

// obd2_core::transport::logging
pub fn parse_raw_capture(path: &Path) -> std::io::Result<Vec<(String, String)>>;

// obd2_core::adapter::mock  (DO NOT use for golden replay; see Hazards)
impl MockAdapter { pub fn new() -> Self; pub fn with_vin(vin: &str) -> Self }

// obd2_core::session
impl<A: Adapter> Session<A> { pub fn new(adapter: A) -> Self; pub fn load_spec_dir(&mut self, dir: &Path) -> Result<usize, Obd2Error> }
```

New code in `crates/obd2-dash/src/profiles/model.rs` (additive -- the ONLY model edit Wave 9 owns):

```rust
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Manufacturer {
    Gm,
    Ford,
    ChryslerRam,
    Generic,
    Fixture, // test/proof-only; never selected in a default release build
}
```

New module `crates/obd2-dash/src/profiles/fixture/mod.rs`. Every name below either is a Wave 1 finalized symbol or a Wave 1 additive symbol named in Depends on; Wave 9 introduces no parallel type:

```rust
use crate::profiles::model::{
    ActiveTestDefinition, AddressState, AddressTemplate, BusDefinition, BusKey, Confidence,
    DecodedDtc, DecodedSignal, DiagnosticProfile, DtcServiceDefinition, EvidencePolicy,
    FailurePolicy, Manufacturer, MatchConfidence, ModuleDefinition, ModuleKey, ModuleMap,
    ModuleSafetyClass, PassiveMonitorDefinition, PollCadence, ProfileDecodeError,
    ProfileId, ProfileMatch, Provenance, RouteDefinition, RouteSet, SignalCategory,
    SignalDefinition, SourceFields, StandardPidOverride, VehicleContext,
};
use obd2_core::protocol::codec::BusFamily;
use obd2_core::vehicle::Protocol;

/// Stable id. The `fixture.` prefix marks it synthetic; it is never a real OEM.
pub const FIXTURE_PROFILE_ID: ProfileId = ProfileId::new("fixture.can11.readonly.v1");

/// Synthetic 17-char VIN with reserved WMI "0FI" that no production vehicle uses.
/// Used only by tests and the fixture corpus.
pub const FIXTURE_VIN: &str = "0FI00000000000001";

/// Standard OBD-II functional/physical CAN ids; deliberately generic, not GM.
const FIXTURE_REQUEST_ID: u16 = 0x7E0;
const FIXTURE_RESPONSE_ID: u16 = 0x7E8;

pub const FIXTURE_BUSES: &[BusDefinition] = &[
    BusDefinition {
        key: BusKey::new("can-hs"),
        family: BusFamily::Can,
        protocol: Protocol::Can11Bit500,
        j1850: None,
        label: "Fixture CAN",
    },
];

pub const FIXTURE_MODULES: &[ModuleDefinition] = &[
    ModuleDefinition {
        key: ModuleKey::Ecm,
        display_label: "Fixture ECM",
        bus: BusKey::new("can-hs"),
        address: AddressState::Confirmed(AddressTemplate::Can11 {
            request_id: FIXTURE_REQUEST_ID,
            response_id: FIXTURE_RESPONSE_ID,
        }),
        safety_class: ModuleSafetyClass::Powertrain,
        coresident_with: None,
    },
];

pub const FIXTURE_MODULE_MAP: ModuleMap = ModuleMap {
    buses: FIXTURE_BUSES,
    modules: FIXTURE_MODULES,
};

// NOTE: BusKey::new / RouteSet::single are invoked in const; ModuleKey uses enum variants directly
// context below, so Wave 1 must define all three as `const fn` (see Depends on).
pub const FIXTURE_SIGNALS: &[SignalDefinition] = &[
    SignalDefinition {
        key: "fixture.coolant_centideg",
        label: "Fixture Coolant",
        category: SignalCategory::Powertrain,
        route: RouteDefinition {
            module: ModuleKey::Ecm,
        },
        service_id: 0x22,
        request_data: &[0xF0, 0x01],
        decoder_id: "fixture.scalar.u16_centi",
        unit: "C",
        cadence: PollCadence::Medium,
        confidence: Confidence::Verified,
        provenance: &[Provenance::LocalFixture],      // Wave 1 additive variant
        source_fields: SourceFields::NONE,            // Wave 1 additive const
        evidence_policy: EvidencePolicy::BoundedLive, // Wave 1 additive variant
        failure_policy: FailurePolicy::SurfaceUnavailable,
        preferred_over: None,
    },
    // one more scalar signal recommended to exercise multi-signal plan ordering
];

pub const FIXTURE_DTC_SERVICES: &[DtcServiceDefinition] = &[
    DtcServiceDefinition {
        key: "fixture.dtc.stored",
        label: "Fixture Stored DTCs",
        route_set: RouteSet::single(RouteDefinition {   // Wave 1 additive const fn
            module: ModuleKey::Ecm,
        }),
        service_id: 0x03,
        request_data: &[],
        decoder_id: "fixture.dtc.sae2byte",
        backoff_policy: Default::default(),
    },
];

pub struct FixtureProfile;

impl DiagnosticProfile for FixtureProfile {
    fn id(&self) -> ProfileId { FIXTURE_PROFILE_ID }
    fn manufacturer(&self) -> Manufacturer { Manufacturer::Fixture }

    fn matches(&self, ctx: &VehicleContext) -> ProfileMatch {
        // Floor: concrete CAN 11-bit (never Protocol::Auto) + trusted VIN-derived identity.
        if !matches!(ctx.protocol, Protocol::Can11Bit500) {
            return ProfileMatch::NoMatch;
        }
        match ctx.vin.as_deref() {
            // corrupted/short/absent VIN can never be Exact (is_trusted gate below)
            Some(vin) if ctx.vin_confidence.is_trusted() // Wave 1 additive method
                && vin.len() == 17
                && vin.starts_with("0FI")
                && vin == FIXTURE_VIN =>
            {
                // Wave 1's MatchConfidence set is {ProtocolPlusVinDimension, VinExact, VinPlusSpec};
                // the fixture matches the exact synthetic VIN, so VinExact (NOT a nonexistent ::High).
                ProfileMatch::Exact { confidence: MatchConfidence::VinExact }
            }
            Some(vin) if vin.starts_with("0FI") => {
                ProfileMatch::Partial { reason: "fixture WMI but identity not trusted".into() }
            }
            _ => ProfileMatch::NoMatch,
        }
    }

    fn standard_pid_overrides(&self) -> &[StandardPidOverride] { &[] } // not global LLY quirk
    fn signals(&self) -> &[SignalDefinition] { FIXTURE_SIGNALS }
    fn dtc_services(&self) -> &[DtcServiceDefinition] { FIXTURE_DTC_SERVICES }
    fn active_tests(&self) -> &[ActiveTestDefinition] { &[] } // read-only proof profile
    fn passive_monitors(&self) -> &[PassiveMonitorDefinition] { &[] }

    fn decode_signal(
        &self,
        signal: &SignalDefinition,
        payload: &[u8],
    ) -> Result<DecodedSignal, ProfileDecodeError> {
        match signal.decoder_id {
            "fixture.scalar.u16_centi" => {
                // Wave 1's ProfileDecodeError uses struct variants; conform to its
                // exact field names (shown here as { expected, got }).
                let raw = payload
                    .get(0..2)
                    .ok_or(ProfileDecodeError::PayloadTooShort { expected: 2, got: payload.len() })?;
                let value = u16::from_be_bytes([raw[0], raw[1]]) as f64 / 100.0;
                Ok(DecodedSignal::scalar(signal.key, value, signal.unit))
            }
            // decoder_id is &'static str, and Wave 1's UnknownDecoder takes &'static str:
            // pass `other` through directly, no String allocation.
            other => Err(ProfileDecodeError::UnknownDecoder(other)),
        }
    }

    fn decode_dtc_response(
        &self,
        service: &DtcServiceDefinition,
        payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
        match service.decoder_id {
            "fixture.dtc.sae2byte" => decode_sae_pairs(payload), // fixture-local, NOT gm_class2
            other => Err(ProfileDecodeError::UnknownDecoder(other)),
        }
    }
}

fn decode_sae_pairs(payload: &[u8]) -> Result<Vec<DecodedDtc>, ProfileDecodeError> { /* ... */ }
```

Conformance ledger (this is the part the OWL review flagged -- get it exactly right):

- CONFORM to Wave 1's finalized shapes; do NOT invent:
  - `MatchConfidence`: Wave 1's FINAL set is `ProtocolPlusVinDimension` / `VinExact` / `VinPlusSpec`. There is no `MatchConfidence::High`, and the older `VinDerived`/`SpecConfirmed`/`VinAndSpec` names are RETIRED -- do not use them.
  - `ProfileDecodeError::PayloadTooShort { .. }` is a struct variant (match Wave 1's exact field names) and `ProfileDecodeError::UnknownDecoder(&'static str)` takes `&'static str`, not `String`. Never call `.to_string()` on `decoder_id`.
  - `DecodedSignal::scalar(key, value, unit)` per Wave 1's signature.
  - Registration uses a `&'static dyn DiagnosticProfile`, never `Box::new(..)` -- Wave 1's registry storage is `&'static dyn`.
- WAVE 1 ADDITIVE symbols Wave 9 consumes (Wave 1 must add per Depends on; Wave 9 must not fork them): `Provenance::LocalFixture`, `EvidencePolicy::BoundedLive`, `SourceFields::NONE`, `RouteSet::single` (const fn), `BusKey::new`+`as_str`, the closed-enum `ModuleKey` with `const fn canonical()`, `IdentityConfidence::is_trusted`, and `ProfileRegistry::register(&'static dyn DiagnosticProfile)`.
- The only authoritative new symbols THIS wave owns are `Manufacturer::Fixture`, `FixtureProfile`, `FIXTURE_PROFILE_ID`, `FIXTURE_VIN`, `FIXTURE_SIGNALS`, `FIXTURE_DTC_SERVICES`, and the two fixture decoders.

Registration in `crates/obd2-dash/src/profiles/registry.rs` (conforms to Wave 1's `&'static dyn` storage -- NO `Box`):

```rust
// A unit-struct static gives us a 'static address to hand the registry.
#[cfg(any(test, feature = "proof-profile"))]
static FIXTURE_PROFILE: crate::profiles::fixture::FixtureProfile =
    crate::profiles::fixture::FixtureProfile;

#[cfg(any(test, feature = "proof-profile"))]
fn register_fixture(registry: &mut ProfileRegistry) {
    // &'static FixtureProfile unsizes to &'static dyn DiagnosticProfile at the call
    // site, matching Wave 1's storage type. Do NOT use Box::new(FixtureProfile).
    registry.register(&FIXTURE_PROFILE);
}
```

The exact `ProfileRegistry::register` / `select` signature is owned by Phase 1; call it as defined (Wave 1 adds `register` per Depends on), do not redeclare it.

### Tests

Unit (inline in `profiles/fixture/mod.rs`, `#[cfg(all(test, feature = "proof-profile"))]`):

- `fixture_decode_scalar_roundtrip` -- `decode_signal` on `[0x13, 0x88]` yields a `DecodedSignal` with value `50.00` and unit `"C"`. Asserts the unit comes from `SignalDefinition.unit`, not the empty-string leak from `Session::read_enhanced` (core inventory #8).
- `fixture_decode_short_payload_errors` -- `decode_signal` on `[0x13]` returns `ProfileDecodeError::PayloadTooShort { expected: 2, got: 1 }` (conforms to Wave 1's struct variant; catches a regression where the slice guard is dropped).
- `fixture_decode_dtc_pairs` -- `decode_dtc_response` decodes synthetic SAE 2-byte pairs into the expected codes; proves the fixture has its own DTC decoder distinct from `gm_class2::decode_class2_dtcs`.
- `fixture_match_exact_only_for_synthetic_identity` -- Exact (with `MatchConfidence::VinExact`) only for `protocol == Can11Bit500` AND `vin == FIXTURE_VIN` AND `vin_confidence.is_trusted()`. Asserts NoMatch for: missing VIN, the real LLY VIN `1GCHK23224F000001`, `Protocol::J1850Vpw`, and `Protocol::Auto`.
- `fixture_match_floor_rejects_corrupted_vin` -- a 17-char `0FI`-prefixed VIN with `I`/`1` corruption and untrusted confidence returns Partial or NoMatch, never Exact (plan match-floor: corrupted VIN never Exact).
- `fixture_has_no_active_tests` -- `active_tests()` is empty (read-only proof).

Route resolution unit (in `profiles/runtime.rs` tests, `#[cfg(all(test, feature = "proof-profile"))]`):

- `resolve_can11_route_preserves_request_and_response_ids` -- `resolve_route(&FIXTURE_MODULE_MAP, &RouteDefinition { module: ModuleKey::Ecm }, Protocol::Can11Bit500)` resolves to `PhysicalAddress::Can11Bit { request_id: 0x7E0, response_id: 0x7E8 }` with both ids intact. This is the CAN twin of the J1850 header-synthesis arm and catches a dropped/swapped `response_id`. Assert on the constructed `ResolvedRoute.physical_address`, not on an adapter reply (the MockAdapter ignores the target, so only the resolver output proves correct routing).

Integration with mock (`tests/proof_profile.rs`, `#![cfg(feature = "proof-profile")]`):

- `dispatch_executes_fixture_signal_end_to_end` -- build `Session<Elm327Adapter>` over a `MockTransport` seeded via `expect()` with the synthetic CAN response for `22 F0 01`, build a `VehicleContext` with `FIXTURE_VIN` + `Can11Bit500` + trusted confidence, select the fixture, call `plan_poll_cycle`, run `execute_request` for the fixture signal capability, and assert the returned `DecodedSignal` equals the golden value. Proves the dispatcher executes a non-GM profile with no GM branch.
- `dispatch_rejects_capability_not_owned_by_selected_profile` -- under a fixture `SelectedProfile`, passing an LLY `CapabilityId` fails dispatcher validation; under an LLY `SelectedProfile`, passing the fixture capability fails. Proves capability-ownership is profile-neutral, not GM-special-cased.
- `partial_match_yields_no_token_and_dispatcher_refuses` -- a `0FI`-prefixed but untrusted VIN produces `ProfileMatch::Partial`, the resolver mints NO `SelectedProfile`, and any attempted routed request through `execute_request` is refused (plan invariant 5 end-to-end: a partial match is visible but cannot poll manufacturer-specific requests).
- `stale_fixture_token_fails_validation` -- mint a fixture `SelectedProfile` at generation N, bump `VehicleContext.generation` to N+1, assert `execute_request` rejects the stale token. Proves generation binding is profile-neutral.
- `scheduler_plan_for_fixture_is_fixture_only` -- `plan_poll_cycle` for the fixture profile yields exactly the fixture routes (resolved through `FIXTURE_MODULE_MAP`) and zero GM routes; planning the LLY profile yields the identical plan it produced before this wave (snapshot compare). Requires Wave 3.5's scheduler.
- `registry_select_fixture_and_lly_are_mutually_exclusive` -- for the LLY `VehicleContext` only `gm.gmt800.lly.class2` is Exact and the fixture is NoMatch; for the fixture `VehicleContext` only the fixture is Exact and LLY is NoMatch. No ambiguity is raised in either direction.
- `evidence_record_carries_profile_and_capability_id` -- a fixture live read emits a `ProfileEvidenceRecord` whose `profile_id == FIXTURE_PROFILE_ID` and `capability_id` matches the fixture signal, with raw write/read text and parsed response bytes populated (plan invariant 8 + Phase 6).

Decoder isolation runtime (in `tests/proof_profile.rs`):

- `gm_payload_through_fixture_decoder_errors` -- feeding a known LLY `62 1540` payload to `FixtureProfile::decode_signal` returns `ProfileDecodeError`, never a value. No global lookup leaks GM semantics into the fixture.
- `fixture_payload_through_gm_decoder_is_not_reachable` -- there is no code path that decodes a fixture capability through `find_lly_did` / the GM registry; assert the fixture capability's `decoder_id` is not present in the GM decoder dispatch table.

Golden corpus (extend the shared corpus runner; no new runner code if it globs):

- `corpus_profile_fixture_can11` -- every entry in `tests/corpus/profile/fixture.can11.readonly.v1/*.jsonl` replays to identical `DecodedSignal`/`DecodedDtc`, byte-for-byte and value-for-value, via the shared loader and pinned `signal_key`+`module` schema.
- `corpus_protocol_can_11bit` -- every entry in `tests/corpus/protocol/can-11bit/*.jsonl` decodes through the real CAN `BusFamily` codec to the expected frame. Seeds the first non-J1850 protocol golden, proving the corpus axis is protocol-plural.
- `corpus_profile_lly_unchanged` -- the existing LLY corpus runner passes with zero diffs. Wave 9 adds no file under `tests/corpus/profile/gm.gmt800.lly.class2/` or `tests/corpus/protocol/j1850-vpw/`; if any LLY golden output changes, the wave is rejected.

Architectural (`tests/architecture.rs`):

- `fixture_module_does_not_reference_gm` -- read `src/profiles/fixture/mod.rs` as text and assert it contains none of `gm_enhanced`, `gm_class2`, `gm_active`, `gm_evidence`, `find_lly_did`, `LLY_`, `class2_`. Decoder isolation by construction (plan decoder-isolation tests).
- `scheduler_has_no_manufacturer_branch` -- read `src/profiles/scheduler.rs` and `src/profiles/runtime.rs` as text and assert no `match` on `Manufacturer` and no `Manufacturer::Gm` / `Manufacturer::Fixture` literal. Confirms Layer 2 stays manufacturer-blind even with two manufacturers present. (Cannot run until the Phase-4 wave creates `scheduler.rs`; this is a hard upstream dependency, not an optional file.)
- `live_code_cannot_reach_probe_only_routed_api` -- the Phase 3 import test still passes with the fixture present (no new bypass introduced by the fixture or its tests).

Recording / replay (`tests/proof_profile_replay.rs`, `#![cfg(feature = "proof-profile")]`):

- `v3_profile_value_frame_roundtrips_fixture` -- write a `FRAME_PROFILE_VALUE` carrying `profile_id == FIXTURE_PROFILE_ID`, a capability id, and raw response bytes; read it back and assert all fields including raw bytes survive. Uses the v3 envelope; asserts the file header is `MAGIC_V3`.
- `replay_surfaces_fixture_value_not_dropped` -- replay a v3 recording containing a fixture frame and assert the replay consumer emits a domain message for it. This is the critical test: it guards the rec-inventory E.2 silent-drop trap where new frame types round-trip but are dropped because no `is_*` predicate / consumer arm handles them.
- `fixture_dtc_frame_uses_raw_bytes_not_f64` -- a `FRAME_PROFILE_DTC` carries its code in `raw_bytes`, not the legacy `value: f64` 8-char-truncating hack (rec inventory format.rs `dtc()`).
- `old_lly_v2_recording_replays_identically` -- a committed v2 `.obd2rec` LLY fixture replays to the identical frame sequence it produced before this wave. Replay-compatibility firewall (plan Phase 7).
- `unknown_future_frame_skipped_not_crash` -- a `frame_type == 0xFE` payload inside the v3 envelope is skipped and replay continues to the next frame. Confirms forward-compat within the documented size ceiling (note the `u8` length cap in the spec).

UI mapping -- DATA LAYER ONLY (`tests/proof_profile_ui.rs`); the renderer-level proof is OUT of scope per the correction:

- `fixture_profile_tabs_data_mapping` -- assert a pure `profile_tabs(profile: &dyn DiagnosticProfile) -> Vec<SignalCategory>` mapping returns exactly the fixture categories (Powertrain) and NO GM categories (Turbo/VGT, Fuel/Rail, GM Class 2). This proves the data-layer derivation is profile-neutral. It does NOT prove the rendered TUI.
- DELIBERATELY ABSENT until a UI-migration wave lands: `fixture_tabs_render_from_capabilities` and `lly_tabs_unchanged` (the renderer-level tests). Reason, verified in source: `renderers.rs` and `tui/ui.rs` still drive sections from hardcoded LLY DID literals -- `renderers.rs:564` (`0x1542`), `:582/:597` (`0x1251`); `tui/ui.rs:2392-2404` (`0x1170 | 0x1171 | 0x163D | 0x163E`, `0x1540 | 0x1543`), `:2544-2545`, `:2679-2685` (`0x162E` + cylinder, `0x162F..=0x1636`). No wave migrates these to `SignalCategory`. Until the migration wave removes those literals, do NOT write a test that claims "UI renders from capabilities"; it would either be vacuous or assert behavior the renderer does not have. Record the renderer-level invariant as UNMET in the wave notes and the architecture doc.

### Acceptance criteria

- [ ] Existing LLY golden corpus stays green with zero diffs (`tests/corpus/profile/gm.gmt800.lly.class2/` and `tests/corpus/protocol/j1850-vpw/` outputs are byte-for-byte and value-for-value unchanged).
- [ ] `cargo test -p obd2-dash` (no features) compiles and passes; the fixture module and its tests are compiled out and the registry does not contain the fixture.
- [ ] `cargo test -p obd2-dash --features proof-profile` passes all unit, integration, decoder-isolation, corpus, architectural, replay, and data-layer UI-mapping tests above.
- [ ] `cargo test -p obd2-core` still passes (no obd2-core change was required for this wave; if any was made, the full protocol corpus is re-run and stays green).
- [ ] The fixture profile is registered only under `cfg(any(test, feature = "proof-profile"))` as a `&'static dyn DiagnosticProfile`; a default release build of the TUI cannot select it, and no `Box` registration exists.
- [ ] Fixture `matches()` returns `Exact { confidence: MatchConfidence::VinExact }` only for the synthetic `FIXTURE_VIN` on `Can11Bit500` with trusted VIN confidence; it returns NoMatch for the LLY VIN and for `Protocol::Auto`, and Partial for a `0FI` VIN that is not trusted.
- [ ] A `Partial`-matched fixture context mints NO `SelectedProfile` and the dispatcher refuses any routed request (invariant 5 end-to-end).
- [ ] For the LLY `VehicleContext`, the registry returns exactly one Exact match (LLY) and the fixture is NoMatch; for the fixture context, exactly one Exact (fixture) and LLY is NoMatch. No ambiguity is raised in either direction.
- [ ] The dispatcher executes a fixture CAN signal end to end with no `match manufacturer` anywhere in Layer 2, and rejects a capability id not owned by the selected fixture profile.
- [ ] A stale fixture `SelectedProfile` (older generation) fails dispatcher validation.
- [ ] `FixtureProfile::decode_signal` cannot decode an LLY payload and there is no path that decodes a fixture capability through `find_lly_did` or the GM registry.
- [ ] `src/profiles/fixture/mod.rs` contains zero GM symbol references (architectural text scan green).
- [ ] `src/profiles/scheduler.rs` and `src/profiles/runtime.rs` contain no `Manufacturer` match and no per-OEM literal (requires the Phase-4 wave to exist; if `scheduler.rs` is absent, Wave 9 is blocked, not green).
- [ ] A v3 recording with a fixture `FRAME_PROFILE_VALUE` round-trips and the replayed value is surfaced by the consumer (not silently dropped); an old v2 LLY recording replays identically; an unknown `0xFE` frame is skipped without crashing.
- [ ] Fixture evidence records carry `profile_id` and `capability_id` and raw request/response bytes.
- [ ] Data-layer tab mapping (`profile_tabs`) returns the fixture categories with no GM categories. The renderer-level "UI renders from capabilities" invariant is explicitly recorded as UNMET (hardcoded LLY DIDs still present at `renderers.rs:564/582/597` and `tui/ui.rs:2392-2404/2544-2545/2679-2685`); this wave does NOT claim it, and a separate UI-migration wave is required to satisfy plan line 766.
- [ ] CI runs `cargo test -p obd2-dash --features proof-profile`; without the feature flag the proof tests are silently absent (CI config must enable it -- verified by checking the CI invocation includes the flag).

### Rollback notes

- The wave ships dark by default. `proof-profile` is an off-by-default Cargo feature, and both the module (`profiles/fixture`) and its registration are `#[cfg(any(test, feature = "proof-profile"))]`. A default release build never compiles or registers the fixture, so the wave is independently shippable behind the flag with zero customer-visible behavior.
- Full revert: delete `src/profiles/fixture/`, the `pub mod fixture;` line in `profiles/mod.rs`, the `register_fixture` block + `FIXTURE_PROFILE` static in `registry.rs`, the `proof-profile` feature in `Cargo.toml`, the `tests/proof_profile*.rs` files, the fixture-specific architectural assertions, and the two corpus directories under `tests/corpus/profile/fixture.can11.readonly.v1/` and `tests/corpus/protocol/can-11bit/`. The LLY corpus and all shared runtime code are untouched, so no GM behavior reverts with it.
- The Wave 1 additive symbols (`Provenance::LocalFixture`, `EvidencePolicy::BoundedLive`, `SourceFields::NONE`, `RouteSet::single`, `BusKey::new`+`as_str`, the closed-enum `ModuleKey`+`canonical()`, `IdentityConfidence::is_trusted`, `ProfileRegistry::register`) are owned by Wave 1, not this wave; reverting Wave 9 does not remove them and must not, since later real profiles need them.
- The single change in THIS wave that is not deletable in isolation is the `Manufacturer::Fixture` enum variant. It is additive, but any `match Manufacturer` arms added for it must be removed in the same revert. Because Layer 2 is forbidden from matching on `Manufacturer`, the only legitimate arms are in UI labeling code; if a revert leaves a dangling `Fixture` arm it is a compile error, which is the desired canary, not a silent leak.
- If the recording dispatch needed an additive edit in `recording/replay.rs` to surface non-GM frames, that edit should be generic (keyed on frame type + `profile_id`) and is safe to keep even after a fixture revert, since it benefits any future profile. Flag it separately in the commit so it can be retained while the rest of the wave is reverted.
- The renderer-level UI proof is already out of scope (data-layer only). No renderer code changes in this wave, so there is nothing UI-specific to back out; the `profile_tabs` mapping and its test are independent of the (still pending) renderer refactor.

### OWL hazards a careless implementer will hit

- Do NOT reintroduce the nonexistent symbols the OWL review flagged. `MatchConfidence::High`, `Provenance` without `LocalFixture`, `EvidencePolicy` without `BoundedLive`, `SourceFields` without `NONE`, `RouteSet` without `single`, `BusKey` without `new`/`as_str`, a stringly `ModuleKey` or `ModuleKey::new`/`as_str` (it is a closed enum with `canonical()`), `vin_confidence` without `is_trusted`, `ProfileDecodeError::UnknownDecoder(String)`, and `Box::new(FixtureProfile)` were ALL in the draft and ALL fail to compile against Wave 1. Use `MatchConfidence::VinExact`/`VinPlusSpec`, the struct-variant `PayloadTooShort`, `UnknownDecoder(&'static str)`, a `&'static dyn` registration, and the Wave-1-additive symbols named in Depends on.
- `BusKey::new` and `RouteSet::single` are called inside `const FIXTURE_SIGNALS` / `const FIXTURE_DTC_SERVICES`; they MUST be `const fn`. `ModuleKey` enum variants are inherently const. A non-const `new` is a hard const-eval error here; raise it against Wave 1 immediately rather than hacking the consts into `static` initializers.
- The registry stores `&'static dyn DiagnosticProfile`. A unit-struct temporary (`&FixtureProfile` as a bare expression) is NOT `'static`; register `&FIXTURE_PROFILE` where `FIXTURE_PROFILE` is a `static`. Do not "fix" the type error by switching the registry to `Box<dyn>` -- that is a Wave 1 storage change that breaks Waves 2/3 and is forbidden by the cross-wave registry note.
- Do NOT start Wave 9 before Wave 3.5 (scheduler.rs + plan_poll_cycle) has landed. The scheduler-neutrality test and the `scheduler_has_no_manufacturer_branch` scan read those files; the poll policy must already be out of `session_runner.rs` (`should_force_standard_poll` :131/:824, cadence `cycle % 5/10/20/60`) and UI-local copies in `tui/ui.rs:2403-2404` + `main.rs:322-343`. A green Wave 9 build that skips this is proving nothing about scheduling.
- Do NOT replay the fixture golden corpus through `obd2_core::adapter::mock::MockAdapter`. Its `routed_request` override ignores `req.target` entirely and returns a canned `[0x80, 0x00]` for every Mode 22 (mock.rs:300-313, verified). A corpus run on it would be green while proving nothing about routing or decoding. Use `Elm327Adapter::new(Box<MockTransport>)` seeded with `MockTransport::expect(command, response)` (synthetic CAN frames), the same harness shape the LLY corpus uses via `parse_raw_capture`. This is the one harness all integration waves should standardize on, since obd2-core is frozen and the mock cannot honor addressed J1850.
- Do NOT label the proof profile `Manufacturer::Ford`. The plan forbids real Ford until after this gate; a `Ford` label invites someone to start adding real Ford data against a fixture skeleton. Use `Manufacturer::Fixture` and a `fixture.` id prefix.
- Do NOT let the fixture match a real vehicle. The synthetic `0FI` WMI plus the `cfg`-gated registration are both required; either alone is insufficient (a loose matcher in a test-feature build can still collide in CI, and a tight matcher in a release build is still surface area).
- Do NOT prove "the frame round-trips" and stop. A v3 frame can serialize, deserialize, and still be dropped at replay because the consumer dispatch handles only the five legacy frame types (rec inventory E.2). The surfacing test (`replay_surfaces_fixture_value_not_dropped`) is the one that matters.
- Do NOT reuse the `FRAME_DTC` `value: f64` code-packing for fixture DTCs; it truncates past 8 ASCII chars. Fixture DTCs must live in `raw_bytes`.
- Do NOT import any `gm_*` symbol into the fixture module "for convenience." It silently destroys the decoder-isolation proof and is caught only by the text-scan architectural test.
- Do NOT assert routing correctness through the adapter reply; the mock ignores the target. Assert the resolver's constructed `PhysicalAddress::Can11Bit { request_id, response_id }` directly, or the CAN route-resolution bug (the analog of the J1850 dropped-header hazard) passes undetected.
- Do NOT claim "UI renders from capabilities." Verified: `renderers.rs` / `ui.rs` still hold hardcoded LLY DID literals (`0x1542`, `0x1251`, `0x163D/E`, `0x1540/3`, the cylinder block `0x162E`/`0x162F..=0x1636`, and the rejected-DID `0x1170/1` fallbacks). No wave migrates them. Wave 9's UI proof is the data-layer `profile_tabs` mapping ONLY; the renderer-level invariant (plan line 766) is UNMET and must be documented as such until a dedicated UI-migration wave removes those literals.
