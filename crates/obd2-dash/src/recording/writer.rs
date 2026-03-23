use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use chrono::Utc;

use super::format::{write_file_header, RecordingFrame, SessionHeader};

/// Append-only writer for recording OBD2 data to a binary file.
pub struct RecordingWriter {
    writer: BufWriter<File>,
    pub file_path: PathBuf,
    pub session_id: String,
    pub frame_count: u64,
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
        std::fs::create_dir_all(recordings_dir)?;
        let file_path = recordings_dir.join(format!("{}.obd2rec", session_id));
        let file = File::create(&file_path)?;
        let mut writer = BufWriter::new(file);

        let header = SessionHeader {
            session_id: session_id.to_string(),
            start_time: Utc::now(),
            vin,
            vehicle_name,
            poll_interval_ms,
        };

        write_file_header(&mut writer, &header)?;

        Ok(Self {
            writer,
            file_path,
            session_id: session_id.to_string(),
            frame_count: 0,
        })
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
        frame.write_to(&mut self.writer)?;
        self.frame_count += 1;
        Ok(())
    }

    /// Write a voltage frame.
    pub fn write_voltage(&mut self, offset_ms: u32, value: f64) -> std::io::Result<()> {
        let frame = RecordingFrame::voltage(offset_ms, value);
        frame.write_to(&mut self.writer)?;
        self.frame_count += 1;
        Ok(())
    }

    /// Write a DTC frame.
    pub fn write_dtc(&mut self, offset_ms: u32, dtc_code: &str) -> std::io::Result<()> {
        let frame = RecordingFrame::dtc(offset_ms, dtc_code);
        frame.write_to(&mut self.writer)?;
        self.frame_count += 1;
        Ok(())
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
        frame.write_to(&mut self.writer)?;
        self.frame_count += 1;
        Ok(())
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
        frame.write_to(&mut self.writer)?;
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
