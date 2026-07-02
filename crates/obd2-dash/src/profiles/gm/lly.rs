use obd2_core::protocol::codec::BusFamily;
use obd2_core::vehicle::Protocol;

use crate::gm_class2::{
    decode_class2_dtcs, CLASS2_DTC_ACTIVE_REQUEST, CLASS2_DTC_ALL_REQUEST,
    POSITIVE_REPORT_DTCS_BY_STATUS, SERVICE_REPORT_DTCS_BY_STATUS,
};
use crate::gm_enhanced::{
    self, is_lly_spec_identity, lly_profile_matches, GmDidDefinition, GmEnhancedDecodeError,
    MODE_22_READ_DATA_BY_ID,
};

use super::super::model::{
    ActiveTestDefinition, AddressState, AddressTemplate, BackoffPolicy, BusDefinition, BusKey,
    Confidence, DecodedDtc, DecodedSignal, DiagnosticProfile, DtcServiceDefinition, EvidencePolicy,
    FailurePolicy, J1850HeaderConvention, Manufacturer, ModuleDefinition, ModuleKey, ModuleMap,
    ModuleSafetyClass, PairRole, PassiveMonitorDefinition, PollCadence, ProfileDecodeError,
    ProfileId, ProfileMatch, Provenance, RouteDefinition, RouteSet, RxdSource, SignalCategory,
    SignalComposition, SignalDefinition, SignalDisplayDefinition, SignalDisplaySource,
    SourceFields, StandardPidOverride, StandardPidPolicy, VehicleContext,
};
use super::super::selection::validate_vin_charset;
use super::active;

pub struct GmLlyClass2Profile;

pub static GM_LLY_CLASS2_PROFILE: GmLlyClass2Profile = GmLlyClass2Profile;

const ID: ProfileId = ProfileId::new("gm.gmt800.lly.class2");
const ALLOWED_PROTOCOLS: &[Protocol] = &[Protocol::J1850Vpw];
const J1850_BUS: BusKey = BusKey::new("j1850vpw");

const BUSES: &[BusDefinition] = &[BusDefinition {
    key: J1850_BUS,
    family: BusFamily::J1850,
    protocol: Protocol::J1850Vpw,
    j1850: Some(J1850HeaderConvention {
        priority: 0x6C,
        source: 0xF1,
    }),
    label: "GM Class 2 J1850 VPW",
}];

const MODULES: &[ModuleDefinition] = &[
    ModuleDefinition {
        key: ModuleKey::Ecm,
        display_label: "Engine Control Module",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x10 }),
        safety_class: ModuleSafetyClass::Powertrain,
        coresident_with: None,
    },
    ModuleDefinition {
        key: ModuleKey::Tcm,
        display_label: "Transmission Control Module",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x18 }),
        safety_class: ModuleSafetyClass::Powertrain,
        coresident_with: None,
    },
    ModuleDefinition {
        key: ModuleKey::Ficm,
        display_label: "Fuel Injection Control Module",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x11 }),
        safety_class: ModuleSafetyClass::Powertrain,
        coresident_with: None,
    },
    ModuleDefinition {
        key: ModuleKey::Bcm,
        display_label: "Body Control Module",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x40 }),
        safety_class: ModuleSafetyClass::Informational,
        coresident_with: None,
    },
    ModuleDefinition {
        key: ModuleKey::Ebcm,
        display_label: "Electronic Brake Control Module",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x29 }),
        safety_class: ModuleSafetyClass::WriteForbidden,
        coresident_with: None,
    },
];

const MODULE_MAP: ModuleMap = ModuleMap {
    buses: BUSES,
    modules: MODULES,
};

const LLY_FORCED_STANDARD_PIDS: &[u8] = &[
    0x04, 0x05, 0x0B, 0x0C, 0x0D, 0x0F, 0x10, 0x11, 0x23, 0x33, 0x42, 0x46, 0x5C,
];

const PROV_SCANGAUGE: &[Provenance] = &[Provenance::ScanGaugePublished];
const PROV_SCANGAUGE_LIVE: &[Provenance] =
    &[Provenance::ScanGaugePublished, Provenance::LiveObserved];
const PROV_LIVE: &[Provenance] = &[Provenance::LiveObserved];

const RXD_3008: RxdSource = RxdSource {
    raw: "3008",
    bit_width: Some(8),
};
const RXD_3010: RxdSource = RxdSource {
    raw: "3010",
    bit_width: Some(16),
};

macro_rules! source_fields {
    ($txd:literal, $rxf:expr, $rxd:expr, $raw_mth:expr) => {
        SourceFields {
            txd: $txd,
            rxf: $rxf,
            rxd: $rxd,
            raw_mth: $raw_mth,
            source_ref: None,
        }
    };
}

macro_rules! lly_signal {
    (
        $key:literal,
        $label:literal,
        $module:expr,
        [$($request:expr),+ $(,)?],
        $unit:literal,
        $cadence:expr,
        $confidence:expr,
        $provenance:expr,
        $source_fields:expr,
        $failure:expr,
        $preferred_over:expr
    ) => {
        SignalDefinition {
            key: $key,
            label: $label,
            category: SignalCategory::Powertrain,
            route: RouteDefinition { module: $module },
            service_id: MODE_22_READ_DATA_BY_ID,
            request_data: &[$($request),+],
            decoder_id: "gm.lly.class2.mode22",
            unit: $unit,
            cadence: $cadence,
            confidence: $confidence,
            provenance: $provenance,
            source_fields: $source_fields,
            evidence_policy: EvidencePolicy::OnError,
            failure_policy: $failure,
            preferred_over: $preferred_over,
        }
    };
}

