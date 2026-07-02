use std::io::{self, Read, Write};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Magic bytes identifying an OBD2 recording file (v1).
pub const MAGIC: &[u8; 8] = b"OBD2REC\x01";
/// Magic bytes for v2 format (includes raw hex bytes per frame).
pub const MAGIC_V2: &[u8; 8] = b"OBD2REC\x02";
/// Magic bytes for v3 format (large payload profile/evidence frames).
pub const MAGIC_V3: &[u8; 8] = b"OBD2REC\x03";

/// Frame type markers.
pub const FRAME_PID: u8 = 0x01;
pub const FRAME_VOLTAGE: u8 = 0x02;
pub const FRAME_DTC: u8 = 0x03;
pub const FRAME_ENHANCED: u8 = 0x04;
pub const FRAME_O2: u8 = 0x05;
pub const FRAME_PROFILE_VALUE: u8 = 0x20;
pub const FRAME_PROFILE_DTC: u8 = 0x21;
pub const FRAME_PROFILE_DISPATCH: u8 = 0x22;
pub const FRAME_ACTIVE_TEST_ATTEMPT: u8 = 0x23;
pub const FRAME_PROFILE_ACTIVE_TEST: u8 = 0x24;

const MAX_V3_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Session header stored as JSON after the magic bytes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHeader {
    pub session_id: String,
    pub start_time: DateTime<Utc>,
    pub vin: Option<String>,
    pub vehicle_name: Option<String>,
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub identity_confidence: Option<String>,
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

    /// Create an enhanced PID frame. Metadata (DID, module, name, unit) is packed into raw_bytes.
    pub fn enhanced(
        offset_ms: u32,
        did: u16,
        module: &str,
        name: &str,
        unit: &str,
        value: f64,
    ) -> Self {
        let mut raw = Vec::new();
        raw.extend_from_slice(&did.to_le_bytes());
        // Pack strings as: len(u8) + bytes
        for s in &[module, name, unit] {
            let bytes = s.as_bytes();
            let len = bytes.len().min(255) as u8;
            raw.push(len);
            raw.extend_from_slice(&bytes[..len as usize]);
        }
        Self {
            frame_type: FRAME_ENHANCED,
            offset_ms,
            pid_code: 0,
            value,
            raw_bytes: raw,
        }
    }

    /// Decode enhanced PID metadata from raw_bytes. Returns (did, module, name, unit).
    pub fn decode_enhanced(&self) -> Option<(u16, String, String, String)> {
        if self.frame_type != FRAME_ENHANCED || self.raw_bytes.len() < 5 {
            return None;
        }
        let did = u16::from_le_bytes([self.raw_bytes[0], self.raw_bytes[1]]);
        let mut pos = 2;
        let mut strings = Vec::new();
        for _ in 0..3 {
            if pos >= self.raw_bytes.len() {
                return None;
            }
            let len = self.raw_bytes[pos] as usize;
            pos += 1;
            if pos + len > self.raw_bytes.len() {
                return None;
            }
            strings.push(String::from_utf8_lossy(&self.raw_bytes[pos..pos + len]).into_owned());
            pos += len;
        }
        Some((did, strings.remove(0), strings.remove(0), strings.remove(0)))
    }

    /// Create an O2 sensor monitoring frame. Metadata (test_name, sensor, unit) packed into raw_bytes.
    pub fn o2(offset_ms: u32, test_name: &str, sensor: &str, unit: &str, value: f64) -> Self {
        let mut raw = Vec::new();
        for s in &[test_name, sensor, unit] {
            let bytes = s.as_bytes();
            let len = bytes.len().min(255) as u8;
            raw.push(len);
            raw.extend_from_slice(&bytes[..len as usize]);
        }
        Self {
            frame_type: FRAME_O2,
            offset_ms,
            pid_code: 0,
            value,
            raw_bytes: raw,
        }
    }

    /// Decode O2 sensor metadata from raw_bytes. Returns (test_name, sensor, unit).
    pub fn decode_o2(&self) -> Option<(String, String, String)> {
        if self.frame_type != FRAME_O2 || self.raw_bytes.is_empty() {
            return None;
        }
        let mut pos = 0;
        let mut strings = Vec::new();
        for _ in 0..3 {
            if pos >= self.raw_bytes.len() {
                return None;
            }
            let len = self.raw_bytes[pos] as usize;
            pos += 1;
            if pos + len > self.raw_bytes.len() {
                return None;
            }
            strings.push(String::from_utf8_lossy(&self.raw_bytes[pos..pos + len]).into_owned());
            pos += len;
        }
        Some((strings.remove(0), strings.remove(0), strings.remove(0)))
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

    /// Create a v3 profile-evidence frame. The payload is JSON and may exceed
    /// the v2 one-byte raw payload limit.
    pub fn profile_evidence(
        offset_ms: u32,
        record: &obd2_dash::profiles::ProfileEvidenceRecord,
    ) -> io::Result<Self> {
        let payload = serde_json::to_vec(record).map_err(io::Error::other)?;
        let frame_type = match &record.decoded {
            Some(obd2_dash::profiles::ProfileDecodedEvidence::Signal { .. }) => FRAME_PROFILE_VALUE,
            Some(obd2_dash::profiles::ProfileDecodedEvidence::Dtcs { .. }) => FRAME_PROFILE_DTC,
            Some(obd2_dash::profiles::ProfileDecodedEvidence::ActiveTest { .. }) => {
                FRAME_PROFILE_ACTIVE_TEST
            }
            None => FRAME_PROFILE_DISPATCH,
        };
        Ok(Self {
            frame_type,
            offset_ms,
            pid_code: 0,
            value: 0.0,
            raw_bytes: payload,
        })
    }

    pub fn decode_profile_evidence(&self) -> Option<obd2_dash::profiles::ProfileEvidenceRecord> {
        if !matches!(
            self.frame_type,
            FRAME_PROFILE_VALUE
                | FRAME_PROFILE_DTC
                | FRAME_PROFILE_DISPATCH
                | FRAME_PROFILE_ACTIVE_TEST
        ) {
            return None;
        }
        serde_json::from_slice(&self.raw_bytes).ok()
    }

    /// Create a v3 active-test attempt frame. The payload is JSON and stores
    /// the refused or accepted command attempt with raw request/response bytes
    /// when they exist.
    pub fn active_test_attempt(
        offset_ms: u32,
        record: &obd2_dash::gm_evidence::GmEvidenceRecord,
    ) -> io::Result<Self> {
        let payload = serde_json::to_vec(record).map_err(io::Error::other)?;
        Ok(Self {
            frame_type: FRAME_ACTIVE_TEST_ATTEMPT,
            offset_ms,
            pid_code: 0,
            value: 0.0,
            raw_bytes: payload,
        })
    }

    pub fn decode_active_test_attempt(&self) -> Option<obd2_dash::gm_evidence::GmEvidenceRecord> {
        if self.frame_type != FRAME_ACTIVE_TEST_ATTEMPT {
            return None;
        }
        serde_json::from_slice(&self.raw_bytes).ok()
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

    /// Write a v3 frame to a binary stream.
    /// Layout: type + offset + pid_code + value + u32 payload length + payload.
    pub fn write_v3_to<W: Write>(&self, writer: &mut W) -> io::Result<()> {
        writer.write_all(&[self.frame_type])?;
        writer.write_all(&self.offset_ms.to_le_bytes())?;
        writer.write_all(&[self.pid_code])?;
        writer.write_all(&self.value.to_le_bytes())?;
        let len = u32::try_from(self.raw_bytes.len()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "v3 frame payload too large")
        })?;
        writer.write_all(&len.to_le_bytes())?;
        writer.write_all(&self.raw_bytes)?;
        Ok(())
    }

    /// Read a frame from a binary stream.
    /// `version`: 1 = v1 (14 bytes only), 2 = v2 (14 bytes + raw bytes),
    /// 3 = v3 (large payload envelope).
    pub fn read_from<R: Read>(reader: &mut R, version: u8) -> io::Result<Option<Self>> {
        if version >= 3 {
            return Self::read_v3_from(reader);
        }

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

    fn read_v3_from<R: Read>(reader: &mut R) -> io::Result<Option<Self>> {
        let mut header = [0u8; 18];
        match reader.read_exact(&mut header) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let frame_type = header[0];
        let offset_ms = u32::from_le_bytes([header[1], header[2], header[3], header[4]]);
        let pid_code = header[5];
        let value = f64::from_le_bytes([
            header[6], header[7], header[8], header[9], header[10], header[11], header[12],
            header[13],
        ]);
        let len = u32::from_le_bytes([header[14], header[15], header[16], header[17]]) as usize;
        if len > MAX_V3_PAYLOAD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "v3 frame payload exceeds safety limit",
            ));
        }

        let mut raw_bytes = vec![0u8; len];
        if len > 0 {
            reader.read_exact(&mut raw_bytes)?;
        }

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
    write_file_header_with_version(writer, header, 2)
}

