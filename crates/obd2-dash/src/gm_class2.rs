use std::fmt;

use obd2_core::adapter::{PhysicalTarget, RoutedRequest};
use obd2_core::protocol::dtc::{Dtc, DtcStatus};
use obd2_core::vehicle::PhysicalAddress;

pub const SERVICE_REPORT_DTCS_BY_STATUS: u8 = 0x19;
pub const POSITIVE_REPORT_DTCS_BY_STATUS: u8 = 0x59;
pub const STATUS_MASK_ALL: u8 = 0xFF;
pub const STATUS_MASK_TECH2_ACTIVE: u8 = 0x92;
pub const DTC_GROUP_ALL: [u8; 2] = [0xFF, 0x00];

pub const CLASS2_DTC_ALL_REQUEST: [u8; 3] = [STATUS_MASK_ALL, DTC_GROUP_ALL[0], DTC_GROUP_ALL[1]];
pub const CLASS2_DTC_ACTIVE_REQUEST: [u8; 3] =
    [STATUS_MASK_TECH2_ACTIVE, DTC_GROUP_ALL[0], DTC_GROUP_ALL[1]];
pub const CLASS2_TOOL_SOURCE: u8 = 0xF1;
pub const CLASS2_DIAGNOSTIC_PRIORITY: u8 = 0x6C;
pub const MODE_22_READ_DATA_BY_IDENTIFIER: u8 = 0x22;
pub const ECM_NODE: u8 = 0x10;
pub const PSI_PER_KPA: f64 = 0.145_037_737_7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmClass2Node {
    pub node: u8,
    pub label: &'static str,
}

pub const DEFAULT_CLASS2_NODES: &[GmClass2Node] = &[
    GmClass2Node {
        node: 0x10,
        label: "ECM/PCM",
    },
    GmClass2Node {
        node: 0x11,
        label: "ECM (engine node 2)",
    },
    GmClass2Node {
        node: 0x18,
        label: "TCM",
    },
    GmClass2Node {
        node: 0x1A,
        label: "TCCM (transfer case)",
    },
    GmClass2Node {
        node: 0x20,
        label: "IPC (cluster)",
    },
    GmClass2Node {
        node: 0x29,
        label: "EBCM/ABS",
    },
    GmClass2Node {
        node: 0x40,
        label: "BCM",
    },
    GmClass2Node {
        node: 0x58,
        label: "SDM (airbag)",
    },
    GmClass2Node {
        node: 0x60,
        label: "HVAC / unknown",
    },
    GmClass2Node {
        node: 0x80,
        label: "Radio/IRC",
    },
    GmClass2Node {
        node: 0xA0,
        label: "DDM (driver door)",
    },
];

#[cfg(feature = "probe")]
pub fn class2_header(node: u8) -> [u8; 3] {
    [CLASS2_DIAGNOSTIC_PRIORITY, node, CLASS2_TOOL_SOURCE]
}

#[cfg(not(feature = "probe"))]
#[allow(dead_code)]
pub(crate) fn class2_header(node: u8) -> [u8; 3] {
    [CLASS2_DIAGNOSTIC_PRIORITY, node, CLASS2_TOOL_SOURCE]
}

#[cfg(feature = "probe")]
pub fn class2_routed_request(node: u8, service_id: u8, data: impl Into<Vec<u8>>) -> RoutedRequest {
    class2_routed_request_inner(node, service_id, data)
}

#[cfg(not(feature = "probe"))]
#[allow(dead_code)]
pub(crate) fn class2_routed_request(
    node: u8,
    service_id: u8,
    data: impl Into<Vec<u8>>,
) -> RoutedRequest {
    class2_routed_request_inner(node, service_id, data)
}

#[allow(dead_code)]
fn class2_routed_request_inner(
    node: u8,
    service_id: u8,
    data: impl Into<Vec<u8>>,
) -> RoutedRequest {
    RoutedRequest {
        service_id,
        data: data.into(),
        target: PhysicalTarget::Addressed(PhysicalAddress::J1850 {
            node,
            header: class2_header(node),
        }),
    }
}

