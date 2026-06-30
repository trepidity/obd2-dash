mod corpus_support;

use corpus_support::{corpus_dir, hex_to_bytes, load_jsonl, PayloadGolden};
use obd2_core::protocol::codec::decode_elm_response_payload_for_command;
use obd2_core::protocol::BusFamily;

#[test]
fn strip_is_byte_stable() {
    let goldens: Vec<PayloadGolden> = load_jsonl(&corpus_dir().join("protocol"), "");
    assert!(
        !goldens.is_empty(),
        "protocol payload corpus must not be empty"
    );

    for golden in goldens {
        let decoded = decode_elm_response_payload_for_command(
            &golden.raw_response_text,
            bus_family(&golden.family),
            golden.skip_bytes,
            Some(&golden.echo_command),
        )
        .unwrap_or_else(|err| {
            panic!(
                "protocol strip failed for {} {}: {err}",
                golden.capture, golden.echo_command
            )
        });

        assert_eq!(decoded, hex_to_bytes(&golden.expected_payload_hex));
    }
}

fn bus_family(family: &str) -> BusFamily {
    match family {
        "J1850" => BusFamily::J1850,
        "CAN" => BusFamily::Can,
        "ISO9141" => BusFamily::Iso9141,
        "KWP2000" => BusFamily::Kwp2000,
        other => panic!("unsupported corpus bus family `{other}`"),
    }
}