/// Write the file header using the v3 magic.
pub fn write_file_header_v3<W: Write>(writer: &mut W, header: &SessionHeader) -> io::Result<()> {
    write_file_header_with_version(writer, header, 3)
}

fn write_file_header_with_version<W: Write>(
    writer: &mut W,
    header: &SessionHeader,
    version: u8,
) -> io::Result<()> {
    match version {
        2 => writer.write_all(MAGIC_V2)?,
        3 => writer.write_all(MAGIC_V3)?,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported recording version",
            ))
        }
    }
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
    } else if &magic == MAGIC_V3 {
        3
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
    use obd2_dash::profiles::{
        ProfileDecodedEvidence, ProfileEvidenceError, ProfileEvidenceRecord, RouteEvidence,
    };

    fn profile_active_test_record() -> ProfileEvidenceRecord {
        ProfileEvidenceRecord {
            timestamp: Utc::now(),
            profile_id: "gm.gmt800.lly.class2".to_string(),
            capability_id: "gm.lly.vgt_vane_control".to_string(),
            capability_kind: "active_test".to_string(),
            module: "ecm".to_string(),
            route: RouteEvidence::J1850 {
                node: 0x10,
                header: vec![0x6C, 0x10, 0xF1],
            },
            service_id: 0,
            request_data: Vec::new(),
            raw_adapter_write_text: None,
            raw_adapter_read_text: None,
            parsed_response_bytes: Vec::new(),
            decoder_id: "gm-active-test-vgt-vane-control".to_string(),
            identity_confidence: Some("confirmed".to_string()),
            manual_confirmation: false,
            probe: false,
            source_fields: None,
            decoded: Some(ProfileDecodedEvidence::ActiveTest {
                test_id: "vgt_vane_control".to_string(),
                command: "manual_percent 35".to_string(),
                accepted: false,
                status: "unverified_command_profile".to_string(),
            }),
            error: Some(ProfileEvidenceError {
                kind: "unverified_command".to_string(),
                detail: "missing verified command profile".to_string(),
            }),
        }
    }

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
            profile_id: None,
            identity_confidence: None,
        };

        let mut buf = Vec::new();
        write_file_header(&mut buf, &header).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let (decoded, version) = read_file_header(&mut cursor).unwrap();
        assert_eq!(version, 2);
        assert_eq!(decoded.session_id, "test-id");
        assert_eq!(decoded.vin, Some("WMW12345".to_string()));
    }

    #[test]
    fn test_v3_header_roundtrip() {
        let header = SessionHeader {
            session_id: "test-v3".to_string(),
            start_time: Utc::now(),
            vin: None,
            vehicle_name: None,
            poll_interval_ms: 250,
            profile_id: Some("fixture.can11.readonly.v1".to_string()),
            identity_confidence: Some("confirmed".to_string()),
        };

        let mut buf = Vec::new();
        write_file_header_v3(&mut buf, &header).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let (decoded, version) = read_file_header(&mut cursor).unwrap();
        assert_eq!(version, 3);
        assert_eq!(decoded.session_id, "test-v3");
        assert_eq!(
            decoded.profile_id.as_deref(),
            Some("fixture.can11.readonly.v1")
        );
        assert_eq!(decoded.identity_confidence.as_deref(), Some("confirmed"));
    }

    #[test]
    fn test_legacy_header_json_defaults_profile_metadata() {
        let json = br#"{
            "session_id":"legacy",
            "start_time":"2026-01-01T00:00:00Z",
            "vin":null,
            "vehicle_name":null,
            "poll_interval_ms":250
        }"#;
        let header: SessionHeader = serde_json::from_slice(json).unwrap();

        assert_eq!(header.session_id, "legacy");
        assert!(header.profile_id.is_none());
        assert!(header.identity_confidence.is_none());
    }

    #[test]
    fn test_unknown_frame_type_roundtrip() {
        let frame = RecordingFrame {
            frame_type: 0xFF,
            offset_ms: 999,
            pid_code: 0,
            value: 42.0,
            raw_bytes: vec![0xDE, 0xAD, 0xBE, 0xEF],
        };
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();

        let pid_frame = RecordingFrame::pid(1000, 0x0C, 3500.0);
        pid_frame.write_to(&mut buf).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let first = RecordingFrame::read_from(&mut cursor, 2).unwrap().unwrap();
        assert_eq!(first.frame_type, 0xFF);
        assert_eq!(first.offset_ms, 999);
        assert_eq!(first.raw_bytes, vec![0xDE, 0xAD, 0xBE, 0xEF]);

        let second = RecordingFrame::read_from(&mut cursor, 2).unwrap().unwrap();
        assert_eq!(second.frame_type, FRAME_PID);
        assert_eq!(second.pid_code, 0x0C);
        assert!((second.value - 3500.0).abs() < 0.001);
    }

    #[test]
    fn test_v3_large_payload_roundtrip() {
        let frame = RecordingFrame {
            frame_type: FRAME_PROFILE_DISPATCH,
            offset_ms: 123,
            pid_code: 0,
            value: 0.0,
            raw_bytes: vec![0xAB; 512],
        };
        let mut buf = Vec::new();
        frame.write_v3_to(&mut buf).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor, 3).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_PROFILE_DISPATCH);
        assert_eq!(decoded.offset_ms, 123);
        assert_eq!(decoded.raw_bytes.len(), 512);
        assert_eq!(decoded.raw_bytes[0], 0xAB);
    }

    #[test]
    fn test_v3_unknown_future_frame_roundtrip_then_pid() {
        let mut buf = Vec::new();
        RecordingFrame {
            frame_type: 0xFE,
            offset_ms: 100,
            pid_code: 0,
            value: 0.0,
            raw_bytes: vec![0xAA; 2048],
        }
        .write_v3_to(&mut buf)
        .unwrap();
        RecordingFrame::pid(200, 0x0C, 900.0)
            .write_v3_to(&mut buf)
            .unwrap();

        let mut cursor = io::Cursor::new(buf);
        let unknown = RecordingFrame::read_from(&mut cursor, 3).unwrap().unwrap();
        assert_eq!(unknown.frame_type, 0xFE);
        assert_eq!(unknown.raw_bytes.len(), 2048);

        let pid = RecordingFrame::read_from(&mut cursor, 3).unwrap().unwrap();
        assert_eq!(pid.frame_type, FRAME_PID);
        assert_eq!(pid.pid_code, 0x0C);
        assert_eq!(pid.value, 900.0);
    }

    #[test]
    fn test_profile_active_test_evidence_uses_active_attempt_frame() {
        let frame = RecordingFrame::profile_evidence(300, &profile_active_test_record()).unwrap();

        assert_eq!(frame.frame_type, FRAME_PROFILE_ACTIVE_TEST);
        let decoded = frame.decode_profile_evidence().expect("profile evidence");
        assert_eq!(decoded.profile_id, "gm.gmt800.lly.class2");
        assert_eq!(decoded.capability_id, "gm.lly.vgt_vane_control");
        assert!(matches!(
            decoded.decoded,
            Some(ProfileDecodedEvidence::ActiveTest {
                accepted: false,
                ref status,
                ..
            }) if status == "unverified_command_profile"
        ));
    }

    #[test]
    fn test_enhanced_frame_roundtrip() {
        let frame = RecordingFrame::enhanced(500, 0x1234, "ecm", "Boost Pressure", "kPa", 22.5);
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor, 2).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_ENHANCED);
        assert!((decoded.value - 22.5).abs() < 0.001);

        let (did, module, name, unit) = decoded.decode_enhanced().unwrap();
        assert_eq!(did, 0x1234);
        assert_eq!(module, "ecm");
        assert_eq!(name, "Boost Pressure");
        assert_eq!(unit, "kPa");
    }

    #[test]
    fn test_o2_frame_roundtrip() {
        let frame = RecordingFrame::o2(750, "Catalyst Monitor B1", "Sensor 1", "V", 0.45);
        let mut buf = Vec::new();
        frame.write_to(&mut buf).unwrap();

        let mut cursor = io::Cursor::new(buf);
        let decoded = RecordingFrame::read_from(&mut cursor, 2).unwrap().unwrap();
        assert_eq!(decoded.frame_type, FRAME_O2);
        assert!((decoded.value - 0.45).abs() < 0.001);

        let (test_name, sensor, unit) = decoded.decode_o2().unwrap();
        assert_eq!(test_name, "Catalyst Monitor B1");
        assert_eq!(sensor, "Sensor 1");
        assert_eq!(unit, "V");
    }

    #[test]
    fn test_mixed_frame_stream_ordering() {
        let mut buf = Vec::new();
        RecordingFrame::pid_with_raw(100, 0x0C, 680.0, &[0x0A, 0xA0])
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::voltage(200, 14.4)
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::enhanced(300, 0xABCD, "tcm", "Trans Temp", "°C", 85.0)
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::dtc(400, "P0420")
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::o2(500, "O2 Monitor", "B1S1", "V", 0.72)
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame {
            frame_type: 0xFE,
            offset_ms: 600,
            pid_code: 0,
            value: 0.0,
            raw_bytes: vec![1, 2, 3],
        }
        .write_to(&mut buf)
        .unwrap();
        RecordingFrame::pid(700, 0x0D, 60.0)
            .write_to(&mut buf)
            .unwrap();

        let mut cursor = io::Cursor::new(buf);
        let mut offsets = Vec::new();
        while let Some(frame) = RecordingFrame::read_from(&mut cursor, 2).unwrap() {
            offsets.push(frame.offset_ms);
        }
        assert_eq!(offsets, vec![100, 200, 300, 400, 500, 600, 700]);
    }

    #[test]
    fn test_full_file_roundtrip_all_frame_types() {
        let mut buf = Vec::new();

        let header = SessionHeader {
            session_id: "test-rule5".to_string(),
            start_time: chrono::Utc::now(),
            vin: Some("1GCHK23164F000001".to_string()),
            vehicle_name: Some("Test Duramax".to_string()),
            poll_interval_ms: 250,
            profile_id: None,
            identity_confidence: None,
        };
        write_file_header(&mut buf, &header).unwrap();

        RecordingFrame::pid_with_raw(100, 0x0C, 680.0, &[0x0A, 0xA0])
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::voltage(200, 14.4)
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::dtc(300, "P0420")
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::enhanced(400, 0x1234, "ecm", "Boost", "kPa", 15.0)
            .write_to(&mut buf)
            .unwrap();
        RecordingFrame::o2(500, "Cat Mon", "B1S1", "V", 0.45)
            .write_to(&mut buf)
            .unwrap();

        let mut cursor = io::Cursor::new(buf);
        let (decoded_header, version) = read_file_header(&mut cursor).unwrap();
        assert_eq!(version, 2);
        assert_eq!(decoded_header.session_id, "test-rule5");

        let mut frames = Vec::new();
        while let Some(frame) = RecordingFrame::read_from(&mut cursor, version).unwrap() {
            frames.push(frame);
        }

        assert_eq!(frames.len(), 5);
        assert_eq!(frames[0].frame_type, FRAME_PID);
        assert_eq!(frames[1].frame_type, FRAME_VOLTAGE);
        assert_eq!(frames[2].frame_type, FRAME_DTC);
        assert_eq!(frames[3].frame_type, FRAME_ENHANCED);
        assert_eq!(frames[4].frame_type, FRAME_O2);

        let (did, module, _, _) = frames[3].decode_enhanced().unwrap();
        assert_eq!(did, 0x1234);
        assert_eq!(module, "ecm");

        let (test_name, sensor, _) = frames[4].decode_o2().unwrap();
        assert_eq!(test_name, "Cat Mon");
        assert_eq!(sensor, "B1S1");
    }
}
