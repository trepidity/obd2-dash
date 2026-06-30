use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GmVehicleIdentity {
    pub vin: Option<String>,
    pub year: Option<u16>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub engine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GmDecodedEvidence {
    Value {
        signal: String,
        raw: u32,
        value: f64,
        unit: String,
    },
    Dtcs {
        records: Vec<GmDtcEvidence>,
    },
    ActiveTest {
        test_id: String,
        command: String,
        accepted: bool,
        status: String,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GmDtcEvidence {
    pub code: String,
    pub gm_status_raw: u8,
    pub gm_status_flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GmEvidenceErrorKind {
    NoData,
    UnsupportedService,
    UnsupportedSubfunction,
    MalformedPayload,
    StaleResponse,
    NegativeResponse,
    Transport,
    Decode,
    Adapter,
    UnverifiedCommand,
    InvalidCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GmEvidenceError {
    pub kind: GmEvidenceErrorKind,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GmEvidenceRecord {
    pub timestamp: DateTime<Utc>,
    pub adapter_port: Option<String>,
    pub protocol: Option<String>,
    pub vehicle: Option<GmVehicleIdentity>,
    pub module_label: String,
    pub node: u8,
    pub request_header: Vec<u8>,
    pub request_service: u8,
    pub request_data: Vec<u8>,
    pub raw_adapter_write_text: String,
    pub raw_adapter_read_text: String,
    pub parsed_response_bytes: Vec<u8>,
    pub decoder: String,
    pub decoded: Option<GmDecodedEvidence>,
    pub decode_confidence: Option<String>,
    pub error: Option<GmEvidenceError>,
}

impl GmEvidenceRecord {
    pub fn routed_request_outcome(
        module_label: impl Into<String>,
        node: u8,
        request_header: [u8; 3],
        request_service: u8,
        request_data: Vec<u8>,
        decoder: impl Into<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            adapter_port: None,
            protocol: None,
            vehicle: None,
            module_label: module_label.into(),
            node,
            request_header: request_header.to_vec(),
            request_service,
            request_data,
            raw_adapter_write_text: String::new(),
            raw_adapter_read_text: String::new(),
            parsed_response_bytes: Vec::new(),
            decoder: decoder.into(),
            decoded: None,
            decode_confidence: None,
            error: None,
        }
    }

    pub fn with_adapter_context(
        mut self,
        adapter_port: Option<String>,
        protocol: Option<String>,
    ) -> Self {
        self.adapter_port = adapter_port;
        self.protocol = protocol;
        self
    }

    pub fn with_vehicle(mut self, vehicle: Option<GmVehicleIdentity>) -> Self {
        self.vehicle = vehicle;
        self
    }

    pub fn with_raw_text(
        mut self,
        raw_adapter_write_text: impl Into<String>,
        raw_adapter_read_text: impl Into<String>,
    ) -> Self {
        self.raw_adapter_write_text = raw_adapter_write_text.into();
        self.raw_adapter_read_text = raw_adapter_read_text.into();
        self
    }

    pub fn with_response_bytes(mut self, parsed_response_bytes: Vec<u8>) -> Self {
        self.parsed_response_bytes = parsed_response_bytes;
        self
    }

    pub fn with_decoded(
        mut self,
        decoded: GmDecodedEvidence,
        decode_confidence: Option<String>,
    ) -> Self {
        self.decoded = Some(decoded);
        self.decode_confidence = decode_confidence;
        self
    }

    pub fn with_error(mut self, kind: GmEvidenceErrorKind, detail: impl Into<String>) -> Self {
        self.error = Some(GmEvidenceError {
            kind,
            detail: detail.into(),
        });
        self
    }
}

pub struct GmEvidenceWriter {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl GmEvidenceWriter {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    pub fn create_raw_capture(prefix: &str) -> io::Result<Self> {
        create_dir_all("raw-captures")?;
        let timestamp = Utc::now().format("%Y%m%d-%H%M%S");
        let file_name = format!("{prefix}-{timestamp}.jsonl");
        Self::create(Path::new("raw-captures").join(file_name))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(&mut self, record: &GmEvidenceRecord) -> io::Result<()> {
        serde_json::to_writer(&mut self.writer, record).map_err(io::Error::other)?;
        self.writer.write_all(b"\n")?;
        Ok(())
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_records_without_forced_flush() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir
            .path()
            .join("raw-captures")
            .join("gm-probe-test.jsonl");
        let mut writer = GmEvidenceWriter::create(&path).unwrap();
        let record = GmEvidenceRecord::routed_request_outcome(
            "ECM/PCM",
            0x10,
            [0x6C, 0x10, 0xF1],
            0x22,
            vec![0x15, 0x42, 0x01],
            "gm-enhanced-mode22",
        )
        .with_adapter_context(
            Some("/dev/cu.test".to_string()),
            Some("J1850 VPW".to_string()),
        )
        .with_raw_text("22154201", "62 15 42 01 64")
        .with_response_bytes(vec![0x01, 0x64])
        .with_decoded(
            GmDecodedEvidence::Value {
                signal: "desired MAP".to_string(),
                raw: 100,
                value: 100.0,
                unit: "kPa abs".to_string(),
            },
            Some("candidate".to_string()),
        );

        writer.append(&record).unwrap();
        writer.flush().unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let decoded: GmEvidenceRecord = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(decoded.module_label, "ECM/PCM");
        assert_eq!(decoded.node, 0x10);
        assert_eq!(decoded.request_header, vec![0x6C, 0x10, 0xF1]);
        assert!(matches!(
            decoded.decoded,
            Some(GmDecodedEvidence::Value { raw: 100, .. })
        ));
    }
}
