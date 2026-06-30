use chrono::{DateTime, Utc};
use obd2_core::protocol::dtc::DtcStatus;
use obd2_core::vehicle::PhysicalAddress;
use serde::{Deserialize, Serialize};

use crate::profiles::model::{Confidence, ProfileDecodeError, Provenance, RxdSource, SourceFields};
use crate::profiles::runtime::{CapabilityId, DispatchError, DispatchEvidence, ProfileResponse};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfileEvidenceRecord {
    pub timestamp: DateTime<Utc>,
    pub profile_id: String,
    pub capability_id: String,
    pub capability_kind: String,
    pub module: String,
    pub route: RouteEvidence,
    pub service_id: u8,
    pub request_data: Vec<u8>,
    pub parsed_response_bytes: Vec<u8>,
    pub decoder_id: String,
    pub identity_confidence: Option<String>,
    pub manual_confirmation: bool,
    pub probe: bool,
    pub source_fields: Option<SourceFieldsEvidence>,
    pub decoded: Option<ProfileDecodedEvidence>,
    pub error: Option<ProfileEvidenceError>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RouteEvidence {
    J1850 { node: u8, header: Vec<u8> },
    Can11 { request_id: u16, response_id: u16 },
    Can29 { request_id: u32, response_id: u32 },
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceFieldsEvidence {
    pub txd: String,
    pub rxf: Option<String>,
    pub rxd: Option<RxdEvidence>,
    pub raw_mth: Option<String>,
    pub source_ref: Option<String>,
    pub range_caveat: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RxdEvidence {
    pub raw: String,
    pub bit_width: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileDecodedEvidence {
    Signal {
        key: String,
        label: String,
        did: Option<u16>,
        value: f64,
        unit: String,
        raw: Vec<u8>,
        selected_raw: Vec<u8>,
        confidence: String,
    },
    Dtcs {
        records: Vec<ProfileDtcEvidence>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileDtcEvidence {
    pub code: String,
    pub status: String,
    pub status_raw: Option<u8>,
    pub status_flags: Vec<String>,
    pub raw: Vec<u8>,
    pub module: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProfileEvidenceError {
    pub kind: String,
    pub detail: String,
}

impl ProfileEvidenceRecord {
    pub fn from_dispatch(
        evidence: &DispatchEvidence<'_>,
        identity_confidence: Option<&str>,
        manual_confirmation: bool,
    ) -> Self {
        let decoded = match evidence.outcome {
            Ok(ProfileResponse::Signal(signal)) => Some(ProfileDecodedEvidence::Signal {
                key: signal.key.to_string(),
                label: evidence.label.unwrap_or(signal.key).to_string(),
                did: did_from_mode22_request(evidence.service_id, evidence.request_data),
                value: signal.value,
                unit: evidence.unit.unwrap_or(signal.unit).to_string(),
                raw: signal.raw.clone(),
                selected_raw: signal.selected_raw.clone(),
                confidence: confidence_label(signal.confidence).to_string(),
            }),
            Ok(ProfileResponse::Dtcs(records)) => Some(ProfileDecodedEvidence::Dtcs {
                records: records
                    .iter()
                    .map(|dtc| ProfileDtcEvidence {
                        code: dtc.code.clone(),
                        status: dtc_status_label(dtc.status).to_string(),
                        status_raw: dtc.status_raw,
                        status_flags: dtc.status_flags.clone(),
                        raw: dtc.raw.clone(),
                        module: dtc.module.as_ref().map(|module| module.0.clone()),
                        notes: dtc.notes.clone(),
                    })
                    .collect(),
            }),
            Err(_) => None,
        };

        let error = evidence.outcome.err().map(|err| ProfileEvidenceError {
            kind: dispatch_error_kind(err).to_string(),
            detail: dispatch_error_detail(err),
        });

        Self {
            timestamp: Utc::now(),
            profile_id: evidence.profile_id.as_str().to_string(),
            capability_id: capability_key(evidence.capability).to_string(),
            capability_kind: capability_kind(evidence.capability).to_string(),
            module: evidence.route.module.canonical().to_string(),
            route: route_evidence(&evidence.physical_address),
            service_id: evidence.service_id,
            request_data: evidence.request_data.to_vec(),
            parsed_response_bytes: evidence.raw_payload.to_vec(),
            decoder_id: evidence.decoder_id.to_string(),
            identity_confidence: identity_confidence.map(str::to_string),
            manual_confirmation,
            probe: evidence.is_probe,
            source_fields: source_fields_evidence(evidence.source_fields),
            decoded,
            error,
        }
    }
}

pub fn capability_key(capability: CapabilityId) -> &'static str {
    match capability {
        CapabilityId::Signal(key)
        | CapabilityId::DtcService(key)
        | CapabilityId::ActiveTest(key) => key,
    }
}

fn capability_kind(capability: CapabilityId) -> &'static str {
    match capability {
        CapabilityId::Signal(_) => "signal",
        CapabilityId::DtcService(_) => "dtc_service",
        CapabilityId::ActiveTest(_) => "active_test",
    }
}

pub fn confidence_label(confidence: Confidence) -> &'static str {
    match confidence {
        Confidence::Candidate => "candidate",
        Confidence::LiveObserved => "live_observed",
        Confidence::Community => "community",
        Confidence::Verified => "verified",
        Confidence::Rejected => "rejected",
    }
}

pub fn provenance_label(provenance: Provenance) -> &'static str {
    match provenance {
        Provenance::ScanGaugePublished => "scangauge_published",
        Provenance::LiveObserved => "live_observed",
        Provenance::LegacySpec => "legacy_spec",
        Provenance::LocalRejection => "local_rejection",
        Provenance::LocalFixture => "local_fixture",
    }
}

fn dtc_status_label(status: DtcStatus) -> &'static str {
    match status {
        DtcStatus::Stored => "stored",
        DtcStatus::Pending => "pending",
        DtcStatus::Permanent => "permanent",
    }
}

pub fn dtc_status_from_label(label: &str) -> DtcStatus {
    match label {
        "pending" => DtcStatus::Pending,
        "permanent" => DtcStatus::Permanent,
        _ => DtcStatus::Stored,
    }
}

fn did_from_mode22_request(service_id: u8, request_data: &[u8]) -> Option<u16> {
    if service_id == 0x22 && request_data.len() >= 2 {
        Some(u16::from_be_bytes([request_data[0], request_data[1]]))
    } else {
        None
    }
}

fn route_evidence(address: &PhysicalAddress) -> RouteEvidence {
    match address {
        PhysicalAddress::J1850 { node, header } => RouteEvidence::J1850 {
            node: *node,
            header: header.to_vec(),
        },
        PhysicalAddress::Can11Bit {
            request_id,
            response_id,
        } => RouteEvidence::Can11 {
            request_id: *request_id,
            response_id: *response_id,
        },
        PhysicalAddress::Can29Bit {
            request_id,
            response_id,
        } => RouteEvidence::Can29 {
            request_id: *request_id,
            response_id: *response_id,
        },
        _ => RouteEvidence::Unknown,
    }
}

pub fn source_fields_evidence(fields: SourceFields) -> Option<SourceFieldsEvidence> {
    if fields == SourceFields::NONE {
        return None;
    }

    Some(SourceFieldsEvidence {
        txd: fields.txd.to_string(),
        rxf: fields.rxf.map(str::to_string),
        rxd: fields.rxd.map(rxd_evidence),
        raw_mth: fields.raw_mth.map(str::to_string),
        source_ref: fields.source_ref.map(str::to_string),
        range_caveat: fields.rxd.and_then(range_caveat),
    })
}

fn rxd_evidence(rxd: RxdSource) -> RxdEvidence {
    RxdEvidence {
        raw: rxd.raw.to_string(),
        bit_width: rxd.bit_width,
    }
}

fn range_caveat(rxd: RxdSource) -> Option<String> {
    (rxd.raw.eq_ignore_ascii_case("3008") && rxd.bit_width == Some(8)).then(|| {
        "RXD=3008 selects an 8-bit value; validate effective range against live bytes before treating this as full-range pressure."
            .to_string()
    })
}

fn dispatch_error_kind(error: &DispatchError) -> &'static str {
    match error {
        DispatchError::Transport(_) => "transport",
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { .. }) => "negative_response",
        DispatchError::Decode(ProfileDecodeError::PayloadTooShort { .. }) => "payload_too_short",
        DispatchError::Decode(_) => "decode",
        DispatchError::ActiveTestLocked { .. } => "active_test_locked",
        DispatchError::StaleGeneration { .. } => "stale_generation",
        DispatchError::CapabilityNotOwned { .. } => "capability_not_owned",
        DispatchError::RouteNotOwnedByCapability { .. } => "route_not_owned",
        DispatchError::ProtocolFamilyMismatch { .. } => "protocol_family_mismatch",
        DispatchError::UnknownProfile(_) => "unknown_profile",
        DispatchError::MissingModuleMap { .. } => "missing_module_map",
        DispatchError::UnknownModule { .. } => "unknown_module",
        DispatchError::UnknownBus { .. } => "unknown_bus",
        DispatchError::AddressCandidate { .. } => "address_candidate",
        DispatchError::AddressUnresolved { .. } => "address_unresolved",
        DispatchError::AddressBusMismatch { .. } => "address_bus_mismatch",
    }
}