macro_rules! display_profile {
    ($key:literal, $label:literal, $category:expr, $unit:literal, $composition:expr) => {
        SignalDisplayDefinition {
            key: $key,
            label: $label,
            category: $category,
            unit: $unit,
            source: SignalDisplaySource::ProfileSignal($key),
            composition: $composition,
        }
    };
}

macro_rules! display_standard {
    ($key:literal, $label:literal, $category:expr, $pid:expr, $unit:literal, $composition:expr) => {
        SignalDisplayDefinition {
            key: $key,
            label: $label,
            category: $category,
            unit: $unit,
            source: SignalDisplaySource::StandardPid($pid),
            composition: $composition,
        }
    };
}

macro_rules! display_derived {
    (
        $key:literal,
        $label:literal,
        $category:expr,
        $unit:literal,
        $formula:literal,
        $inputs:expr,
        $composition:expr
    ) => {
        SignalDisplayDefinition {
            key: $key,
            label: $label,
            category: $category,
            unit: $unit,
            source: SignalDisplaySource::Derived {
                formula_key: $formula,
                input_keys: $inputs,
            },
            composition: $composition,
        }
    };
}

pub const LLY_SIGNALS: &[SignalDefinition] = &[
    lly_signal!(
        "lly.1940",
        "transmission temperature",
        ModuleKey::Tcm,
        [0x19, 0x40, 0x01],
        "deg F",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C18F122194001",
            Some("046205190640"),
            Some(RXD_3008),
            Some("00090005FFD8")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1470",
        "oil pressure",
        ModuleKey::Ecm,
        [0x14, 0x70, 0x01],
        "psi",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122147001",
            Some("046205140670"),
            Some(RXD_3008),
            Some("001D00320000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.163D",
        "desired fuel rail pressure",
        ModuleKey::Ecm,
        [0x16, 0x3D, 0x01],
        "psi",
        PollCadence::Fast,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163D01",
            Some("04624516063D"),
            Some(RXD_3008),
            Some("0091000A0000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.163E",
        "actual fuel rail pressure",
        ModuleKey::Ecm,
        [0x16, 0x3E, 0x01],
        "psi",
        PollCadence::Fast,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163E01",
            Some("04624516063E"),
            Some(RXD_3008),
            Some("0091000A0000")
        ),
        FailurePolicy::PreferStandardPid,
        Some("standard:23")
    ),
    lly_signal!(
        "lly.1540",
        "VGT vane desired",
        ModuleKey::Ecm,
        [0x15, 0x40, 0x01],
        "%",
        PollCadence::Fast,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122154001",
            Some("046205150640"),
            Some(RXD_3008),
            Some("006400FF0000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1543",
        "VGT vane actual",
        ModuleKey::Ecm,
        [0x15, 0x43, 0x01],
        "%",
        PollCadence::Fast,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122154301",
            Some("046205160643"),
            Some(RXD_3008),
            Some("006400FF0000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1193",
        "injector pulse width cyl 1",
        ModuleKey::Ecm,
        [0x11, 0x93, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119301",
            Some("046245110693"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1194",
        "injector pulse width cyl 2",
        ModuleKey::Ecm,
        [0x11, 0x94, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119401",
            Some("046245110694"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1195",
        "injector pulse width cyl 3",
        ModuleKey::Ecm,
        [0x11, 0x95, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119501",
            Some("046245110695"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1196",
        "injector pulse width cyl 4",
        ModuleKey::Ecm,
        [0x11, 0x96, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119601",
            Some("046245110696"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1197",
        "injector pulse width cyl 5",
        ModuleKey::Ecm,
        [0x11, 0x97, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119701",
            Some("046245110697"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1198",
        "injector pulse width cyl 6",
        ModuleKey::Ecm,
        [0x11, 0x98, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119801",
            Some("046245110698"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1199",
        "injector pulse width cyl 7",
        ModuleKey::Ecm,
        [0x11, 0x99, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119901",
            Some("046245110699"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.119A",
        "injector pulse width cyl 8",
        ModuleKey::Ecm,
        [0x11, 0x9A, 0x01],
        "ms",
        PollCadence::Medium,
        Confidence::Community,
        PROV_SCANGAUGE,
        source_fields!(
            "6C10F122119A01",
            Some("04624511069A"),
            Some(RXD_3010),
            Some("00C800830000")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.162F",
        "injector balance cyl 1",
        ModuleKey::Ecm,
        [0x16, 0x2F, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122162F01",
            Some("04628516062F"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1630",
        "injector balance cyl 2",
        ModuleKey::Ecm,
        [0x16, 0x30, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163001",
            Some("046285160630"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1631",
        "injector balance cyl 3",
        ModuleKey::Ecm,
        [0x16, 0x31, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163101",
            Some("046285160631"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1632",
        "injector balance cyl 4",
        ModuleKey::Ecm,
        [0x16, 0x32, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163201",
            Some("046285160632"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1633",
        "injector balance cyl 5",
        ModuleKey::Ecm,
        [0x16, 0x33, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163301",
            Some("046285160633"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1634",
        "injector balance cyl 6",
        ModuleKey::Ecm,
        [0x16, 0x34, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163401",
            Some("046285160634"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1635",
        "injector balance cyl 7",
        ModuleKey::Ecm,
        [0x16, 0x35, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163501",
            Some("046285160635"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1636",
        "injector balance cyl 8",
        ModuleKey::Ecm,
        [0x16, 0x36, 0x01],
        "mm3",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_SCANGAUGE_LIVE,
        source_fields!(
            "6C10F122163601",
            Some("046285160636"),
            Some(RXD_3010),
            Some("00050020EC00")
        ),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1251",
        "barometric pressure",
        ModuleKey::Ecm,
        [0x12, 0x51, 0x01],
        "kPa abs",
        PollCadence::Medium,
        Confidence::LiveObserved,
        PROV_LIVE,
        source_fields!("6C10F122125101", None, Some(RXD_3008), None),
        FailurePolicy::SurfaceUnavailable,
        None
    ),
    lly_signal!(
        "lly.1542",
        "desired MAP",
        ModuleKey::Ecm,
        [0x15, 0x42, 0x01],
        "kPa abs",
        PollCadence::Fast,
        Confidence::Candidate,
        PROV_LIVE,
        source_fields!("6C10F122154201", None, Some(RXD_3008), None),
        FailurePolicy::CandidateOnly,
        None
    ),
];

const VGT_ERROR_INPUTS: &[&str] = &["lly.1543", "lly.1540"];
const FUEL_RAIL_ACTUAL_INPUTS: &[&str] = &["standard:23", "lly.163E"];
const FUEL_RAIL_DELTA_INPUTS: &[&str] = &["lly.fuel_rail.actual", "lly.163D"];
const BAROMETRIC_INPUTS: &[&str] = &["standard:33", "lly.1251"];
const BOOST_INPUTS: &[&str] = &["standard:0B", "lly.barometric_pressure"];
const DESIRED_MAP_INPUTS: &[&str] = &["lly.1542"];

pub const LLY_SIGNAL_DISPLAY: &[SignalDisplayDefinition] = &[
    display_profile!(
        "lly.1543",
        "VGT vane actual",
        SignalCategory::Turbo,
        "%",
        SignalComposition::Pair {
            group_key: "lly.vgt_vane",
            role: PairRole::Actual,
        }
    ),
    display_profile!(
        "lly.1540",
        "VGT vane desired",
        SignalCategory::Turbo,
        "%",
        SignalComposition::Pair {
            group_key: "lly.vgt_vane",
            role: PairRole::Desired,
        }
    ),
    display_derived!(
        "lly.vgt_vane.error",
        "VGT vane error",
        SignalCategory::Turbo,
        "%",
        "actual_minus_desired",
        VGT_ERROR_INPUTS,
        SignalComposition::Pair {
            group_key: "lly.vgt_vane",
            role: PairRole::Error,
        }
    ),
    display_derived!(
        "lly.fuel_rail.actual",
        "actual fuel rail pressure",
        SignalCategory::Fuel,
        "psi",
        "first_available",
        FUEL_RAIL_ACTUAL_INPUTS,
        SignalComposition::Pair {
            group_key: "lly.fuel_rail",
            role: PairRole::Actual,
        }
    ),
    display_profile!(
        "lly.163D",
        "desired fuel rail pressure",
        SignalCategory::Fuel,
        "psi",
        SignalComposition::Pair {
            group_key: "lly.fuel_rail",
            role: PairRole::Desired,
        }
    ),
    display_derived!(
        "lly.fuel_rail.delta",
        "fuel rail delta",
        SignalCategory::Fuel,
        "psi",
        "actual_minus_desired",
        FUEL_RAIL_DELTA_INPUTS,
        SignalComposition::Pair {
            group_key: "lly.fuel_rail",
            role: PairRole::Delta,
        }
    ),
    display_profile!(
        "lly.162F",
        "injector balance cyl 1",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 0,
            row_label: "1",
        }
    ),
    display_profile!(
        "lly.1630",
        "injector balance cyl 2",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 1,
            row_label: "2",
        }
    ),
    display_profile!(
        "lly.1631",
        "injector balance cyl 3",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 2,
            row_label: "3",
        }
    ),
    display_profile!(
        "lly.1632",
        "injector balance cyl 4",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 3,
            row_label: "4",
        }
    ),
    display_profile!(
        "lly.1633",
        "injector balance cyl 5",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 4,
            row_label: "5",
        }
    ),
    display_profile!(
        "lly.1634",
        "injector balance cyl 6",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 5,
            row_label: "6",
        }
    ),
    display_profile!(
        "lly.1635",
        "injector balance cyl 7",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 6,
            row_label: "7",
        }
    ),
    display_profile!(
        "lly.1636",
        "injector balance cyl 8",
        SignalCategory::Fuel,
        "mm3",
        SignalComposition::TableRow {
            table_key: "lly.injector_balance",
            row_index: 7,
            row_label: "8",
        }
    ),
    display_standard!(
        "standard:0B",
        "Intake MAP",
        SignalCategory::Turbo,
        0x0B,
        "psi",
        SignalComposition::Pair {
            group_key: "lly.map_pressure",
            role: PairRole::Actual,
        }
    ),
    display_derived!(
        "lly.desired_map",
        "desired MAP",
        SignalCategory::Turbo,
        "psi",
        "profile_desired_map_to_psi",
        DESIRED_MAP_INPUTS,
        SignalComposition::Pair {
            group_key: "lly.map_pressure",
            role: PairRole::Desired,
        }
    ),
    display_derived!(
        "lly.barometric_pressure",
        "barometric pressure",
        SignalCategory::Turbo,
        "psi",
        "first_available",
        BAROMETRIC_INPUTS,
        SignalComposition::Scalar
    ),
    display_derived!(
        "lly.boost_pressure",
        "boost pressure",
        SignalCategory::Turbo,
        "psi",
        "max_zero_subtract",
        BOOST_INPUTS,
        SignalComposition::Scalar
    ),
    display_standard!(
        "standard:10",
        "MAF",
        SignalCategory::Turbo,
        0x10,
        "g/s",
        SignalComposition::Scalar
    ),
    display_standard!(
        "standard:05",
        "coolant temperature",
        SignalCategory::Powertrain,
        0x05,
        "F",
        SignalComposition::Scalar
    ),
    display_standard!(
        "standard:0F",
        "intake air temperature",
        SignalCategory::Powertrain,
        0x0F,
        "F",
        SignalComposition::Scalar
    ),
    display_standard!(
        "standard:5C",
        "engine oil temperature",
        SignalCategory::Powertrain,
        0x5C,
        "F",
        SignalComposition::Scalar
    ),
    display_standard!(
        "standard:46",
        "ambient air temperature",
        SignalCategory::Body,
        0x46,
        "F",
        SignalComposition::Scalar
    ),
    display_profile!(
        "lly.1940",
        "transmission temperature",
        SignalCategory::Transmission,
        "F",
        SignalComposition::Scalar
    ),
    display_profile!(
        "lly.1470",
        "oil pressure",
        SignalCategory::Powertrain,
        "psi",
        SignalComposition::Scalar
    ),
];

const LLY_DTC_BACKOFF: BackoffPolicy = BackoffPolicy {
    skip_after_misses: 1,
    max_skips: 3,
};

const LLY_DTC_SERVICES: &[DtcServiceDefinition] = &[
    DtcServiceDefinition {
        key: "lly.class2.dtc.all",
        label: "GM Class 2 all DTCs",
        route_set: RouteSet::discovered_on_bus(J1850_BUS),
        service_id: SERVICE_REPORT_DTCS_BY_STATUS,
        request_data: &CLASS2_DTC_ALL_REQUEST,
        decoder_id: "gm.class2.dtc",
        backoff_policy: LLY_DTC_BACKOFF,
        cadence: PollCadence::Slow,
    },
    DtcServiceDefinition {
        key: "lly.class2.dtc.active",
        label: "GM Class 2 active/history DTCs",
        route_set: RouteSet::discovered_on_bus(J1850_BUS),
        service_id: SERVICE_REPORT_DTCS_BY_STATUS,
        request_data: &CLASS2_DTC_ACTIVE_REQUEST,
        decoder_id: "gm.class2.dtc",
        backoff_policy: LLY_DTC_BACKOFF,
        cadence: PollCadence::Slow,
    },
];

impl DiagnosticProfile for GmLlyClass2Profile {
    fn id(&self) -> ProfileId {
        ID
    }

    fn manufacturer(&self) -> Manufacturer {
        Manufacturer::Gm
    }

    fn allowed_protocols(&self) -> &'static [Protocol] {
        ALLOWED_PROTOCOLS
    }

    fn module_map(&self) -> Option<&ModuleMap> {
        Some(&MODULE_MAP)
    }

    fn matches(&self, ctx: &VehicleContext) -> ProfileMatch {
        if ctx.protocol != Protocol::J1850Vpw {
            return ProfileMatch::NoMatch;
        }

        let Some(vin) = ctx.vin.as_deref() else {
            return match ctx.spec.as_ref() {
                Some(spec) if is_lly_spec_identity(spec) => ProfileMatch::Partial {
                    reason: "LLY spec identity present but VIN is unread".into(),
                },
                _ => ProfileMatch::NoMatch,
            };
        };

        let spec = ctx.spec.as_ref();
        if lly_profile_matches(vin, spec, ctx.protocol) {
            if ctx.vin_confidence.is_trusted() && validate_vin_charset(vin) {
                return ProfileMatch::Exact {
                    confidence: super::super::model::MatchConfidence::VinPlusSpec,
                };
            }
            return ProfileMatch::Partial {
                reason: "LLY identity matched but VIN is not confirmed".into(),
            };
        }

        match spec {
            Some(spec) if !is_lly_spec_identity(spec) => ProfileMatch::NoMatch,
            Some(_) if vin_has_lly_hint(vin) => ProfileMatch::Partial {
                reason: "LLY VIN hint present but full legacy gate did not match".into(),
            },
            None if vin_has_lly_hint(vin) => ProfileMatch::Partial {
                reason: "LLY VIN hint present but no decoded spec is loaded".into(),
            },
            _ => ProfileMatch::NoMatch,
        }
    }

    fn standard_pid_overrides(&self) -> &[StandardPidOverride] {
        &[]
    }

    fn standard_pid_policy(&self) -> StandardPidPolicy {
        StandardPidPolicy {
            forced: LLY_FORCED_STANDARD_PIDS,
        }
    }

    fn signals(&self) -> &[SignalDefinition] {
        LLY_SIGNALS
    }

    fn signal_display(&self) -> &[SignalDisplayDefinition] {
        LLY_SIGNAL_DISPLAY
    }

    fn dtc_services(&self) -> &[DtcServiceDefinition] {
        LLY_DTC_SERVICES
    }

    fn active_tests(&self) -> &[ActiveTestDefinition] {
        active::active_tests()
    }

    fn passive_monitors(&self) -> &[PassiveMonitorDefinition] {
        &[]
    }

    fn decode_signal(
        &self,
        signal: &SignalDefinition,
        payload: &[u8],
    ) -> Result<DecodedSignal, ProfileDecodeError> {
        decode_lly_signal(signal, payload)
    }

    fn decode_dtc_response(
        &self,
        service: &DtcServiceDefinition,
        payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
        decode_lly_dtc_response(service, payload)
    }
}

fn lly_backing(did: u16) -> Option<&'static GmDidDefinition> {
    gm_enhanced::find_lly_did(did)
}

pub fn decode_lly_signal(
    signal: &SignalDefinition,
    payload: &[u8],
) -> Result<DecodedSignal, ProfileDecodeError> {
    if signal.decoder_id != "gm.lly.class2.mode22" {
        return Err(ProfileDecodeError::UnknownDecoder(signal.decoder_id));
    }
    if signal.request_data.len() < 2 {
        return Err(ProfileDecodeError::PayloadTooShort {
            expected: 2,
            got: signal.request_data.len(),
        });
    }

    let did = u16::from_be_bytes([signal.request_data[0], signal.request_data[1]]);
    let definition = lly_backing(did)
        .ok_or_else(|| ProfileDecodeError::Other(format!("unknown LLY DID 0x{did:04X}")))?;
    let decoded =
        gm_enhanced::decode_did_value(definition, payload).map_err(map_gm_decode_error)?;

    Ok(DecodedSignal {
        key: signal.key,
        value: decoded.value,
        unit: decoded.unit,
        raw: payload.to_vec(),
        selected_raw: selected_raw_bytes(decoded.selected_raw, definition),
        module: signal.route.module.to_core_module_id(),
        confidence: signal.confidence,
    })
}

pub fn decode_lly_dtc_response(
    service: &DtcServiceDefinition,
    payload: &[u8],
) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
    if service.decoder_id != "gm.class2.dtc" {
        return Err(ProfileDecodeError::UnknownDecoder(service.decoder_id));
    }
    if payload.len() >= 3 && payload[0] == 0x7F && payload[1] == SERVICE_REPORT_DTCS_BY_STATUS {
        return Err(ProfileDecodeError::NegativeResponse {
            service: payload[1],
            nrc: payload[2],
        });
    }

    let records =
        decode_class2_dtcs(payload).map_err(|err| ProfileDecodeError::Decode(err.to_string()))?;
    let raw_triplets = nonzero_class2_dtc_triplets(payload);
    let mut decoded = Vec::with_capacity(records.len());
    for (idx, record) in records.into_iter().enumerate() {
        let raw = raw_triplets
            .get(idx)
            .map(|triplet| triplet.to_vec())
            .unwrap_or_else(|| payload.to_vec());
        let status_flags = record
            .status
            .labels()
            .into_iter()
            .map(str::to_string)
            .collect();
        decoded.push(DecodedDtc {
            code: record.dtc.code,
            status: record.status.generic_status(),
            status_raw: Some(record.status.raw),
            status_flags,
            raw,
            module: None,
            notes: Some(format!(
                "GM Class 2 status 0x{:02X}: {}",
                record.status.raw,
                record.status.display_flags()
            )),
        });
    }
    Ok(decoded)
}

fn nonzero_class2_dtc_triplets(payload: &[u8]) -> Vec<[u8; 3]> {
    let payload = if payload.first().copied() == Some(POSITIVE_REPORT_DTCS_BY_STATUS) {
        &payload[1..]
    } else {
        payload
    };
    payload
        .chunks_exact(3)
        .filter_map(|chunk| {
            let triplet = [chunk[0], chunk[1], chunk[2]];
            (triplet != [0x00, 0x00, 0x00]).then_some(triplet)
        })
        .collect()
}

fn selected_raw_bytes(raw: u32, definition: &GmDidDefinition) -> Vec<u8> {
    let width = definition
        .rxd
        .map(|rxd| usize::from(rxd.bit_width) / 8)
        .unwrap_or(0);
    match width {
        1 => vec![raw as u8],
        2 => (raw as u16).to_be_bytes().to_vec(),
        _ => raw.to_be_bytes().to_vec(),
    }
}

fn map_gm_decode_error(error: GmEnhancedDecodeError) -> ProfileDecodeError {
    match error {
        GmEnhancedDecodeError::PayloadTooShort { needed, actual, .. } => {
            ProfileDecodeError::PayloadTooShort {
                expected: needed,
                got: actual,
            }
        }
        GmEnhancedDecodeError::MismatchedPositiveResponse { .. } => {
            ProfileDecodeError::MismatchedResponse
        }
        other => ProfileDecodeError::Decode(other.to_string()),
    }
}

fn vin_has_lly_hint(vin: &str) -> bool {
    let vin = vin.trim().to_ascii_uppercase();
    let bytes = vin.as_bytes();
    if bytes.len() < 8 || bytes[7] != b'2' {
        return false;
    }
    matches!(&bytes[..3], b"1GC" | b"1GT" | b"2GC")
}

#[allow(dead_code)]
const _: RouteDefinition = RouteDefinition {
    module: ModuleKey::Ecm,
};
#[allow(dead_code)]
const _: RouteSet = RouteSet::discovered_on_bus(J1850_BUS);
#[allow(dead_code)]
const _: BackoffPolicy = BackoffPolicy::NONE;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gm_enhanced::{
        Confidence as GmConfidence, FailurePolicy as GmFailurePolicy, PollCadence as GmPollCadence,
        Provenance as GmProvenance, ECM_NODE, LLY_ENHANCED_DIDS, LLY_REJECTED_DIDS, TCM_NODE,
    };
    use crate::profiles::runtime::resolve_route;
    use std::collections::HashSet;

    #[test]
    fn lly_signals_match_backing_registry() {
        assert_eq!(LLY_SIGNALS.len(), 24);
        assert_eq!(LLY_SIGNALS.len(), LLY_ENHANCED_DIDS.len());

        for signal in LLY_SIGNALS {
            let did = signal_did(signal);
            let backing = lly_backing(did)
                .unwrap_or_else(|| panic!("profile signal {} has no backing DID", signal.key));

            assert_eq!(signal.service_id, backing.service);
            assert_eq!(signal.request_data, backing.request_data().as_slice());
            assert_eq!(signal.label, backing.name);
            assert_eq!(signal.unit, backing.unit);
            assert_eq!(signal.source_fields.txd, backing.txd);
            assert_eq!(signal.source_fields.rxf, backing.rxf);
            assert_eq!(signal.source_fields.raw_mth, backing.raw_mth);
            assert_eq!(
                signal.source_fields.rxd.map(|rxd| (rxd.raw, rxd.bit_width)),
                backing.rxd.map(|rxd| (rxd.raw, Some(rxd.bit_width)))
            );
            assert_eq!(signal.confidence, profile_confidence(backing.confidence));
            assert_eq!(signal.cadence, profile_cadence(backing.cadence));
            assert_eq!(
                signal.failure_policy,
                profile_failure(backing.failure_policy)
            );
            assert_eq!(signal.provenance, profile_provenance(backing.provenance));
            assert_eq!(signal.route.module, expected_module(backing.module.node));
        }
    }

    #[test]
    fn lly_signals_exclude_rejected_dids() {
        let rejected: Vec<u16> = LLY_REJECTED_DIDS.iter().map(|entry| entry.did).collect();
        assert_eq!(rejected, vec![0x1170, 0x1171, 0x1172, 0x1117, 0x119D]);
        for signal in LLY_SIGNALS {
            assert!(!rejected.contains(&signal_did(signal)));
        }
    }

    #[test]
    fn lly_tcm_signal_routes_to_tcm() {
        for signal in LLY_SIGNALS {
            let did = signal_did(signal);
            if did == 0x1940 {
                assert_eq!(signal.route.module, ModuleKey::Tcm);
            } else {
                assert_eq!(signal.route.module, ModuleKey::Ecm, "DID 0x{did:04X}");
            }
        }
    }

    #[test]
    fn lly_routes_resolve_to_expected_headers() {
        for signal in LLY_SIGNALS {
            let did = signal_did(signal);
            let resolved = resolve_route(&MODULE_MAP, &signal.route, Protocol::J1850Vpw).unwrap();
            match did {
                0x1940 => {
                    assert_eq!(resolved.route.module, ModuleKey::Tcm);
                    assert_eq!(resolved.module.key.canonical(), "tcm");
                    assert_eq!(
                        resolved.physical_address,
                        obd2_core::vehicle::PhysicalAddress::J1850 {
                            node: 0x18,
                            header: [0x6C, 0x18, 0xF1],
                        }
                    );
                }
                _ => {
                    assert_eq!(resolved.route.module, ModuleKey::Ecm);
                    assert_eq!(resolved.module.key.canonical(), "ecm");
                    assert_eq!(
                        resolved.physical_address,
                        obd2_core::vehicle::PhysicalAddress::J1850 {
                            node: 0x10,
                            header: [0x6C, 0x10, 0xF1],
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn decode_signal_parity_full_and_stripped() {
        let full = find_signal(0x1540);
        let full_payload = [0x62, 0x15, 0x40, 0x01, 0x80];
        let full_backing = lly_backing(0x1540).unwrap();
        let legacy_full = gm_enhanced::decode_did_value(full_backing, &full_payload).unwrap();
        let decoded_full = decode_lly_signal(full, &full_payload).unwrap();
        assert_eq!(decoded_full.value.to_bits(), legacy_full.value.to_bits());
        assert_eq!(decoded_full.unit, legacy_full.unit);
        assert_eq!(decoded_full.selected_raw, vec![0x80]);
        assert_eq!(decoded_full.raw, full_payload);

        let stripped = find_signal(0x1251);
        let stripped_payload = [0x01, 0x64];
        let stripped_backing = lly_backing(0x1251).unwrap();
        let legacy_stripped =
            gm_enhanced::decode_did_value(stripped_backing, &stripped_payload).unwrap();
        let decoded_stripped = decode_lly_signal(stripped, &stripped_payload).unwrap();
        assert_eq!(
            decoded_stripped.value.to_bits(),
            legacy_stripped.value.to_bits()
        );
        assert_eq!(decoded_stripped.unit, legacy_stripped.unit);
        assert_eq!(decoded_stripped.selected_raw, vec![0x64]);
        assert_eq!(decoded_stripped.raw, stripped_payload);

        let wide = find_signal(0x162F);
        let wide_payload = [0x01, 0x80, 0x00];
        let decoded_wide = decode_lly_signal(wide, &wide_payload).unwrap();
        assert_eq!(decoded_wide.selected_raw, vec![0x80, 0x00]);
        assert_eq!(decoded_wide.raw, wide_payload);
    }

    #[test]
    fn decode_signal_error_mapping() {
        let signal = find_signal(0x1542);
        let mismatched = decode_lly_signal(signal, &[0x62, 0x15, 0x43, 0x01, 0x64]).unwrap_err();
        assert_eq!(mismatched, ProfileDecodeError::MismatchedResponse);

        let short = decode_lly_signal(find_signal(0x162F), &[0x01]).unwrap_err();
        assert_eq!(
            short,
            ProfileDecodeError::PayloadTooShort {
                expected: 2,
                got: 0,
            }
        );
    }

    #[test]
    fn decode_dtc_response_preserves_gm_class2_status() {
        let service = find_dtc_service("lly.class2.dtc.all");
        let decoded =
            decode_lly_dtc_response(service, &[0x59, 0x43, 0x79, 0x93, 0xD0, 0x24, 0x12]).unwrap();

        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].code, "C0379");
        assert_eq!(
            decoded[0].status,
            obd2_core::protocol::dtc::DtcStatus::Stored
        );
        assert_eq!(decoded[0].status_raw, Some(0x93));
        assert_eq!(
            decoded[0].status_flags,
            vec!["mil", "history", "current", "immature"]
        );
        assert_eq!(decoded[0].raw, vec![0x43, 0x79, 0x93]);
        assert_eq!(
            decoded[0].notes.as_deref(),
            Some("GM Class 2 status 0x93: mil|history|current|immature")
        );
        assert_eq!(decoded[1].code, "U1024");
        assert_eq!(decoded[1].status_raw, Some(0x12));
    }

    #[test]
    fn decode_dtc_response_guards_leading_negative_response() {
        let service = find_dtc_service("lly.class2.dtc.active");
        let err = decode_lly_dtc_response(service, &[0x7F, 0x19, 0x12]).unwrap_err();

        assert_eq!(
            err,
            ProfileDecodeError::NegativeResponse {
                service: 0x19,
                nrc: 0x12,
            }
        );
    }

    #[test]
    fn source_fields_preserved() {
        let signal = find_signal(0x163E);
        assert_eq!(signal.preferred_over, Some("standard:23"));
        assert_eq!(signal.failure_policy, FailurePolicy::PreferStandardPid);
        assert_eq!(signal.confidence, Confidence::LiveObserved);
        assert_eq!(
            signal.source_fields.rxd,
            Some(RxdSource {
                raw: "3008",
                bit_width: Some(8),
            })
        );
        assert_eq!(find_signal(0x1542).confidence, Confidence::Candidate);
    }

    #[test]
    fn lly_display_definitions_cover_expected_compositions() {
        assert_eq!(GM_LLY_CLASS2_PROFILE.signal_display(), LLY_SIGNAL_DISPLAY);

        assert_pair_role("lly.1543", "lly.vgt_vane", PairRole::Actual);
        assert_pair_role("lly.1540", "lly.vgt_vane", PairRole::Desired);
        assert_pair_role("lly.vgt_vane.error", "lly.vgt_vane", PairRole::Error);
        assert_eq!(
            find_display("lly.vgt_vane.error").source,
            SignalDisplaySource::Derived {
                formula_key: "actual_minus_desired",
                input_keys: VGT_ERROR_INPUTS,
            }
        );

        assert_pair_role("lly.fuel_rail.actual", "lly.fuel_rail", PairRole::Actual);
        assert_pair_role("lly.163D", "lly.fuel_rail", PairRole::Desired);
        assert_pair_role("lly.fuel_rail.delta", "lly.fuel_rail", PairRole::Delta);
        assert_eq!(
            find_display("lly.fuel_rail.actual").source,
            SignalDisplaySource::Derived {
                formula_key: "first_available",
                input_keys: FUEL_RAIL_ACTUAL_INPUTS,
            }
        );
        assert_eq!(
            find_display("lly.fuel_rail.delta").source,
            SignalDisplaySource::Derived {
                formula_key: "actual_minus_desired",
                input_keys: FUEL_RAIL_DELTA_INPUTS,
            }
        );

        assert_eq!(
            find_display("lly.barometric_pressure").source,
            SignalDisplaySource::Derived {
                formula_key: "first_available",
                input_keys: BAROMETRIC_INPUTS,
            }
        );
        assert_eq!(
            find_display("lly.boost_pressure").source,
            SignalDisplaySource::Derived {
                formula_key: "max_zero_subtract",
                input_keys: BOOST_INPUTS,
            }
        );
    }

    #[test]
    fn lly_injector_balance_display_is_eight_row_table() {
        let rows: Vec<_> = LLY_SIGNAL_DISPLAY
            .iter()
            .filter_map(|display| match display.composition {
                SignalComposition::TableRow {
                    table_key: "lly.injector_balance",
                    row_index,
                    row_label,
                } => Some((display.key, row_index, row_label)),
                _ => None,
            })
            .collect();

        assert_eq!(rows.len(), 8);
        for (idx, (key, row_index, row_label)) in rows.iter().enumerate() {
            assert_eq!(*row_index, idx as u8);
            assert_eq!(*row_label, (idx + 1).to_string());
            assert!(
                find_signal_by_key(key).is_some(),
                "missing signal for {key}"
            );
        }
    }

    #[test]
    fn lly_display_includes_standard_scalars_without_profile_signal_fork() {
        for (key, pid, unit) in [
            ("standard:10", 0x10, "g/s"),
            ("standard:05", 0x05, "F"),
            ("standard:0F", 0x0F, "F"),
            ("standard:5C", 0x5C, "F"),
            ("standard:46", 0x46, "F"),
        ] {
            let display = find_display(key);
            assert_eq!(display.source, SignalDisplaySource::StandardPid(pid));
            assert_eq!(display.unit, unit);
            assert_eq!(display.composition, SignalComposition::Scalar);
        }

        let map = find_display("standard:0B");
        assert_eq!(map.source, SignalDisplaySource::StandardPid(0x0B));
        assert_eq!(map.unit, "psi");
        assert_eq!(
            map.composition,
            SignalComposition::Pair {
                group_key: "lly.map_pressure",
                role: PairRole::Actual,
            }
        );
        assert_eq!(
            find_display("lly.desired_map").source,
            SignalDisplaySource::Derived {
                formula_key: "profile_desired_map_to_psi",
                input_keys: DESIRED_MAP_INPUTS,
            }
        );
        assert_eq!(
            find_display("lly.1940").category,
            SignalCategory::Transmission
        );
    }

    #[test]
    fn lly_display_profile_signal_refs_are_owned_by_lly_profile() {
        let profile_signal_keys: HashSet<&str> =
            LLY_SIGNALS.iter().map(|signal| signal.key).collect();
        let display_keys: HashSet<&str> = LLY_SIGNAL_DISPLAY
            .iter()
            .map(|display| display.key)
            .collect();

        for display in LLY_SIGNAL_DISPLAY {
            match display.source {
                SignalDisplaySource::ProfileSignal(signal_key) => {
                    assert!(
                        profile_signal_keys.contains(signal_key),
                        "display {} points at missing signal {}",
                        display.key,
                        signal_key
                    );
                }
                SignalDisplaySource::StandardPid(_) => {}
                SignalDisplaySource::Derived { input_keys, .. } => {
                    for input in input_keys {
                        assert!(
                            profile_signal_keys.contains(input)
                                || display_keys.contains(input)
                                || input.starts_with("standard:"),
                            "display {} derived input {} is not known",
                            display.key,
                            input
                        );
                    }
                }
            }
        }
    }

    fn find_signal(did: u16) -> &'static SignalDefinition {
        LLY_SIGNALS
            .iter()
            .find(|signal| signal_did(signal) == did)
            .unwrap_or_else(|| panic!("missing signal 0x{did:04X}"))
    }

    fn find_signal_by_key(key: &str) -> Option<&'static SignalDefinition> {
        LLY_SIGNALS.iter().find(|signal| signal.key == key)
    }

    fn find_display(key: &str) -> &'static SignalDisplayDefinition {
        LLY_SIGNAL_DISPLAY
            .iter()
            .find(|display| display.key == key)
            .unwrap_or_else(|| panic!("missing display definition {key}"))
    }

    fn assert_pair_role(key: &str, group_key: &'static str, role: PairRole) {
        let display = find_display(key);
        assert_eq!(
            display.composition,
            SignalComposition::Pair { group_key, role }
        );
    }

    fn find_dtc_service(key: &str) -> &'static DtcServiceDefinition {
        LLY_DTC_SERVICES
            .iter()
            .find(|service| service.key == key)
            .unwrap_or_else(|| panic!("missing DTC service {key}"))
    }

    fn signal_did(signal: &SignalDefinition) -> u16 {
        assert!(signal.request_data.len() >= 3);
        assert_eq!(signal.request_data[2], 0x01);
        u16::from_be_bytes([signal.request_data[0], signal.request_data[1]])
    }

    fn expected_module(node: u8) -> ModuleKey {
        match node {
            ECM_NODE => ModuleKey::Ecm,
            TCM_NODE => ModuleKey::Tcm,
            other => panic!("unexpected LLY signal node 0x{other:02X}"),
        }
    }

    fn profile_confidence(confidence: GmConfidence) -> Confidence {
        match confidence {
            GmConfidence::Candidate => Confidence::Candidate,
            GmConfidence::LiveObserved => Confidence::LiveObserved,
            GmConfidence::Community => Confidence::Community,
            GmConfidence::Verified => Confidence::Verified,
            GmConfidence::Rejected => Confidence::Rejected,
        }
    }

    fn profile_cadence(cadence: GmPollCadence) -> PollCadence {
        match cadence {
            GmPollCadence::Fast => PollCadence::Fast,
            GmPollCadence::Medium => PollCadence::Medium,
            GmPollCadence::Slow => PollCadence::Slow,
            GmPollCadence::OnDemand => PollCadence::OnDemand,
        }
    }

    fn profile_failure(policy: GmFailurePolicy) -> FailurePolicy {
        match policy {
            GmFailurePolicy::SurfaceUnavailable => FailurePolicy::SurfaceUnavailable,
            GmFailurePolicy::PreferStandardPid => FailurePolicy::PreferStandardPid,
            GmFailurePolicy::CandidateOnly => FailurePolicy::CandidateOnly,
            GmFailurePolicy::DoNotPoll => FailurePolicy::DoNotPoll,
        }
    }

    fn profile_provenance(provenance: &[GmProvenance]) -> Vec<Provenance> {
        provenance
            .iter()
            .map(|item| match item {
                GmProvenance::ScanGaugePublished => Provenance::ScanGaugePublished,
                GmProvenance::LiveObserved => Provenance::LiveObserved,
                GmProvenance::LegacySpec => Provenance::LegacySpec,
                GmProvenance::LocalRejection => Provenance::LocalRejection,
            })
            .collect()
    }
}
