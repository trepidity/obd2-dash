# OBD-II Dash Regression Corpus

This directory is the frozen Wave 0 corpus. Replay tests read only this committed tree.
The ignored seeder writes candidates to `.staging/`; promotion is a manual diff and copy.

## Layout

- `protocol/<family>/*.jsonl`: Layer 1 ELM strip fixtures.
- `profile/<profile_id>/signal-*.jsonl`: profile-tier signal decode fixtures.
- `profile/<profile_id>/dtc-*.jsonl`: profile-tier DTC decode fixtures.

The profile directory is intentionally flat. Test loaders select by filename prefix
(`signal-` or `dtc-`). Real-vs-synthetic provenance is carried in each record:
signal and payload records use `capture`, DTC records use `source`.

## Schemas

`PayloadGolden` pins raw ELM response text to stripped payload bytes:
`capture`, `raw_response_text`, `family`, `skip_bytes`, `echo_command`,
`expected_payload_hex`.

`SignalGolden` pins adapter-stripped payload bytes to the current LLY decoder output:
`capture`, `profile_id`, `service_id`, `did`, optional `signal_key`, `module`,
`request_hex`, `request_header_hex`, `payload_hex`, and `expected`.
`module` is the physical route from `request_header_hex` byte 1 (`0x10` is `ecm`,
`0x18` is `tcm`), not a display label. `signal_key` is absent in Wave 0 and is
reserved for later additive population.

`DtcGolden` pins `decode_class2_dtcs` output: `source`, `profile_id`,
`payload_hex`, and ordered `expected` records with `code`, `gm_status_raw`, and
the Debug form of `generic_status`.

Numeric `service_id`, `did`, and status fields are decimal JSON numbers. Hex
byte streams stay as uppercase compact strings.

## Wave 0 Coverage

Real LLY J1850 VPW captures currently pin exactly these positive Mode 22 DIDs:

- `0x1540`: VGT vane desired
- `0x1543`: VGT vane actual
- `0x162F`: injector balance cylinder 1

The initial fixture lines come from
`raw-captures/1GTHK29294E391526-dev_cu_usbserial-223230360830-20260627T035830.obd2raw`
under header `6C10F1`.

DTC fixtures are synthetic and lifted from the existing `gm_class2.rs` unit-test
vectors. No `$19`/`$59` fixture in Wave 0 is labeled real.

## Known Gaps

Not covered by real positive Wave 0 fixtures: `0x1251`, `0x1542`, `0x163D`,
`0x163E`, `0x1470`, `0x1940`, injector pulse width `0x1193..0x119A`, and
injector balance `0x1630..0x1636`. `0x1940` is a known route-vs-label gap: when
it is added, `module` must record the TCM route even if the live display label is
still wrong.

The corrupted-looking `1IGTHKI...` capture filenames are not negative VIN
goldens. Their `0902` payload bytes decode cleanly to the same VIN as the clean
captures, so they cannot prove a corrupted-VIN rejection path.

## Additive Rule

Later waves may add new lines, new `signal-*.jsonl` or `dtc-*.jsonl` files, or
new protocol family tiers. They must not rewrite frozen lines or restructure this
layout without an explicit corpus migration and review.