fn dispatch_error_detail(error: &DispatchError) -> String {
    match error {
        DispatchError::Decode(ProfileDecodeError::NegativeResponse { service, nrc }) => {
            format!("negative response service 0x{service:02X}, NRC 0x{nrc:02X}")
        }
        DispatchError::Decode(ProfileDecodeError::PayloadTooShort { expected, got }) => {
            format!("payload too short: expected {expected}, got {got}")
        }
        DispatchError::Decode(ProfileDecodeError::UnknownDecoder(decoder)) => {
            format!("unknown decoder: {decoder}")
        }
        DispatchError::Decode(ProfileDecodeError::Decode(message))
        | DispatchError::Decode(ProfileDecodeError::Other(message)) => message.clone(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profiles::model::RxdSource;

    #[test]
    fn source_fields_rxd_caveat_preserves_raw_field() {
        let fields = SourceFields {
            txd: "6C10F122163D01",
            rxf: Some("046205190640"),
            rxd: Some(RxdSource {
                raw: "3008",
                bit_width: Some(8),
            }),
            raw_mth: Some("0091000A0000"),
            source_ref: Some("ScanGauge LB7/LLY"),
        };

        let projected = source_fields_evidence(fields).expect("source fields");

        assert_eq!(projected.rxd.as_ref().unwrap().raw, "3008");
        assert!(projected
            .range_caveat
            .as_ref()
            .expect("range caveat")
            .contains("RXD=3008"));
    }
}
