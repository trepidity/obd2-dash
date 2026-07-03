use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::format::{write_file_header, write_file_header_v3, RecordingFrame, SessionHeader};

/// Append-only writer for recording OBD2 data to a binary file.
pub struct RecordingWriter {
    writer: BufWriter<File>,
    pub file_path: PathBuf,
    pub session_id: String,
    pub frame_count: u64,
    version: u8,
    profile_id: Option<String>,
}

struct RecordingWriterConfig<'a> {
    recordings_dir: &'a Path,
    session_id: &'a str,
    vin: Option<String>,
    vehicle_name: Option<String>,
    poll_interval_ms: u64,
    version: u8,
    profile_id: Option<String>,
    identity_confidence: Option<String>,
}

impl RecordingWriter {
    /// Create a new recording file and write the session header.
    pub fn new(
        recordings_dir: &Path,
        session_id: &str,
        vin: Option<String>,
        vehicle_name: Option<String>,
        poll_interval_ms: u64,
    ) -> std::io::Result<Self> {
        Self::new_with_config(RecordingWriterConfig {
            recordings_dir,
            session_id,
            vin,
            vehicle_name,
            poll_interval_ms,
            version: 2,
            profile_id: None,
            identity_confidence: None,
        })
    }

    /// Create a v3 recording file. Default app recording still uses v2 until
    /// the v3 writer is explicitly wired behind a config flag.
    pub fn new_v3(
        recordings_dir: &Path,
        session_id: &str,
        vin: Option<String>,
        vehicle_name: Option<String>,
        poll_interval_ms: u64,
    ) -> std::io::Result<Self> {
        Self::new_v3_with_profile(
            recordings_dir,
            session_id,
            vin,
            vehicle_name,
            poll_interval_ms,
            None,
            None,
        )
    }

    pub fn new_v3_with_profile(
        recordings_dir: &Path,
        session_id: &str,
        vin: Option<String>,
        vehicle_name: Option<String>,
        poll_interval_ms: u64,
        profile_id: Option<String>,
        identity_confidence: Option<String>,
    ) -> std::io::Result<Self> {
        Self::new_with_config(RecordingWriterConfig {
            recordings_dir,
            session_id,
            vin,
            vehicle_name,
            poll_interval_ms,
            version: 3,
            profile_id,
            identity_confidence,
        })
    }

