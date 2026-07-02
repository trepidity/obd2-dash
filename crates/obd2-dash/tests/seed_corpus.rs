mod corpus_support;

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use corpus_support::{
    bytes_to_hex, corpus_dir, hex_to_bytes, DtcExpected, DtcGolden, PayloadGolden, SignalExpected,
    SignalGolden,
};
use obd2_core::adapter::elm_codec::decode_elm_response_payload_for_command;
use obd2_core::protocol::BusFamily;
use obd2_core::transport::parse_raw_capture;
use obd2_dash::gm_class2::decode_class2_dtcs;
use obd2_dash::gm_enhanced::{decode_did_value, find_lly_did};

const PROFILE_ID: &str = "gm.gmt800.lly.class2";

#[test]
#[ignore = "dev-only corpus seeder; writes to tests/corpus/.staging, never the frozen tree"]
fn seed_lly_corpus_from_raw_captures() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let raw_dir = repo_root.join("raw-captures");
    let staging = corpus_dir().join(".staging");

    if staging.exists() {
        fs::remove_dir_all(&staging)
            .unwrap_or_else(|err| panic!("failed to clear {}: {err}", staging.display()));
    }
    fs::create_dir_all(staging.join("protocol/j1850-vpw")).expect("create staging protocol dir");
    fs::create_dir_all(staging.join("profile/gm.gmt800.lly.class2"))
        .expect("create staging profile dir");

    let mut signal_lines = Vec::new();
    let mut payload_lines = Vec::new();
    let mut seen = BTreeSet::new();

    for capture in raw_capture_paths(&raw_dir) {
        let capture_name = capture
            .file_name()
            .and_then(|name| name.to_str())
            .expect("utf-8 capture name")
            .to_string();
        let pairs = parse_raw_capture(&capture)
            .unwrap_or_else(|err| panic!("failed to parse {}: {err}", capture.display()));
        let mut header = String::new();

        for (command, response) in pairs {
            if let Some(next_header) = parse_at_sh(&command) {
                header = next_header;
                continue;
            }
            if !command.starts_with("22") {
                continue;
            }

            let request = hex_to_bytes(&command);
            if request.len() < 3 {
                continue;
            }
            let did = u16::from_be_bytes([request[1], request[2]]);
            let Some(definition) = find_lly_did(did) else {
                continue;
            };
            let Ok(payload) = decode_elm_response_payload_for_command(
                &response,
                BusFamily::J1850,
                3,
                Some(&command),
            ) else {
                continue;
            };
            let Ok(decoded) = decode_did_value(definition, &payload) else {
                continue;
            };

            let payload_hex = bytes_to_hex(&payload);
            if !seen.insert((capture_name.clone(), command.clone(), payload_hex.clone())) {
                continue;
            }

            let signal = SignalGolden {
                capture: capture_name.clone(),
                profile_id: PROFILE_ID.to_string(),
                service_id: request[0],
                did,
                signal_key: None,
                module: module_from_header(&header).to_string(),
                request_hex: bytes_to_hex(&request),
                request_header_hex: header.clone(),
                payload_hex: payload_hex.clone(),
                expected: SignalExpected {
                    selected_raw: decoded.selected_raw,
                    value: decoded.value,
                    unit: decoded.unit.to_string(),
                },
            };
            signal_lines.push(serde_json::to_string(&signal).expect("serialize signal golden"));

            let payload = PayloadGolden {
                capture: capture_name.clone(),
                raw_response_text: response,
                family: "J1850".to_string(),
                skip_bytes: 3,
                echo_command: command,
                expected_payload_hex: payload_hex,
            };
            payload_lines.push(serde_json::to_string(&payload).expect("serialize payload golden"));
        }
    }

    fs::write(
        staging.join("profile/gm.gmt800.lly.class2/signal-seeded.jsonl"),
        signal_lines.join("\n") + "\n",
    )
    .expect("write staged signal corpus");
    fs::write(
        staging.join("protocol/j1850-vpw/payload-seeded.jsonl"),
        payload_lines.join("\n") + "\n",
    )
    .expect("write staged payload corpus");
    fs::write(
        staging.join("profile/gm.gmt800.lly.class2/dtc-synthetic.jsonl"),
        synthetic_dtc_lines().join("\n") + "\n",
    )
    .expect("write staged synthetic DTC corpus");
}

fn raw_capture_paths(raw_dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(raw_dir)
        .unwrap_or_else(|err| panic!("failed to scan {}: {err}", raw_dir.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().and_then(|ext| ext.to_str()) == Some("obd2raw")).then_some(path)
        })
        .collect();
    paths.sort();
    paths
}

fn parse_at_sh(command: &str) -> Option<String> {
    let upper = command.trim().to_ascii_uppercase();
    let header = upper
        .strip_prefix("AT SH ")
        .or_else(|| upper.strip_prefix("ATSH"))?;
    let compact: String = header
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    (compact.len() == 6).then_some(compact)
}

fn module_from_header(header: &str) -> &'static str {
    let bytes = hex_to_bytes(header);
    match bytes.get(1).copied() {
        Some(0x10) => "ecm",
        Some(0x18) => "tcm",
        _ => "unknown",
    }
}

fn synthetic_dtc_lines() -> Vec<String> {
    let payload = "59437993D02412";
    let records = decode_class2_dtcs(&hex_to_bytes(payload)).expect("synthetic DTC payload");
    let expected = records
        .into_iter()
        .map(|record| DtcExpected {
            code: record.dtc.code,
            gm_status_raw: record.status.raw,
            generic_status: format!("{:?}", record.status.generic_status()),
        })
        .collect();
    let golden = DtcGolden {
        source: "synthetic".to_string(),
        profile_id: PROFILE_ID.to_string(),
        payload_hex: payload.to_string(),
        expected,
    };
    vec![serde_json::to_string(&golden).expect("serialize synthetic DTC golden")]
}
