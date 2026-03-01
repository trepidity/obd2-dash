use std::io::{self, Read, Write};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Magic bytes identifying an OBD2 recording file (v1).
pub const MAGIC: &[u8; 8] = b"OBD2REC\x01";
/// Magic bytes for v2 format (includes raw hex bytes per frame).
pub const MAGIC_V2: &[u8; 8] = b"OBD2REC\x02";

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

/// A single recorded frame (14 bytes on disk for v1; 14 + 1 + N for v2 with raw bytes).
#[derive(Debug, Clone)]
pub struct RecordingFrame {
    pub frame_type: u8,
    pub offset_ms: u32,
    pub pid_code: u8,
    pub value: f64,
    pub raw_bytes: Vec<u8>,
}

impl RecordingFrame {
    pub fn pid(offset_ms: u32, pid_code: u8, value: f64) -> Self {
        Self {
            frame_type: FRAME_PID,
            offset_ms,
            pid_code,
            value,
            raw_bytes: vec![],
        }
    }

    pub fn pid_with_raw(offset_ms: u32, pid_code: u8, value: f64, raw_bytes: &[u8]) -> Self {
        Self {
            frame_type: FRAME_PID,
            offset_ms,
            pid_code,
            value,
            raw_bytes: raw_bytes.to_vec(),
        }
    }

    pub fn voltage(offset_ms: u32, value: f64) -> Self {
        Self {
            frame_type: FRAME_VOLTAGE,
            offset_ms,
            pid_code: 0,
            value,
            raw_bytes: vec![],
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
            raw_bytes: vec![],
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

    /// Write a frame to a binary stream (v2: 14 bytes + 1 byte length + raw bytes).
    pub fn write_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.frame_type])?;
        writer.write_all(&self.offset_ms.to_le_bytes())?;
        writer.write_all(&[self.pid_code])?;
        writer.write_all(&self.value.to_le_bytes())?;
        // v2: write raw bytes length (u8) + raw bytes
        let len = self.raw_bytes.len().min(255) as u8;
        writer.write_all(&[len])?;
        if len > 0 {
            writer.write_all(&self.raw_bytes[..len as usize])?;
        }
        Ok(())
    }

    /// Read a frame from a binary stream.
    /// `version`: 1 = v1 (14 bytes only), 2 = v2 (14 bytes + raw bytes).
    pub fn read_from<R: Read>(reader: &mut R, version: u8) -> io::Result<Option<Self>> {
        let mut buf = [0u8; 14];
        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let frame_type = buf[0];
        let offset_ms = u32::from_le_bytes([buf[1], buf[2], buf[3], buf[4]]);
        let pid_code = buf[5];
        let value = f64::from_le_bytes([
            buf[6], buf[7], buf[8], buf[9], buf[10], buf[11], buf[12], buf[13],
        ]);

        let raw_bytes = if version >= 2 {
            let mut len_buf = [0u8; 1];
            reader.read_exact(&mut len_buf)?;
            let len = len_buf[0] as usize;
            if len > 0 {
                let mut raw = vec![0u8; len];
                reader.read_exact(&mut raw)?;
                raw
            } else {
                vec![]
            }
        } else {
            vec![]
        };

        Ok(Some(RecordingFrame {
            frame_type,
            offset_ms,
            pid_code,
            value,
            raw_bytes,
        }))
    }
}

/// Write the file header (v2 magic + JSON session header).
pub fn write_file_header<W: Write>(writer: &mut W, header: &SessionHeader) -> io::Result<()> {
    writer.write_all(MAGIC_V2)?;
    let json = serde_json::to_vec(header).map_err(io::Error::other)?;
    let len = json.len() as u32;
    writer.write_all(&len.to_le_bytes())?;
    writer.write_all(&json)?;
    Ok(())
}

/// Read the file header (accepts v1 or v2 magic). Returns (header, version).
pub fn read_file_header<R: Read>(reader: &mut R) -> io::Result<(SessionHeader, u8)> {
    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic)?;

    let version = if &magic == MAGIC {
        1
    } else if &magic == MAGIC_V2 {
        2
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid recording file: bad magic bytes",
        ));
    };

    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut json_buf = vec![0u8; len];
    reader.read_exact(&mut json_buf)?;

    let header: SessionHeader = serde_json::from_slice(&json_buf)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok((header, version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_roundtrip_no_raw() {
        let frame = RecordingFrame::pid(1234, 0x0C, 3500.0);
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();
        // v2: 14 bytes + 1 byte (raw_bytes len=0)
        assert_eq!(buf.len(), 15);

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor, 2).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_PID);
        assert_eq!(decoded.offset_ms, 1234);
        assert_eq!(decoded.pid_code, 0x0C);
        assert!((decoded.value - 3500.0).abs() < 0.001);
        assert!(decoded.raw_bytes.is_empty());
    }

    #[test]
    fn test_frame_roundtrip_with_raw() {
        let raw = vec![0x0C, 0x80];
        let frame = RecordingFrame::pid_with_raw(1234, 0x0C, 3500.0, &raw);
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();
        // v2: 14 bytes + 1 byte len + 2 raw bytes = 17
        assert_eq!(buf.len(), 17);

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor, 2).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_PID);
        assert_eq!(decoded.offset_ms, 1234);
        assert_eq!(decoded.pid_code, 0x0C);
        assert!((decoded.value - 3500.0).abs() < 0.001);
        assert_eq!(decoded.raw_bytes, vec![0x0C, 0x80]);
    }

    #[test]
    fn test_v1_frame_read() {
        // Simulate a v1 frame (14 bytes only, no raw_bytes trailer)
        let frame = RecordingFrame::pid(1234, 0x0C, 3500.0);
        let mut buf = Vec::new();
        // Write only the 14-byte v1 portion manually
        buf.push(frame.frame_type);
        buf.extend_from_slice(&frame.offset_ms.to_le_bytes());
        buf.push(frame.pid_code);
        buf.extend_from_slice(&frame.value.to_le_bytes());
        assert_eq!(buf.len(), 14);

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor, 1).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_PID);
        assert_eq!(decoded.offset_ms, 1234);
        assert_eq!(decoded.pid_code, 0x0C);
        assert!((decoded.value - 3500.0).abs() < 0.001);
        assert!(decoded.raw_bytes.is_empty());
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
        let (decoded, version) = read_file_header(&mut cursor).unwrap();
        assert_eq!(version, 2);
        assert_eq!(decoded.session_id, "test-id");
        assert_eq!(decoded.vin, Some("WMW12345".to_string()));
    }
}