    fn new_with_config(config: RecordingWriterConfig<'_>) -> std::io::Result<Self> {
        std::fs::create_dir_all(config.recordings_dir)?;
        let file_path = config
            .recordings_dir
            .join(format!("{}.obd2rec", config.session_id));
        let file = File::create(&file_path)?;
        let mut writer = BufWriter::new(file);

        let profile_id = config.profile_id.clone();
        let header = SessionHeader {
            session_id: config.session_id.to_string(),
            start_time: Utc::now(),
            vin: config.vin,
            vehicle_name: config.vehicle_name,
            poll_interval_ms: config.poll_interval_ms,
            profile_id: config.profile_id,
            identity_confidence: config.identity_confidence,
        };

        match config.version {
            2 => write_file_header(&mut writer, &header)?,
            3 => write_file_header_v3(&mut writer, &header)?,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "unsupported recording writer version",
                ))
            }
        }

        Ok(Self {
            writer,
            file_path,
            session_id: config.session_id.to_string(),
            frame_count: 0,
            version: config.version,
            profile_id,
        })
    }

    pub fn profile_id(&self) -> Option<&str> {
        self.profile_id.as_deref()
    }

    /// Write a PID frame with optional raw hex bytes.
    pub fn write_pid(
        &mut self,
        offset_ms: u32,
        pid_code: u8,
        value: f64,
        raw_bytes: &[u8],
    ) -> std::io::Result<()> {
        let frame = RecordingFrame::pid_with_raw(offset_ms, pid_code, value, raw_bytes);
        self.write_frame(&frame)
    }

    /// Write a voltage frame.
    pub fn write_voltage(&mut self, offset_ms: u32, value: f64) -> std::io::Result<()> {
        let frame = RecordingFrame::voltage(offset_ms, value);
        self.write_frame(&frame)
    }

    /// Write a DTC frame.
    pub fn write_dtc(&mut self, offset_ms: u32, dtc_code: &str) -> std::io::Result<()> {
        let frame = RecordingFrame::dtc(offset_ms, dtc_code);
        self.write_frame(&frame)
    }

    /// Write an enhanced PID frame.
    pub fn write_enhanced(
        &mut self,
        offset_ms: u32,
        did: u16,
        module: &str,
        name: &str,
        unit: &str,
        value: f64,
    ) -> std::io::Result<()> {
        let frame = RecordingFrame::enhanced(offset_ms, did, module, name, unit, value);
        self.write_frame(&frame)
    }

    /// Write an O2 sensor monitoring frame.
    pub fn write_o2(
        &mut self,
        offset_ms: u32,
        test_name: &str,
        sensor: &str,
        unit: &str,
        value: f64,
    ) -> std::io::Result<()> {
        let frame = RecordingFrame::o2(offset_ms, test_name, sensor, unit, value);
        self.write_frame(&frame)
    }

    /// Write an owned profile evidence frame. Returns false for v2 files, where
    /// the evidence would exceed the one-byte raw payload envelope.
    pub fn write_profile_evidence(
        &mut self,
        offset_ms: u32,
        record: &obd2_dash::profiles::ProfileEvidenceRecord,
    ) -> std::io::Result<bool> {
        if self.version < 3 {
            return Ok(false);
        }
        let frame = RecordingFrame::profile_evidence(offset_ms, record)?;
        self.write_frame(&frame)?;
        Ok(true)
    }

    /// Write an active-test attempt frame. Returns false for v2 files, where
    /// the JSON evidence would exceed the one-byte raw payload envelope.
    pub fn write_active_test_attempt(
        &mut self,
        offset_ms: u32,
        record: &obd2_dash::gm_evidence::GmEvidenceRecord,
    ) -> std::io::Result<bool> {
        if self.version < 3 {
            return Ok(false);
        }
        let frame = RecordingFrame::active_test_attempt(offset_ms, record)?;
        self.write_frame(&frame)?;
        Ok(true)
    }

    fn write_frame(&mut self, frame: &RecordingFrame) -> std::io::Result<()> {
        if self.version >= 3 {
            frame.write_v3_to(&mut self.writer)?;
        } else {
            frame.write_to(&mut self.writer)?;
        }
        self.frame_count += 1;
        Ok(())
    }

    /// Flush any buffered data and finalize the recording.
    pub fn finish(mut self) -> std::io::Result<PathBuf> {
        use std::io::Write;
        self.writer.flush()?;
        tracing::info!(
            "Recording finished: {} frames written to {}",
            self.frame_count,
            self.file_path.display()
        );
        Ok(self.file_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use obd2_dash::gm_active::{
        active_test_evidence_record, blocked_active_test_result, GmActiveTestCommand,
    };
    use obd2_dash::profiles::{ProfileDecodedEvidence, ProfileEvidenceRecord, RouteEvidence};

    fn sample_profile_record() -> ProfileEvidenceRecord {
        ProfileEvidenceRecord {
            timestamp: Utc::now(),
            profile_id: "gm.gmt800.lly.class2".to_string(),
            capability_id: "vgt_vane_actual".to_string(),
            capability_kind: "signal".to_string(),
            module: "ecm".to_string(),
            route: RouteEvidence::J1850 {
                node: 0x10,
                header: vec![0x6C, 0x10, 0xF1],
            },
            service_id: 0x22,
            request_data: vec![0x15, 0x43, 0x01],
            raw_adapter_write_text: None,
            raw_adapter_read_text: None,
            parsed_response_bytes: vec![0x62, 0x15, 0x43, 0xE1],
            decoder_id: "gm-lly-mode22".to_string(),
            identity_confidence: Some("confirmed".to_string()),
            manual_confirmation: false,
            probe: false,
            source_fields: None,
            decoded: Some(ProfileDecodedEvidence::Signal {
                key: "vgt_vane_actual".to_string(),
                label: "VGT Vane Position Actual".to_string(),
                did: Some(0x1543),
                value: 88.2,
                unit: "%".to_string(),
                raw: vec![0xE1],
                selected_raw: vec![0xE1],
                confidence: "community".to_string(),
            }),
            error: None,
        }
    }

    #[test]
    fn v2_writer_skips_profile_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let mut writer = RecordingWriter::new(dir.path(), "v2", None, None, 250).unwrap();
        let wrote = writer
            .write_profile_evidence(100, &sample_profile_record())
            .unwrap();

        assert!(!wrote);
        assert_eq!(writer.frame_count, 0);
    }

    #[test]
    fn v3_writer_roundtrips_profile_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let mut record = sample_profile_record();
        record.parsed_response_bytes = vec![0xAB; 512];

        let mut writer = RecordingWriter::new_v3_with_profile(
            dir.path(),
            "v3",
            None,
            None,
            250,
            Some(record.profile_id.clone()),
            Some("confirmed".to_string()),
        )
        .unwrap();
        let wrote = writer.write_profile_evidence(100, &record).unwrap();
        let path = writer.finish().unwrap();

        assert!(wrote);
        let (header, frames) = crate::recording::reader::read_recording(&path).unwrap();
        assert_eq!(header.profile_id, Some("gm.gmt800.lly.class2".to_string()));
        assert_eq!(header.identity_confidence, Some("confirmed".to_string()));
        assert_eq!(frames.len(), 1);
        let decoded = frames[0]
            .decode_profile_evidence()
            .expect("profile evidence");
        assert_eq!(decoded.profile_id, "gm.gmt800.lly.class2");
        assert_eq!(decoded.parsed_response_bytes.len(), 512);
    }

    #[test]
    fn v3_writer_roundtrips_active_test_attempt() {
        let dir = tempfile::tempdir().unwrap();
        let command = GmActiveTestCommand::VgtTcLearn { enabled: true };
        let result = blocked_active_test_result(&command);
        let record = active_test_evidence_record(&command, &result);

        let mut writer = RecordingWriter::new_v3(dir.path(), "active", None, None, 250).unwrap();
        let wrote = writer.write_active_test_attempt(120, &record).unwrap();
        let path = writer.finish().unwrap();

        assert!(wrote);
        let (_header, frames) = crate::recording::reader::read_recording(&path).unwrap();
        assert_eq!(frames.len(), 1);
        let decoded = frames[0]
            .decode_active_test_attempt()
            .expect("active test attempt");
        assert_eq!(decoded.decoder, "gm-active-test-vgt-vane-control");
        assert_eq!(decoded.node, 0x10);
    }
}