#[cfg(feature = "probe")]
pub fn class2_dtc_all_request(node: u8) -> RoutedRequest {
    class2_dtc_all_request_inner(node)
}

#[cfg(not(feature = "probe"))]
#[allow(dead_code)]
pub(crate) fn class2_dtc_all_request(node: u8) -> RoutedRequest {
    class2_dtc_all_request_inner(node)
}

#[allow(dead_code)]
fn class2_dtc_all_request_inner(node: u8) -> RoutedRequest {
    class2_routed_request(
        node,
        SERVICE_REPORT_DTCS_BY_STATUS,
        CLASS2_DTC_ALL_REQUEST.to_vec(),
    )
}

#[cfg(feature = "probe")]
pub fn class2_dtc_active_request(node: u8) -> RoutedRequest {
    class2_dtc_active_request_inner(node)
}

#[cfg(not(feature = "probe"))]
#[allow(dead_code)]
pub(crate) fn class2_dtc_active_request(node: u8) -> RoutedRequest {
    class2_dtc_active_request_inner(node)
}

#[allow(dead_code)]
fn class2_dtc_active_request_inner(node: u8) -> RoutedRequest {
    class2_routed_request(
        node,
        SERVICE_REPORT_DTCS_BY_STATUS,
        CLASS2_DTC_ACTIVE_REQUEST.to_vec(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmClass2Status {
    pub raw: u8,
    pub warning_indicator_requested: bool,
    pub pending: bool,
    pub old: bool,
    pub history: bool,
    pub current: bool,
    pub immature: bool,
    pub reserved: u8,
}

impl GmClass2Status {
    pub fn from_byte(raw: u8) -> Self {
        Self {
            raw,
            warning_indicator_requested: (raw & 0x80) != 0,
            pending: (raw & 0x40) != 0,
            old: (raw & 0x20) != 0,
            history: (raw & 0x10) != 0,
            current: (raw & 0x02) != 0,
            immature: (raw & 0x01) != 0,
            reserved: raw & 0x0C,
        }
    }

    pub fn labels(self) -> Vec<&'static str> {
        let mut labels = Vec::with_capacity(7);
        if self.warning_indicator_requested {
            labels.push("mil");
        }
        if self.pending {
            labels.push("pending");
        }
        if self.old {
            labels.push("old");
        }
        if self.history {
            labels.push("history");
        }
        if self.current {
            labels.push("current");
        }
        if self.immature {
            labels.push("immature");
        }
        if self.reserved != 0 {
            labels.push("reserved");
        }
        labels
    }

    pub fn display_flags(self) -> String {
        let labels = self.labels();
        if labels.is_empty() {
            "none".to_string()
        } else {
            labels.join("|")
        }
    }

    pub fn generic_status(self) -> DtcStatus {
        // This is only a coarse UI bucket. The original GM status byte must be
        // retained separately; it is not the UDS status-byte layout.
        if self.pending {
            DtcStatus::Pending
        } else {
            DtcStatus::Stored
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GmClass2DtcRecord {
    pub dtc: Dtc,
    pub status: GmClass2Status,
}

impl GmClass2DtcRecord {
    pub fn into_dtc(mut self, module: Option<&str>) -> Dtc {
        self.dtc.status = self.status.generic_status();
        if let Some(module) = module {
            self.dtc.source_module = Some(module.to_string());
        }
        self.dtc.notes = Some(format!(
            "GM Class 2 status 0x{:02X}: {}",
            self.status.raw,
            self.status.display_flags()
        ));
        self.dtc
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmClass2DecodeError {
    UnexpectedPayloadLength { len: usize, payload: Vec<u8> },
}

impl fmt::Display for GmClass2DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedPayloadLength { len, payload } => {
                write!(
                    f,
                    "unsupported GM Class 2 DTC payload length {len}: {}",
                    hex_bytes(payload)
                )
            }
        }
    }
}

impl std::error::Error for GmClass2DecodeError {}

pub fn decode_class2_dtcs(payload: &[u8]) -> Result<Vec<GmClass2DtcRecord>, GmClass2DecodeError> {
    let payload = positive_payload(payload);
    if payload.is_empty() || payload.iter().all(|byte| *byte == 0) {
        return Ok(Vec::new());
    }

    if !payload.len().is_multiple_of(3) {
        return Err(GmClass2DecodeError::UnexpectedPayloadLength {
            len: payload.len(),
            payload: payload.to_vec(),
        });
    }

    let mut records = Vec::with_capacity(payload.len() / 3);
    for chunk in payload.chunks_exact(3) {
        if chunk == [0x00, 0x00, 0x00] {
            continue;
        }
        records.push(GmClass2DtcRecord {
            dtc: Dtc::from_bytes(chunk[0], chunk[1]),
            status: GmClass2Status::from_byte(chunk[2]),
        });
    }
    Ok(records)
}

fn positive_payload(payload: &[u8]) -> &[u8] {
    if payload.first().copied() == Some(POSITIVE_REPORT_DTCS_BY_STATUS) {
        &payload[1..]
    } else {
        payload
    }
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        write!(&mut out, "{byte:02X}").expect("write to string");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_class2_payload_with_positive_service_byte() {
        let records = decode_class2_dtcs(&[0x59, 0x43, 0x79, 0x93]).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].dtc.code, "C0379");
        assert!(records[0].status.warning_indicator_requested);
        assert!(records[0].status.history);
        assert!(records[0].status.current);
        assert!(records[0].status.immature);
        assert_eq!(
            records[0].status.display_flags(),
            "mil|history|current|immature"
        );
    }

    #[test]
    fn decodes_payload_after_adapter_service_strip() {
        let records = decode_class2_dtcs(&[0x43, 0x79, 0x93]).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].dtc.code, "C0379");
    }

    #[test]
    fn decodes_multiple_triplet_records() {
        let records = decode_class2_dtcs(&[0x43, 0x79, 0x93, 0xD0, 0x24, 0x12]).unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].dtc.code, "C0379");
        assert_eq!(records[1].dtc.code, "U1024");
        assert!(records[1].status.history);
        assert!(records[1].status.current);
    }

    #[test]
    fn skips_zero_records() {
        let records = decode_class2_dtcs(&[0x00, 0x00, 0x00, 0x43, 0x79, 0x93]).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].dtc.code, "C0379");
    }

    #[test]
    fn empty_and_zero_payloads_are_empty() {
        assert!(decode_class2_dtcs(&[]).unwrap().is_empty());
        assert!(decode_class2_dtcs(&[0x00, 0x00, 0x00]).unwrap().is_empty());
    }

    #[test]
    fn rejects_unproven_payload_shape() {
        let err = decode_class2_dtcs(&[0x43, 0x79]).unwrap_err();
        assert!(matches!(
            err,
            GmClass2DecodeError::UnexpectedPayloadLength { len: 2, .. }
        ));
    }

    #[test]
    fn into_dtc_preserves_gm_status_in_notes() {
        let dtc = decode_class2_dtcs(&[0x43, 0x79, 0x93])
            .unwrap()
            .remove(0)
            .into_dtc(Some("abs"));
        assert_eq!(dtc.code, "C0379");
        assert_eq!(dtc.source_module.as_deref(), Some("abs"));
        assert_eq!(
            dtc.notes.as_deref(),
            Some("GM Class 2 status 0x93: mil|history|current|immature")
        );
    }

    #[test]
    fn builds_class2_routed_request_header() {
        let request = class2_routed_request(0x18, 0x22, vec![0x19, 0x40, 0x01]);
        assert_eq!(request.service_id, 0x22);
        assert_eq!(request.data, vec![0x19, 0x40, 0x01]);

        match request.target {
            PhysicalTarget::Addressed(PhysicalAddress::J1850 { node, header }) => {
                assert_eq!(node, 0x18);
                assert_eq!(header, [0x6C, 0x18, 0xF1]);
            }
            _ => panic!("expected addressed J1850 request"),
        }
    }
}
