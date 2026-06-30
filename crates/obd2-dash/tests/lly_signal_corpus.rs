mod corpus_support;

use std::collections::BTreeSet;

use corpus_support::{corpus_dir, hex_to_bytes, load_jsonl, SignalGolden};
use obd2_dash::gm_enhanced::{decode_did_value, find_lly_did, LLY_REJECTED_DIDS};
use obd2_dash::profiles::gm::lly::{decode_lly_signal, LLY_SIGNALS};

const PROFILE_ID: &str = "gm.gmt800.lly.class2";

#[test]
fn every_signal_golden_decodes_identically() {
    let goldens = signal_goldens();
    let mut dids = BTreeSet::new();

    for golden in &goldens {
        dids.insert(golden.did);
        decode_signal_golden(golden);
    }

    assert_eq!(dids, BTreeSet::from([0x1540_u16, 0x1543_u16, 0x162F_u16]));
}

#[test]
fn corpus_dids_are_all_known_lly() {
    let goldens = signal_goldens();
    assert!(!goldens.is_empty(), "signal corpus must not be empty");

    for golden in goldens {
        assert!(
            find_lly_did(golden.did).is_some(),
            "DID 0x{:04X} must resolve in current LLY definitions",
            golden.did
        );
        assert!(
            !LLY_REJECTED_DIDS
                .iter()
                .any(|entry| entry.did == golden.did),
            "DID 0x{:04X} must not be a rejected LLY DID",
            golden.did
        );
    }
}

fn signal_goldens() -> Vec<SignalGolden> {
    load_jsonl(
        &corpus_dir().join("profile").join("gm.gmt800.lly.class2"),
        "signal-",
    )
}

fn decode_signal_golden(golden: &SignalGolden) {
    assert_eq!(golden.profile_id, PROFILE_ID);
    assert_eq!(golden.service_id, 0x22);
    assert!(
        matches!(golden.module.as_str(), "ecm" | "tcm"),
        "invalid module route `{}`",
        golden.module
    );

    let definition = find_lly_did(golden.did)
        .unwrap_or_else(|| panic!("LLY DID 0x{:04X} must resolve", golden.did));
    let payload = hex_to_bytes(&golden.payload_hex);
    let decoded = decode_did_value(definition, &payload)
        .unwrap_or_else(|err| panic!("DID 0x{:04X} decode failed: {err}", golden.did));

    assert_eq!(decoded.selected_raw, golden.expected.selected_raw);
    assert_eq!(decoded.value.to_bits(), golden.expected.value.to_bits());
    assert_eq!(decoded.unit, golden.expected.unit.as_str());

    let signal = LLY_SIGNALS
        .iter()
        .find(|signal| {
            u16::from_be_bytes([signal.request_data[0], signal.request_data[1]]) == golden.did
        })
        .unwrap_or_else(|| panic!("LLY signal 0x{:04X} must resolve", golden.did));
    if let Some(signal_key) = golden.signal_key.as_deref() {
        assert_eq!(signal.key, signal_key);
    }
    let profile_decoded = decode_lly_signal(signal, &payload)
        .unwrap_or_else(|err| panic!("profile DID 0x{:04X} decode failed: {err:?}", golden.did));
    assert_eq!(
        selected_raw_u32(&profile_decoded.selected_raw),
        golden.expected.selected_raw
    );
    assert_eq!(
        profile_decoded.value.to_bits(),
        golden.expected.value.to_bits()
    );
    assert_eq!(profile_decoded.unit, golden.expected.unit.as_str());
    assert_eq!(profile_decoded.raw, payload);
    assert_eq!(profile_decoded.module.0, golden.module);
}

fn selected_raw_u32(bytes: &[u8]) -> u32 {
    let mut raw = 0u32;
    for byte in bytes {
        raw = (raw << 8) | u32::from(*byte);
    }
    raw
}
