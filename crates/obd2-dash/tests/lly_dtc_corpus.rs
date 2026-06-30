mod corpus_support;

use corpus_support::{corpus_dir, hex_to_bytes, load_jsonl, DtcGolden};
use obd2_dash::gm_class2::decode_class2_dtcs;

const PROFILE_ID: &str = "gm.gmt800.lly.class2";

#[test]
fn synthetic_dtc_goldens_decode_identically() {
    let goldens: Vec<DtcGolden> = load_jsonl(
        &corpus_dir().join("profile").join("gm.gmt800.lly.class2"),
        "dtc-",
    );
    assert!(!goldens.is_empty(), "DTC corpus must not be empty");

    for golden in goldens {
        assert_eq!(golden.profile_id, PROFILE_ID);
        assert_eq!(golden.source, "synthetic");

        let payload = hex_to_bytes(&golden.payload_hex);
        let records = decode_class2_dtcs(&payload)
            .unwrap_or_else(|err| panic!("DTC payload {} failed: {err}", golden.payload_hex));

        assert_eq!(records.len(), golden.expected.len());
        for (record, expected) in records.iter().zip(&golden.expected) {
            assert_eq!(record.dtc.code, expected.code);
            assert_eq!(record.status.raw, expected.gm_status_raw);
            assert_eq!(
                format!("{:?}", record.status.generic_status()),
                expected.generic_status
            );
        }
    }
}
