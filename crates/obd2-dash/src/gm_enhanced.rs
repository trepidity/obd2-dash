use std::fmt;

use obd2_core::adapter::RoutedRequest;
use obd2_core::vehicle::{Protocol, VehicleSpec};

use crate::gm_class2::{class2_header, class2_routed_request, GmClass2Node};

pub const MODE_22_READ_DATA_BY_ID: u8 = 0x22;
pub const POSITIVE_MODE_22_READ_DATA_BY_ID: u8 = 0x62;
pub const ECM_NODE: u8 = 0x10;
pub const TCM_NODE: u8 = 0x18;
pub const SELECTOR_01: &[u8] = &[0x01];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmProtocol {
    J1850VpwClass2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    Candidate,
    LiveObserved,
    Community,
    Verified,
    Rejected,
}

impl Confidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::LiveObserved => "live-observed",
            Self::Community => "community",
            Self::Verified => "verified",
            Self::Rejected => "rejected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    ScanGaugePublished,
    LiveObserved,
    LegacySpec,
    LocalRejection,
}

impl Provenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ScanGaugePublished => "scangauge-published",
            Self::LiveObserved => "live-observed",
            Self::LegacySpec => "legacy-spec",
            Self::LocalRejection => "local-rejection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollCadence {
    Fast,
    Medium,
    Slow,
    OnDemand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailurePolicy {
    SurfaceUnavailable,
    PreferStandardPid,
    CandidateOnly,
    DoNotPoll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectedDisposition {
    Rejected,
    UnverifiedLegacy,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VehicleMatcher {
    pub platform: &'static str,
    pub model_years: &'static [u16],
    pub engine: &'static str,
    pub vin_8: Option<char>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmRxd {
    pub raw: &'static str,
    pub byte_index: usize,
    pub bit_width: u8,
}

impl GmRxd {
    pub const fn new(raw: &'static str, byte_index: usize, bit_width: u8) -> Self {
        Self {
            raw,
            byte_index,
            bit_width,
        }
    }

    pub fn from_scangauge(raw: &'static str) -> Result<Self, GmEnhancedDecodeError> {
        if raw.len() != 4 {
            return Err(GmEnhancedDecodeError::InvalidRxd { raw });
        }
        let selector = parse_hex_byte(&raw[0..2])?;
        let bit_width = parse_hex_byte(&raw[2..4])?;
        let Some(byte_index) = selector.checked_sub(0x30) else {
            return Err(GmEnhancedDecodeError::InvalidRxd { raw });
        };
        if bit_width != 8 && bit_width != 16 {
            return Err(GmEnhancedDecodeError::UnsupportedRxdWidth { bit_width });
        }
        Ok(Self {
            raw,
            byte_index: byte_index as usize,
            bit_width,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmMth {
    pub raw: &'static str,
    pub multiplier: i16,
    pub divisor: i16,
    pub offset: i16,
}

impl GmMth {
    pub const fn new(raw: &'static str, multiplier: i16, divisor: i16, offset: i16) -> Self {
        Self {
            raw,
            multiplier,
            divisor,
            offset,
        }
    }

    pub fn from_scangauge(raw: &'static str) -> Result<Self, GmEnhancedDecodeError> {
        if raw.len() != 12 {
            return Err(GmEnhancedDecodeError::InvalidMth { raw });
        }
        let mut bytes = [0u8; 6];
        for (index, byte) in bytes.iter_mut().enumerate() {
            let start = index * 2;
            *byte = parse_hex_byte(&raw[start..start + 2])?;
        }
        Ok(Self {
            raw,
            multiplier: i16::from_be_bytes([bytes[0], bytes[1]]),
            divisor: i16::from_be_bytes([bytes[2], bytes[3]]),
            offset: i16::from_be_bytes([bytes[4], bytes[5]]),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmDecodeKind {
    RxdMth { rxd: GmRxd, mth: GmMth },
    RxdUnsigned { rxd: GmRxd },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmDidDefinition {
    pub did: u16,
    pub selector: &'static [u8],
    pub module: GmClass2Node,
    pub name: &'static str,
    pub unit: &'static str,
    pub service: u8,
    pub txd: &'static str,
    pub rxf: Option<&'static str>,
    pub rxd: Option<GmRxd>,
    pub raw_mth: Option<&'static str>,
    pub decode: GmDecodeKind,
    pub confidence: Confidence,
    pub provenance: &'static [Provenance],
    pub evidence_id: Option<&'static str>,
    pub cadence: PollCadence,
    pub failure_policy: FailurePolicy,
    pub range_suspect: bool,
    pub notes: &'static str,
}

impl GmDidDefinition {
    pub fn request_data(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(2 + self.selector.len());
        data.push((self.did >> 8) as u8);
        data.push(self.did as u8);
        data.extend_from_slice(self.selector);
        data
    }

    #[cfg(feature = "probe")]
    pub fn request_header(&self) -> [u8; 3] {
        class2_header(self.module.node)
    }

    #[cfg(not(feature = "probe"))]
    #[allow(dead_code)]
    pub(crate) fn request_header(&self) -> [u8; 3] {
        class2_header(self.module.node)
    }

    #[cfg(feature = "probe")]
    pub fn routed_request(&self) -> RoutedRequest {
        class2_routed_request(self.module.node, self.service, self.request_data())
    }

    #[cfg(not(feature = "probe"))]
    #[allow(dead_code)]
    pub(crate) fn routed_request(&self) -> RoutedRequest {
        class2_routed_request(self.module.node, self.service, self.request_data())
    }

    pub fn decode_value(
        &self,
        payload: &[u8],
    ) -> Result<GmDecodedValue<'_>, GmEnhancedDecodeError> {
        decode_did_value(self, payload)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmRejectedDid {
    pub did: u16,
    pub selector: &'static [u8],
    pub module: GmClass2Node,
    pub signal: &'static str,
    pub candidate_request: &'static str,
    pub disposition: RejectedDisposition,
    pub confidence: Confidence,
    pub provenance: &'static [Provenance],
    pub reason: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmDtcServices {
    pub all_status_request: &'static [u8],
    pub active_history_request: &'static [u8],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmProfile {
    pub id: &'static str,
    pub applies_to: VehicleMatcher,
    pub protocol: GmProtocol,
    pub nodes: &'static [GmClass2Node],
    pub dids: &'static [GmDidDefinition],
    pub rejected_dids: &'static [GmRejectedDid],
    pub dtc_services: GmDtcServices,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GmDecodedValue<'a> {
    pub definition: &'a GmDidDefinition,
    pub selected_raw: u32,
    pub value: f64,
    pub unit: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmEnhancedDecodeError {
    InvalidHexByte {
        text: String,
    },
    InvalidRxd {
        raw: &'static str,
    },
    InvalidMth {
        raw: &'static str,
    },
    UnsupportedRxdWidth {
        bit_width: u8,
    },
    PayloadTooShort {
        did: u16,
        rxd: &'static str,
        needed: usize,
        actual: usize,
    },
    MthZeroDivisor {
        raw: &'static str,
    },
    MismatchedPositiveResponse {
        expected_did: u16,
        response_did: u16,
    },
}

impl fmt::Display for GmEnhancedDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHexByte { text } => write!(f, "invalid hex byte {text:?}"),
            Self::InvalidRxd { raw } => write!(f, "invalid ScanGauge RXD {raw}"),
            Self::InvalidMth { raw } => write!(f, "invalid ScanGauge MTH {raw}"),
            Self::UnsupportedRxdWidth { bit_width } => {
                write!(f, "unsupported RXD width {bit_width} bits")
            }
            Self::PayloadTooShort {
                did,
                rxd,
                needed,
                actual,
            } => write!(
                f,
                "payload for DID 0x{did:04X} is too short for RXD {rxd}: need {needed}, got {actual}"
            ),
            Self::MthZeroDivisor { raw } => write!(f, "ScanGauge MTH {raw} has zero divisor"),
            Self::MismatchedPositiveResponse {
                expected_did,
                response_did,
            } => write!(
                f,
                "positive response DID 0x{response_did:04X} does not match expected DID 0x{expected_did:04X}"
            ),
        }
    }
}

impl std::error::Error for GmEnhancedDecodeError {}

const ECM: GmClass2Node = GmClass2Node {
    node: ECM_NODE,
    label: "ECM/PCM",
};
const TCM: GmClass2Node = GmClass2Node {
    node: TCM_NODE,
    label: "TCM",
};
const RXD_3008: GmRxd = GmRxd::new("3008", 0, 8);
const RXD_3010: GmRxd = GmRxd::new("3010", 0, 16);
const MTH_TEMP_F: GmMth = GmMth::new("00090005FFD8", 9, 5, -40);
const MTH_OIL_PSI: GmMth = GmMth::new("001D00320000", 29, 50, 0);
const MTH_RAIL_KPSI: GmMth = GmMth::new("0091000A0000", 145, 10, 0);
const MTH_PERCENT: GmMth = GmMth::new("006400FF0000", 100, 255, 0);
const MTH_PULSE_MS: GmMth = GmMth::new("00C800830000", 200, 131, 0);
const MTH_BALANCE_MM3: GmMth = GmMth::new("00050020EC00", 5, 32, -5120);
const PROV_SCANGAUGE: &[Provenance] = &[Provenance::ScanGaugePublished];
const PROV_SCANGAUGE_LIVE: &[Provenance] =
    &[Provenance::ScanGaugePublished, Provenance::LiveObserved];
const PROV_LIVE: &[Provenance] = &[Provenance::LiveObserved];
const PROV_LEGACY_REJECTED: &[Provenance] = &[Provenance::LegacySpec, Provenance::LocalRejection];
const LLY_YEARS: &[u16] = &[2004, 2005];

pub const LLY_ENHANCED_DIDS: &[GmDidDefinition] = &[
    GmDidDefinition {
        did: 0x1940,
        selector: SELECTOR_01,
        module: TCM,
        name: "transmission temperature",
        unit: "deg F",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C18F122194001",
        rxf: Some("046205190640"),
        rxd: Some(RXD_3008),
        raw_mth: Some("00090005FFD8"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3008,
            mth: MTH_TEMP_F,
        },
        confidence: Confidence::Community,
        provenance: PROV_SCANGAUGE,
        evidence_id: None,
        cadence: PollCadence::Medium,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "ScanGauge-published Allison TCM temperature at node 0x18.",
    },
    GmDidDefinition {
        did: 0x1470,
        selector: SELECTOR_01,
        module: ECM,
        name: "oil pressure",
        unit: "psi",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122147001",
        rxf: Some("046205140670"),
        rxd: Some(RXD_3008),
        raw_mth: Some("001D00320000"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3008,
            mth: MTH_OIL_PSI,
        },
        confidence: Confidence::Community,
        provenance: PROV_SCANGAUGE,
        evidence_id: None,
        cadence: PollCadence::Medium,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "ScanGauge-published 2003+ GM oil pressure entry.",
    },
    GmDidDefinition {
        did: 0x163D,
        selector: SELECTOR_01,
        module: ECM,
        name: "desired fuel rail pressure",
        unit: "psi",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122163D01",
        rxf: Some("04624516063D"),
        rxd: Some(RXD_3008),
        raw_mth: Some("0091000A0000"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3008,
            mth: MTH_RAIL_KPSI,
        },
        confidence: Confidence::LiveObserved,
        provenance: PROV_SCANGAUGE_LIVE,
        evidence_id: None,
        cadence: PollCadence::Fast,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: true,
        notes: "Range-suspect 8-bit ScanGauge scaling; vendor label is 1000's of PSI. Keep local bytes for cross-check.",
    },
    GmDidDefinition {
        did: 0x163E,
        selector: SELECTOR_01,
        module: ECM,
        name: "actual fuel rail pressure",
        unit: "psi",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122163E01",
        rxf: Some("04624516063E"),
        rxd: Some(RXD_3008),
        raw_mth: Some("0091000A0000"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3008,
            mth: MTH_RAIL_KPSI,
        },
        confidence: Confidence::LiveObserved,
        provenance: PROV_SCANGAUGE_LIVE,
        evidence_id: None,
        cadence: PollCadence::Fast,
        failure_policy: FailurePolicy::PreferStandardPid,
        range_suspect: true,
        notes: "Prefer standard PID 01 23 for actual rail when available. Vendor label is 1000's of PSI; keep local bytes for range validation.",
    },
    GmDidDefinition {
        did: 0x1540,
        selector: SELECTOR_01,
        module: ECM,
        name: "VGT vane desired",
        unit: "%",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122154001",
        rxf: Some("046205150640"),
        rxd: Some(RXD_3008),
        raw_mth: Some("006400FF0000"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3008,
            mth: MTH_PERCENT,
        },
        confidence: Confidence::LiveObserved,
        provenance: PROV_SCANGAUGE_LIVE,
        evidence_id: None,
        cadence: PollCadence::Fast,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "Compute VGT error as actual minus desired.",
    },
    GmDidDefinition {
        did: 0x1543,
        selector: SELECTOR_01,
        module: ECM,
        name: "VGT vane actual",
        unit: "%",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122154301",
        rxf: Some("046205160643"),
        rxd: Some(RXD_3008),
        raw_mth: Some("006400FF0000"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3008,
            mth: MTH_PERCENT,
        },
        confidence: Confidence::LiveObserved,
        provenance: PROV_SCANGAUGE_LIVE,
        evidence_id: None,
        cadence: PollCadence::Fast,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "High value at idle/closed; lower value means more open.",
    },
    injector_pulse_width(1, 0x1193, "6C10F122119301", "046245110693"),
    injector_pulse_width(2, 0x1194, "6C10F122119401", "046245110694"),
    injector_pulse_width(3, 0x1195, "6C10F122119501", "046245110695"),
    injector_pulse_width(4, 0x1196, "6C10F122119601", "046245110696"),
    injector_pulse_width(5, 0x1197, "6C10F122119701", "046245110697"),
    injector_pulse_width(6, 0x1198, "6C10F122119801", "046245110698"),
    injector_pulse_width(7, 0x1199, "6C10F122119901", "046245110699"),
    injector_pulse_width(8, 0x119A, "6C10F122119A01", "04624511069A"),
    injector_balance(1, 0x162F, "6C10F122162F01", "04628516062F"),
    injector_balance(2, 0x1630, "6C10F122163001", "046285160630"),
    injector_balance(3, 0x1631, "6C10F122163101", "046285160631"),
    injector_balance(4, 0x1632, "6C10F122163201", "046285160632"),
    injector_balance(5, 0x1633, "6C10F122163301", "046285160633"),
    injector_balance(6, 0x1634, "6C10F122163401", "046285160634"),
    injector_balance(7, 0x1635, "6C10F122163501", "046285160635"),
    injector_balance(8, 0x1636, "6C10F122163601", "046285160636"),
    GmDidDefinition {
        did: 0x1251,
        selector: SELECTOR_01,
        module: ECM,
        name: "barometric pressure",
        unit: "kPa abs",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122125101",
        rxf: None,
        rxd: Some(RXD_3008),
        raw_mth: None,
        decode: GmDecodeKind::RxdUnsigned { rxd: RXD_3008 },
        confidence: Confidence::LiveObserved,
        provenance: PROV_LIVE,
        evidence_id: None,
        cadence: PollCadence::Medium,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "Live-observed LLY path; evidence capture pending. Compare against 0x119D.",
    },
    GmDidDefinition {
        did: 0x1542,
        selector: SELECTOR_01,
        module: ECM,
        name: "desired MAP",
        unit: "kPa abs",
        service: MODE_22_READ_DATA_BY_ID,
        txd: "6C10F122154201",
        rxf: None,
        rxd: Some(RXD_3008),
        raw_mth: None,
        decode: GmDecodeKind::RxdUnsigned { rxd: RXD_3008 },
        confidence: Confidence::Candidate,
        provenance: PROV_LIVE,
        evidence_id: None,
        cadence: PollCadence::Fast,
        failure_policy: FailurePolicy::CandidateOnly,
        range_suspect: false,
        notes:
            "Candidate desired MAP/boost target; do not use for alerts before persisted validation.",
    },
];

pub const LLY_REJECTED_DIDS: &[GmRejectedDid] = &[
    GmRejectedDid {
        did: 0x1170,
        selector: &[],
        module: ECM,
        signal: "fuel rail actual",
        candidate_request: "221170",
        disposition: RejectedDisposition::Rejected,
        confidence: Confidence::Rejected,
        provenance: PROV_LEGACY_REJECTED,
        reason: "Older spec entry; current LLY path uses standard PID 01 23 and published/live-observed 22 16 3E 01.",
    },
    GmRejectedDid {
        did: 0x1171,
        selector: &[],
        module: ECM,
        signal: "fuel rail desired",
        candidate_request: "221171",
        disposition: RejectedDisposition::Rejected,
        confidence: Confidence::Rejected,
        provenance: PROV_LEGACY_REJECTED,
        reason: "Older spec entry; current LLY path uses published/live-observed 22 16 3D 01.",
    },
    GmRejectedDid {
        did: 0x1172,
        selector: &[],
        module: ECM,
        signal: "fuel rail error",
        candidate_request: "221172",
        disposition: RejectedDisposition::Rejected,
        confidence: Confidence::Rejected,
        provenance: PROV_LEGACY_REJECTED,
        reason: "Compute actual minus desired until a verified LLY error DID exists.",
    },
    GmRejectedDid {
        did: 0x1117,
        selector: &[],
        module: ECM,
        signal: "desired boost/MAP",
        candidate_request: "221117",
        disposition: RejectedDisposition::UnverifiedLegacy,
        confidence: Confidence::Candidate,
        provenance: PROV_LEGACY_REJECTED,
        reason: "Legacy unverified entry; do not wire without a persisted positive response proving meaning and scale.",
    },
    GmRejectedDid {
        did: 0x119D,
        selector: SELECTOR_01,
        module: ECM,
        signal: "barometric pressure",
        candidate_request: "22119D01",
        disposition: RejectedDisposition::Unresolved,
        confidence: Confidence::Candidate,
        provenance: PROV_LEGACY_REJECTED,
        reason: "Community label may point to V8; current live-observed LLY path used 0x1251. Capture both before deciding.",
    },
];

pub const LLY_DTC_SERVICES: GmDtcServices = GmDtcServices {
    all_status_request: &crate::gm_class2::CLASS2_DTC_ALL_REQUEST,
    active_history_request: &crate::gm_class2::CLASS2_DTC_ACTIVE_REQUEST,
};

pub const LLY_DURAMAX_PROFILE: GmProfile = GmProfile {
    id: "gmt800-lly-duramax-class2",
    applies_to: VehicleMatcher {
        platform: "GMT800",
        model_years: LLY_YEARS,
        engine: "6.6L Duramax LLY",
        vin_8: Some('2'),
    },
    protocol: GmProtocol::J1850VpwClass2,
    nodes: crate::gm_class2::DEFAULT_CLASS2_NODES,
    dids: LLY_ENHANCED_DIDS,
    rejected_dids: LLY_REJECTED_DIDS,
    dtc_services: LLY_DTC_SERVICES,
};

pub fn find_lly_did(did: u16) -> Option<&'static GmDidDefinition> {
    LLY_ENHANCED_DIDS.iter().find(|entry| entry.did == did)
}

pub fn lly_profile_matches(vin: &str, spec: Option<&VehicleSpec>, protocol: Protocol) -> bool {
    if protocol != Protocol::J1850Vpw {
        return false;
    }

    let Some(spec) = spec else {
        return false;
    };

    let vin = vin.trim();
    if vin.len() != 17 {
        return false;
    }
    let Some(vin_8) = vin.chars().nth(7).map(|value| value.to_ascii_uppercase()) else {
        return false;
    };
    if Some(vin_8) != LLY_DURAMAX_PROFILE.applies_to.vin_8 {
        return false;
    }

    if !is_lly_spec_identity(spec) {
        return false;
    }

    spec.identity
        .vin_match
        .as_ref()
        .is_some_and(|matcher| matcher.matches(vin))
}

pub fn is_lly_spec_identity(spec: &VehicleSpec) -> bool {
    if spec.identity.model_years != (2004, 2005) {
        return false;
    }
    if !spec.identity.engine.code.eq_ignore_ascii_case("LLY") {
        return false;
    }
    if spec.identity.engine.cylinders != 8 {
        return false;
    }
    if (spec.identity.engine.displacement_l - 6.6).abs() > 0.05 {
        return false;
    }
    spec.identity
        .engine
        .fuel_type
        .eq_ignore_ascii_case("diesel")
}

pub fn decode_did_value<'a>(
    definition: &'a GmDidDefinition,
    payload: &[u8],
) -> Result<GmDecodedValue<'a>, GmEnhancedDecodeError> {
    let data = selected_mode22_data(definition, payload)?;
    let (rxd, value) = match definition.decode {
        GmDecodeKind::RxdMth { rxd, mth } => {
            let raw = select_rxd_raw(definition.did, data, rxd)?;
            let value = apply_mth(raw, mth)?;
            (raw, value)
        }
        GmDecodeKind::RxdUnsigned { rxd } => {
            let raw = select_rxd_raw(definition.did, data, rxd)?;
            (raw, raw as f64)
        }
    };
    Ok(GmDecodedValue {
        definition,
        selected_raw: rxd,
        value,
        unit: definition.unit,
    })
}

pub fn selected_mode22_data<'a>(
    definition: &GmDidDefinition,
    payload: &'a [u8],
) -> Result<&'a [u8], GmEnhancedDecodeError> {
    let mut data = payload;
    if data.len() >= 3 && data[0] == POSITIVE_MODE_22_READ_DATA_BY_ID {
        let response_did = u16::from_be_bytes([data[1], data[2]]);
        if response_did != definition.did {
            return Err(GmEnhancedDecodeError::MismatchedPositiveResponse {
                expected_did: definition.did,
                response_did,
            });
        }
        data = &data[3..];
    }
    if !definition.selector.is_empty() && data.starts_with(definition.selector) {
        data = &data[definition.selector.len()..];
    }
    Ok(data)
}

pub fn select_rxd_raw(did: u16, payload: &[u8], rxd: GmRxd) -> Result<u32, GmEnhancedDecodeError> {
    let width_bytes = usize::from(rxd.bit_width) / 8;
    let needed = rxd.byte_index + width_bytes;
    if payload.len() < needed {
        return Err(GmEnhancedDecodeError::PayloadTooShort {
            did,
            rxd: rxd.raw,
            needed,
            actual: payload.len(),
        });
    }

    match rxd.bit_width {
        8 => Ok(u32::from(payload[rxd.byte_index])),
        16 => Ok(u32::from(u16::from_be_bytes([
            payload[rxd.byte_index],
            payload[rxd.byte_index + 1],
        ]))),
        bit_width => Err(GmEnhancedDecodeError::UnsupportedRxdWidth { bit_width }),
    }
}

pub fn apply_mth(raw: u32, mth: GmMth) -> Result<f64, GmEnhancedDecodeError> {
    if mth.divisor == 0 {
        return Err(GmEnhancedDecodeError::MthZeroDivisor { raw: mth.raw });
    }
    Ok((raw as f64 * f64::from(mth.multiplier) / f64::from(mth.divisor)) + f64::from(mth.offset))
}

const fn injector_pulse_width(
    cylinder: u8,
    did: u16,
    txd: &'static str,
    rxf: &'static str,
) -> GmDidDefinition {
    let name = match cylinder {
        1 => "injector pulse width cyl 1",
        2 => "injector pulse width cyl 2",
        3 => "injector pulse width cyl 3",
        4 => "injector pulse width cyl 4",
        5 => "injector pulse width cyl 5",
        6 => "injector pulse width cyl 6",
        7 => "injector pulse width cyl 7",
        8 => "injector pulse width cyl 8",
        _ => "injector pulse width",
    };
    GmDidDefinition {
        did,
        selector: SELECTOR_01,
        module: ECM,
        name,
        unit: "ms",
        service: MODE_22_READ_DATA_BY_ID,
        txd,
        rxf: Some(rxf),
        rxd: Some(RXD_3010),
        raw_mth: Some("00C800830000"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3010,
            mth: MTH_PULSE_MS,
        },
        confidence: Confidence::Community,
        provenance: PROV_SCANGAUGE,
        evidence_id: None,
        cadence: PollCadence::Medium,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "ScanGauge-published injector pulse width; lower priority than balance rate.",
    }
}

const fn injector_balance(
    cylinder: u8,
    did: u16,
    txd: &'static str,
    rxf: &'static str,
) -> GmDidDefinition {
    let name = match cylinder {
        1 => "injector balance cyl 1",
        2 => "injector balance cyl 2",
        3 => "injector balance cyl 3",
        4 => "injector balance cyl 4",
        5 => "injector balance cyl 5",
        6 => "injector balance cyl 6",
        7 => "injector balance cyl 7",
        8 => "injector balance cyl 8",
        _ => "injector balance",
    };
    GmDidDefinition {
        did,
        selector: SELECTOR_01,
        module: ECM,
        name,
        unit: "mm3",
        service: MODE_22_READ_DATA_BY_ID,
        txd,
        rxf: Some(rxf),
        rxd: Some(RXD_3010),
        raw_mth: Some("00050020EC00"),
        decode: GmDecodeKind::RxdMth {
            rxd: RXD_3010,
            mth: MTH_BALANCE_MM3,
        },
        confidence: Confidence::LiveObserved,
        provenance: PROV_SCANGAUGE_LIVE,
        evidence_id: None,
        cadence: PollCadence::Medium,
        failure_policy: FailurePolicy::SurfaceUnavailable,
        range_suspect: false,
        notes: "Warm idle diagnostic value; offset is signed MTH word 0xEC00.",
    }
}

fn parse_hex_byte(text: &str) -> Result<u8, GmEnhancedDecodeError> {
    u8::from_str_radix(text, 16).map_err(|_| GmEnhancedDecodeError::InvalidHexByte {
        text: text.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded_lly_spec() -> obd2_core::vehicle::VehicleSpec {
        obd2_core::specs::embedded::load_embedded_specs()
            .into_iter()
            .find(|spec| spec.identity.engine.code == "LLY")
            .expect("embedded LLY spec")
    }

    #[test]
    fn lly_profile_gate_accepts_known_lly_vin() {
        let spec = embedded_lly_spec();

        assert!(lly_profile_matches(
            "1GTHK29294E391526",
            Some(&spec),
            Protocol::J1850Vpw
        ));
    }

    #[test]
    fn lly_profile_gate_rejects_missing_spec_or_wrong_protocol() {
        let spec = embedded_lly_spec();

        assert!(!lly_profile_matches(
            "1GTHK29294E391526",
            None,
            Protocol::J1850Vpw
        ));
        assert!(!lly_profile_matches(
            "1GTHK29294E391526",
            Some(&spec),
            Protocol::Can11Bit500
        ));
    }

    #[test]
    fn lly_profile_gate_rejects_wrong_vin_engine_digit() {
        let spec = embedded_lly_spec();

        assert!(!lly_profile_matches(
            "1GTHK29994E391526",
            Some(&spec),
            Protocol::J1850Vpw
        ));
    }

    #[test]
    fn lly_profile_gate_rejects_wrong_spec_identity() {
        let mut spec = embedded_lly_spec();
        spec.identity.engine.code = "LB7".to_string();

        assert!(!lly_profile_matches(
            "1GTHK29294E391526",
            Some(&spec),
            Protocol::J1850Vpw
        ));
    }

    #[test]
    fn registry_contains_lly_foundation_entries() {
        assert_eq!(LLY_ENHANCED_DIDS.len(), 24);
        assert!(find_lly_did(0x1251).is_some());
        assert!(find_lly_did(0x1542).is_some());

        let rail_desired = find_lly_did(0x163D).unwrap();
        let rail_actual = find_lly_did(0x163E).unwrap();
        assert!(rail_desired.range_suspect);
        assert!(rail_actual.range_suspect);
        assert_eq!(rail_actual.failure_policy, FailurePolicy::PreferStandardPid);
    }

    #[test]
    fn rejected_dids_are_preserved() {
        let rejected: Vec<u16> = LLY_REJECTED_DIDS.iter().map(|entry| entry.did).collect();
        assert_eq!(rejected, vec![0x1170, 0x1171, 0x1172, 0x1117, 0x119D]);
        assert_eq!(
            LLY_REJECTED_DIDS
                .iter()
                .find(|entry| entry.did == 0x119D)
                .unwrap()
                .disposition,
            RejectedDisposition::Unresolved
        );
    }

    #[test]
    fn builds_mode22_routed_request_from_definition() {
        let definition = find_lly_did(0x1940).unwrap();
        let request = definition.routed_request();
        assert_eq!(definition.request_header(), [0x6C, 0x18, 0xF1]);
        assert_eq!(request.service_id, 0x22);
        assert_eq!(request.data, vec![0x19, 0x40, 0x01]);
    }

    #[test]
    fn decodes_full_positive_mode22_with_selector_byte() {
        let definition = find_lly_did(0x1540).unwrap();
        let decoded = definition
            .decode_value(&[0x62, 0x15, 0x40, 0x01, 0x80])
            .unwrap();
        assert_eq!(decoded.selected_raw, 0x80);
        assert!((decoded.value - 50.196).abs() < 0.01);
        assert_eq!(decoded.unit, "%");
    }

    #[test]
    fn decodes_adapter_stripped_mode22_payload() {
        let definition = find_lly_did(0x1251).unwrap();
        let decoded = definition.decode_value(&[0x01, 0x64]).unwrap();
        assert_eq!(decoded.selected_raw, 100);
        assert_eq!(decoded.value, 100.0);
        assert_eq!(decoded.unit, "kPa abs");
    }

    #[test]
    fn decodes_rxd_16_bit_payload_and_signed_mth_offset() {
        let definition = find_lly_did(0x162F).unwrap();
        let decoded = definition.decode_value(&[0x01, 0x80, 0x00]).unwrap();
        assert_eq!(decoded.selected_raw, 0x8000);
        assert_eq!(decoded.value, 0.0);
    }

    #[test]
    fn parses_scangauge_rxd_and_mth_signed_words() {
        let rxd = GmRxd::from_scangauge("3010").unwrap();
        assert_eq!(rxd.byte_index, 0);
        assert_eq!(rxd.bit_width, 16);

        let mth = GmMth::from_scangauge("00050020EC00").unwrap();
        assert_eq!(mth.multiplier, 5);
        assert_eq!(mth.divisor, 32);
        assert_eq!(mth.offset, -5120);
        assert_eq!(apply_mth(0x8000, mth).unwrap(), 0.0);
    }

    #[test]
    fn rejects_mismatched_positive_did() {
        let definition = find_lly_did(0x1542).unwrap();
        let err = definition
            .decode_value(&[0x62, 0x15, 0x43, 0x01, 0x64])
            .unwrap_err();
        assert!(matches!(
            err,
            GmEnhancedDecodeError::MismatchedPositiveResponse {
                expected_did: 0x1542,
                response_did: 0x1543
            }
        ));
    }
}
