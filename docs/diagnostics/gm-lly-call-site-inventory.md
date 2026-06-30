# GM/LLY Call-Site Inventory

Wave 0 freezes the current dashboard-side raw manufacturer-routed call sites.
The machine-checked source of truth is
`crates/obd2-dash/tests/architecture.rs`; this document is the narrative mirror.

Each `(file, symbol, max_count)` is a non-increasing upper bound. Adding a new
manufacturer-routed call site requires raising a bound deliberately with review.
Moving or removing a call site must lower the matching bound in the same commit.
Do not delete the architectural test to make a migration pass.

## Frozen Dashboard Bounds

| File | Symbol | Max count |
| --- | --- | ---: |
| `src/session_runner.rs` | `find_lly_did(` | 0 |
| `src/session_runner.rs` | `.raw_request(` | 2 |
| `src/session_runner.rs` | `.adapter_mut(` | 0 |
| `src/session_runner.rs` | `class2_routed_request(` | 0 |
| `src/session_runner.rs` | `class2_dtc_all_request(` | 0 |
| `src/session_runner.rs` | `class2_dtc_active_request(` | 0 |
| `src/session_runner.rs` | `.routed_request(` | 0 |
| `src/app.rs` | all watched symbols | 0 |
| `src/main.rs` | all watched symbols | 0 |
| `src/domain.rs` | all watched symbols | 0 |
| `src/vehicle_data.rs` | all watched symbols | 0 |
| `src/mock_profile.rs` | all watched symbols | 0 |
| `src/tui/*.rs` | all watched symbols | 0 |
| `src/widget/*.rs` | all watched symbols | 0 |

Watched symbols are `find_lly_did(`, `.raw_request(`, `.routed_request(`,
`class2_routed_request(`, `class2_dtc_all_request(`,
`class2_dtc_active_request(`, and `.adapter_mut(`.

## Definer Quarantine

The architecture test also pins current GM library definers:

| Symbol definition | Allowed file |
| --- | --- |
| `fn class2_routed_request(` | `src/gm_class2.rs` |
| `fn class2_header(` | `src/gm_class2.rs` |
| `fn find_lly_did(` | `src/gm_enhanced.rs` |

When a later wave relocates these definitions behind the profile runtime, it
updates both the test and this inventory in the same commit.

## Scope Note

This worker's write scope is the dash crate test/corpus surface and this
diagnostic document. The GUI sibling crate guard described in the broader Wave 0
plan is not added here because `apps/obd2-gui/**` is outside the requested
ownership scope.
