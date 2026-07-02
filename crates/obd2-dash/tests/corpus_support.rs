#![allow(dead_code)]

use std::fmt::Write as _;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignalGolden {
    pub capture: String,
    pub profile_id: String,
    pub service_id: u8,
    pub did: u16,
    #[serde(default)]
    pub signal_key: Option<String>,
    pub module: String,
    pub request_hex: String,
    pub request_header_hex: String,
    pub payload_hex: String,
    pub expected: SignalExpected,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SignalExpected {
    pub selected_raw: u32,
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DtcGolden {
    pub source: String,
    pub profile_id: String,
    pub payload_hex: String,
    pub expected: Vec<DtcExpected>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DtcExpected {
    pub code: String,
    pub gm_status_raw: u8,
    pub generic_status: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PayloadGolden {
    pub capture: String,
    pub raw_response_text: String,
    pub family: String,
    pub skip_bytes: usize,
    pub echo_command: String,
    pub expected_payload_hex: String,
}

pub fn load_jsonl<T: for<'de> Deserialize<'de>>(dir: &Path, name_prefix: &str) -> Vec<T> {
    let mut paths = Vec::new();
    collect_jsonl_paths(dir, name_prefix, &mut paths)
        .unwrap_or_else(|err| panic!("failed to scan {}: {err}", dir.display()));
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let file = fs::File::open(&path)
            .unwrap_or_else(|err| panic!("failed to open {}: {err}", path.display()));
        let reader = BufReader::new(file);
        for (line_index, line) in reader.lines().enumerate() {
            let line =
                line.unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value = serde_json::from_str(trimmed).unwrap_or_else(|err| {
                panic!(
                    "failed to parse {}:{}: {err}",
                    path.display(),
                    line_index + 1
                )
            });
            out.push(value);
        }
    }
    out
}

pub fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    let compact: String = s.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    if !compact.len().is_multiple_of(2) {
        panic!("hex string has odd length: {s}");
    }

    let mut bytes = Vec::with_capacity(compact.len() / 2);
    for index in (0..compact.len()).step_by(2) {
        let token = &compact[index..index + 2];
        let byte = u8::from_str_radix(token, 16)
            .unwrap_or_else(|_| panic!("invalid hex byte `{token}` in `{s}`"));
        bytes.push(byte);
    }
    bytes
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut out, "{byte:02X}").expect("write to string");
    }
    out
}

fn collect_jsonl_paths(
    dir: &Path,
    name_prefix: &str,
    out: &mut Vec<PathBuf>,
) -> std::io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_jsonl_paths(&path, name_prefix, out)?;
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let is_jsonl = path.extension().and_then(|ext| ext.to_str()) == Some("jsonl");
        let prefix_matches = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(name_prefix));
        if is_jsonl && prefix_matches {
            out.push(path);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        for sample in ["621540E2", "8022", "59437993D02412"] {
            assert_eq!(bytes_to_hex(&hex_to_bytes(sample)), sample);
        }
        assert_eq!(bytes_to_hex(&hex_to_bytes("62 15 40 E2")), "621540E2");
        assert!(std::panic::catch_unwind(|| hex_to_bytes("ABC")).is_err());
        assert!(std::panic::catch_unwind(|| hex_to_bytes("ZZ")).is_err());
    }

    #[test]
    fn load_jsonl_rejects_malformed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("signal-bad.jsonl");
        fs::write(&path, "{not json}\n").expect("write malformed fixture");

        let panic = std::panic::catch_unwind(|| load_jsonl::<SignalGolden>(dir.path(), ""))
            .expect_err("malformed JSONL must panic");
        let message = panic_message(panic);
        assert!(message.contains(path.to_str().expect("utf-8 temp path")));
        assert!(message.contains(":1:"));
    }

    fn panic_message(panic: Box<dyn std::any::Any + Send>) -> String {
        if let Some(message) = panic.downcast_ref::<String>() {
            message.clone()
        } else if let Some(message) = panic.downcast_ref::<&str>() {
            (*message).to_string()
        } else {
            "<non-string panic>".to_string()
        }
    }
}
