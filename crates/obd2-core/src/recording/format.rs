use std::io::{self, Read, Write};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Magic bytes identifying an OBD2 recording file.
pub const MAGIC: &[u8; 8] = b"OBD2REC\x01";

/// Frame type markers.
pub const FRAME_PID: u8 = 0x01;
pub const FRAME_VOLTAGE: u8 = 0x02;
pub const FRAME_DTC: u8 = 0x03;

/// Session header stored as JSON after the magic bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    pub session_id: String,
    pub start_time: DateTime<Utc>,
    pub vin: Option<String>,
    pub vehicle_name: Option<String>,
    pub poll_interval_ms: u64,
}

/// A single recorded frame (14 bytes on disk for PID/Voltage frames).
#[derive(Debug, Clone)]
pub struct RecordingFrame {
    pub frame_type: u8,
    pub offset_ms: u32,
    pub pid_code: u8,
    pub value: f64,
}

impl RecordingFrame {
    pub fn pid(offset_ms: u32, pid_code: u8, value: f64) -> Self {
        Self {
            frame_type: FRAME_PID,
            offset_ms,
            pid_code,
            value,
        }
    }

    pub fn voltage(offset_ms: u32, value: f64) -> Self {
        Self {
            frame_type: FRAME_VOLTAGE,
            offset_ms,
            pid_code: 0,
            value,
        }
    }

    pub fn dtc(offset_ms: u32, dtc_code: &str) -> Self {
        // Encode DTC code as a hash into the value field for compactness.
        // We use the first 5 chars of the DTC code (e.g., "P0301") as bytes.
        let bytes = dtc_code.as_bytes();
        let mut val_bytes = [0u8; 8];
        let len = bytes.len().min(8);
        val_bytes[..len].copy_from_slice(&bytes[..len]);
        let value = f64::from_le_bytes(val_bytes);
        Self {
            frame_type: FRAME_DTC,
            offset_ms,
            pid_code: 0,
            value,
        }
    }

    /// Decode DTC code from a DTC frame's value field.
    pub fn decode_dtc_code(&self) -> Option<String> {
        if self.frame_type != FRAME_DTC {
            return None;
        }
        let bytes = self.value.to_le_bytes();
        // Find the end of the string (first null byte or end)
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8(bytes[..end].to_vec()).ok()
    }

    /// Write a frame to a binary stream (14 bytes).
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.frame_type])?;
        writer.write_all(&self.offset_ms.to_le_bytes())?;
        writer.write_all(&[self.pid_code])?;
        writer.write_all(&self.value.to_le_bytes())?;
        Ok(())
    }

    /// Read a frame from a binary stream (14 bytes).
    pub fn read_from<R: Read>(reader: &mut R) -> io::Result<Option<Self>> {
        let mut buf = [0u8; 14];
        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let frame_type = buf[0];
        let offset_ms = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
        let pid_code = buf[5];
        let value = f64::from_le_bytes([buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13]]);

        Ok(Some(RecordingFrame {
            frame_type,
            offset_ms,
            pid_code,
            value,
        }))
    }
}

/// Write the file header (magic + JSON session header).
pub fn write_file_header<W: Write>(writer: &mut W, header: &SessionHeader) -> io::Result<()> {
    writer.write_all(MAGIC)?;
    let json = serde_json::to_vec(header).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    let len = json.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&json)?;
    Ok(())
}

/// Read the file header (magic + JSON session header).
pub fn read_file_header<R: Read>(reader: &mut R) -> io::Result<SessionHeader> {
    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid recording file: bad magic bytes",
        ));
    }

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut json_buf = vec![0u8; len];
    reader.read_exact(&mut json_buf)?;

    let header: SessionHeader =
        serde_json::from_slice(&json_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip() {
        let frame = RecordingFrame::pid(1234, 0x0C, 3500.0);
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();
        assert_eq!(buf.len(), 14);

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_PID);
        assert_eq!(decoded.offset_ms, 1234);
        assert_eq!(decoded.pid_code, 0x0C);
        assert!((decoded.value - 3500.0).abs() < 0.001);
    }

    #[test]
    fn test_dtc_code_roundtrip() {
        let frame = RecordingFrame::dtc(5000, "P0301");
        let code = frame.decode_dtc_code().unwrap();
        assert_eq!(code, "P0301");
    }

    #[test]
    fn test_header_roundtrip() {
        let header = SessionHeader {
            session_id: "test-id".to_string(),
            start_time: Utc::now(),
            vin: Some("WMW12345".to_string()),
            vehicle_name: Some("Test Car".to_string()),
            poll_interval_ms: 250,
        };

        let mut buf = Vec::new();
        write_file_header(&mut buf, &header).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded = read_file_header(&mut cursor).unwrap();
        assert_eq!(decoded.session_id, "test-id");
        assert_eq!(decoded.vin, Some("WMW12345".to_string()));
    }
}
