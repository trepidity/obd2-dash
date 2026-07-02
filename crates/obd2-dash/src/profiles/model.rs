use obd2_core::protocol::codec::BusFamily;
use obd2_core::protocol::dtc::DtcStatus;
use obd2_core::vehicle::{ModuleId, Protocol, VehicleSpec};

// ---- Identity ----------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProfileId(pub &'static str);

impl ProfileId {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Manufacturer {
    Gm,
    Ford,
    ChryslerRam,
    Fixture,
    Generic,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdentityConfidence {
    Unread,
    Corrupted,
    Single,
    Confirmed,
}

impl IdentityConfidence {
    pub const fn is_trusted(&self) -> bool {
        matches!(self, IdentityConfidence::Confirmed)
    }
}

// ---- Match result ------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchConfidence {
    ProtocolPlusVinDimension,
    VinExact,
    VinPlusSpec,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProfileMatch {
    Exact { confidence: MatchConfidence },
    Partial { reason: String },
    NoMatch,
}

// ---- Vehicle context ---------------------------------------------------

#[derive(Clone, Debug)]
pub struct VehicleContext {
    pub generation: u64,
    pub protocol: Protocol,
    pub vin: Option<String>,
    pub vin_confidence: IdentityConfidence,
    pub spec: Option<VehicleSpec>,
    pub discovered_modules: Vec<ModuleId>,
    pub active_bus: Option<String>,
}

// ---- Sealed selected-profile token -------------------------------------

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedProfile {
    profile_id: ProfileId,
    context_generation: u64,
    manual_confirmed: bool,
    _sealed: (),
}

impl SelectedProfile {
    pub(in crate::profiles) fn seal(
        profile_id: ProfileId,
        context_generation: u64,
        manual_confirmed: bool,
    ) -> Self {
        Self {
            profile_id,
            context_generation,
            manual_confirmed,
            _sealed: (),
        }
    }

    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }

    pub fn context_generation(&self) -> u64 {
        self.context_generation
    }

    pub fn manual_confirmed(&self) -> bool {
        self.manual_confirmed
    }

    pub fn is_valid_for(&self, generation: u64) -> bool {
        self.context_generation == generation
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Selection {
    Matched(SelectedProfile),
    Partial {
        profile_id: ProfileId,
        reason: String,
    },
    None,
}

// ---- Routing -----------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BusKey(pub &'static str);

impl BusKey {
    pub const fn new(key: &'static str) -> Self {
        Self(key)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModuleKey {
    Ecm,
    Tcm,
    Ficm,
    Bcm,
    Ebcm,
    Ipc,
    Sdm,
    Hvac,
}

impl ModuleKey {
    pub const fn canonical(self) -> &'static str {
        match self {
            ModuleKey::Ecm => "ecm",
            ModuleKey::Tcm => "tcm",
            ModuleKey::Ficm => "ficm",
            ModuleKey::Bcm => "bcm",
            ModuleKey::Ebcm => "abs",
            ModuleKey::Ipc => "ipc",
            ModuleKey::Sdm => "airbag",
            ModuleKey::Hvac => "hvac",
        }
    }

    pub fn to_core_module_id(self) -> ModuleId {
        ModuleId::new(self.canonical())
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct RouteDefinition {
    pub module: ModuleKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressTemplate {
    J1850 { node: u8 },
    Can11 { request_id: u16, response_id: u16 },
    Can29 { request_id: u32, response_id: u32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteScope {
    Single(RouteDefinition),
    Explicit(&'static [RouteDefinition]),
    DiscoveredOnBus { bus: BusKey },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RouteSet {
    pub scope: RouteScope,
}

impl RouteSet {
    pub const fn single(route: RouteDefinition) -> Self {
        Self {
            scope: RouteScope::Single(route),
        }
    }

    pub const fn explicit(routes: &'static [RouteDefinition]) -> Self {
        Self {
            scope: RouteScope::Explicit(routes),
        }
    }

    pub const fn discovered_on_bus(bus: BusKey) -> Self {
        Self {
            scope: RouteScope::DiscoveredOnBus { bus },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct J1850HeaderConvention {
    pub priority: u8,
    pub source: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BusDefinition {
    pub key: BusKey,
    pub family: BusFamily,
    pub protocol: Protocol,
    pub j1850: Option<J1850HeaderConvention>,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressState {
    Confirmed(AddressTemplate),
    Candidate {
        templates: &'static [AddressTemplate],
        reason: &'static str,
    },
    Unresolved {
        reason: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleSafetyClass {
    Informational,
    Powertrain,
    WriteForbidden,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleDefinition {
    pub key: ModuleKey,
    pub display_label: &'static str,
    pub bus: BusKey,
    pub address: AddressState,
    pub safety_class: ModuleSafetyClass,
    pub coresident_with: Option<ModuleKey>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModuleMap {
    pub buses: &'static [BusDefinition],
    pub modules: &'static [ModuleDefinition],
}

// ---- Static capability evidence ---------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleEvidenceState {
    Candidate,
    Unsupported {
        nrc: Option<u8>,
        evidence_id: &'static str,
    },
    Probed {
        evidence_id: &'static str,
    },
    Confirmed {
        evidence_id: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct EvidenceRef(pub &'static str);

impl EvidenceRef {
    pub const fn new(id: &'static str) -> Self {
        Self(id)
    }

    pub const fn as_str(&self) -> &'static str {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PassiveCapabilityState {
    NotSeen,
    Observed {
        frames_ref: EvidenceRef,
        lines: u32,
        last_offset_ms: u32,
    },
}

// ---- Source fields -----------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RxdSource {
    pub raw: &'static str,
    pub bit_width: Option<u8>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceFields {
    pub txd: &'static str,
    pub rxf: Option<&'static str>,
    pub rxd: Option<RxdSource>,
    pub raw_mth: Option<&'static str>,
    pub source_ref: Option<&'static str>,
}

impl SourceFields {
    pub const NONE: Self = Self {
        txd: "",
        rxf: None,
        rxd: None,
        raw_mth: None,
        source_ref: None,
    };
}

// ---- Capability and poll vocabulary -----------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalCategory {
    Powertrain,
    Turbo,
    Fuel,
    Transmission,
    Body,
    Chassis,
    Emissions,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollCadence {
    Fast,
    Medium,
    Slow,
    OnDemand,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
    Candidate,
    LiveObserved,
    Community,
    Verified,
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Provenance {
    ScanGaugePublished,
    LiveObserved,
    LegacySpec,
    LocalRejection,
    LocalFixture,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailurePolicy {
    SurfaceUnavailable,
    PreferStandardPid,
    CandidateOnly,
    DoNotPoll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidencePolicy {
    None,
    OnError,
    OnDemand,
    BoundedLive,
    Always,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackoffPolicy {
    pub skip_after_misses: u8,
    pub max_skips: u8,
}

impl BackoffPolicy {
    pub const NONE: Self = Self {
        skip_after_misses: 0,
        max_skips: 0,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SafetyClass {
    Passive,
    StationaryOnly,
    IdleOnly,
    Locked,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub category: SignalCategory,
    pub route: RouteDefinition,
    pub service_id: u8,
    pub request_data: &'static [u8],
    pub decoder_id: &'static str,
    pub unit: &'static str,
    pub cadence: PollCadence,
    pub confidence: Confidence,
    pub provenance: &'static [Provenance],
    pub source_fields: SourceFields,
    pub evidence_policy: EvidencePolicy,
    pub failure_policy: FailurePolicy,
    pub preferred_over: Option<&'static str>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalDisplaySource {
    ProfileSignal(&'static str),
    StandardPid(u8),
    Derived {
        formula_key: &'static str,
        input_keys: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PairRole {
    Actual,
    Desired,
    Error,
    Delta,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SignalComposition {
    Scalar,
    Pair {
        group_key: &'static str,
        role: PairRole,
    },
    TableRow {
        table_key: &'static str,
        row_index: u8,
        row_label: &'static str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SignalDisplayDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub category: SignalCategory,
    pub unit: &'static str,
    pub source: SignalDisplaySource,
    pub composition: SignalComposition,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DtcServiceDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub route_set: RouteSet,
    pub service_id: u8,
    pub request_data: &'static [u8],
    pub decoder_id: &'static str,
    pub backoff_policy: BackoffPolicy,
    pub cadence: PollCadence,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardPidOverride {
    pub pid: u8,
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StandardPidPolicy {
    pub forced: &'static [u8],
}

impl StandardPidPolicy {
    pub const EMPTY: Self = Self { forced: &[] };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PassiveMonitorDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub route: RouteDefinition,
    pub decoder_id: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileRequestDefinition {
    pub route: RouteDefinition,
    pub service_id: u8,
    pub request_data: &'static [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivePrecondition {
    pub label: &'static str,
    pub detail: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActiveCommandProfile {
    Locked,
    Verified(ProfileRequestDefinition),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActiveTestDefinition {
    pub key: &'static str,
    pub label: &'static str,
    pub safety_class: SafetyClass,
    pub command_profile: ActiveCommandProfile,
    pub preconditions: &'static [ActivePrecondition],
    pub lock_reason: Option<&'static str>,
    pub supported_modes: &'static [&'static str],
    pub safety_notes: &'static [&'static str],
    pub timeout: std::time::Duration,
    pub cancel_command: Option<ProfileRequestDefinition>,
    pub evidence_policy: EvidencePolicy,
}

// ---- Decode output -----------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedSignal {
    pub key: &'static str,
    pub value: f64,
    pub unit: &'static str,
    pub raw: Vec<u8>,
    pub selected_raw: Vec<u8>,
    pub module: ModuleId,
    pub confidence: Confidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DecodedDtc {
    pub code: String,
    pub status: DtcStatus,
    pub status_raw: Option<u8>,
    pub status_flags: Vec<String>,
    pub raw: Vec<u8>,
    pub module: Option<ModuleId>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProfileDecodeError {
    PayloadTooShort { expected: usize, got: usize },
    MismatchedResponse,
    UnknownDecoder(&'static str),
    NegativeResponse { service: u8, nrc: u8 },
    Decode(String),
    Other(String),
}

pub trait DiagnosticProfile: Sync {
    fn id(&self) -> ProfileId;
    fn manufacturer(&self) -> Manufacturer;
    fn allowed_protocols(&self) -> &'static [Protocol] {
        &[]
    }
    fn module_map(&self) -> Option<&ModuleMap> {
        None
    }
    fn matches(&self, ctx: &VehicleContext) -> ProfileMatch;
    fn standard_pid_overrides(&self) -> &[StandardPidOverride];
    fn standard_pid_policy(&self) -> StandardPidPolicy {
        StandardPidPolicy::EMPTY
    }
    fn signals(&self) -> &[SignalDefinition];
    fn signal_display(&self) -> &[SignalDisplayDefinition] {
        &[]
    }
    fn dtc_services(&self) -> &[DtcServiceDefinition];
    fn active_tests(&self) -> &[ActiveTestDefinition];
    fn passive_monitors(&self) -> &[PassiveMonitorDefinition];

    fn decode_signal(
        &self,
        signal: &SignalDefinition,
        payload: &[u8],
    ) -> Result<DecodedSignal, ProfileDecodeError>;

    fn decode_dtc_response(
        &self,
        service: &DtcServiceDefinition,
        payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct ObjectSafeProfile;

    impl DiagnosticProfile for ObjectSafeProfile {
        fn id(&self) -> ProfileId {
            ProfileId::new("test.object-safe")
        }

        fn manufacturer(&self) -> Manufacturer {
            Manufacturer::Generic
        }

        fn matches(&self, _ctx: &VehicleContext) -> ProfileMatch {
            ProfileMatch::NoMatch
        }

        fn standard_pid_overrides(&self) -> &[StandardPidOverride] {
            &[]
        }

        fn signals(&self) -> &[SignalDefinition] {
            &[]
        }

        fn dtc_services(&self) -> &[DtcServiceDefinition] {
            &[]
        }

        fn active_tests(&self) -> &[ActiveTestDefinition] {
            &[]
        }

        fn passive_monitors(&self) -> &[PassiveMonitorDefinition] {
            &[]
        }

        fn decode_signal(
            &self,
            _signal: &SignalDefinition,
            _payload: &[u8],
        ) -> Result<DecodedSignal, ProfileDecodeError> {
            Err(ProfileDecodeError::Decode("test".into()))
        }

        fn decode_dtc_response(
            &self,
            _service: &DtcServiceDefinition,
            _payload: &[u8],
        ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
            Err(ProfileDecodeError::Decode("test".into()))
        }
    }

    fn _assert_object_safe(_: &dyn DiagnosticProfile) {}

    #[test]
    fn id_is_copy_eq_hash() {
        let id = ProfileId::new("gm.gmt800.lly.class2");
        let same = id;
        let mut set = HashSet::new();

        set.insert(id);
        set.insert(same);

        assert_eq!(set.len(), 1);
        assert_eq!(id, same);
        assert_eq!(id.as_str(), "gm.gmt800.lly.class2");
    }

    #[test]
    fn manufacturer_is_exhaustive_and_has_generic() {
        fn label(manufacturer: Manufacturer) -> &'static str {
            match manufacturer {
                Manufacturer::Gm => "gm",
                Manufacturer::Ford => "ford",
                Manufacturer::ChryslerRam => "chrysler_ram",
                Manufacturer::Fixture => "fixture",
                Manufacturer::Generic => "generic",
            }
        }

        assert_eq!(label(Manufacturer::Fixture), "fixture");
        assert_eq!(label(Manufacturer::Generic), "generic");
    }

    #[test]
    fn match_confidence_is_exhaustive() {
        fn rank(confidence: MatchConfidence) -> u8 {
            match confidence {
                MatchConfidence::ProtocolPlusVinDimension => 1,
                MatchConfidence::VinExact => 2,
                MatchConfidence::VinPlusSpec => 3,
            }
        }

        assert_eq!(rank(MatchConfidence::VinPlusSpec), 3);
    }

    #[test]
    fn identity_confidence_is_trusted() {
        fn trusted(confidence: IdentityConfidence) -> bool {
            match confidence {
                IdentityConfidence::Unread => false,
                IdentityConfidence::Corrupted => false,
                IdentityConfidence::Single => false,
                IdentityConfidence::Confirmed => true,
            }
        }

        assert!(!IdentityConfidence::Unread.is_trusted());
        assert!(!IdentityConfidence::Corrupted.is_trusted());
        assert!(!IdentityConfidence::Single.is_trusted());
        assert!(IdentityConfidence::Confirmed.is_trusted());
        assert!(trusted(IdentityConfidence::Confirmed));
    }

    #[test]
    fn evidence_policy_is_exhaustive_with_bounded_live() {
        fn code(policy: EvidencePolicy) -> u8 {
            match policy {
                EvidencePolicy::None => 0,
                EvidencePolicy::OnError => 1,
                EvidencePolicy::OnDemand => 2,
                EvidencePolicy::BoundedLive => 3,
                EvidencePolicy::Always => 4,
            }
        }

        assert_eq!(code(EvidencePolicy::BoundedLive), 3);
    }

    #[test]
    fn selected_profile_round_trips_via_seal() {
        let id = ProfileId::new("gm.gmt800.lly.class2");
        let selected = SelectedProfile::seal(id, 7, false);

        assert_eq!(selected.profile_id(), id);
        assert_eq!(selected.context_generation(), 7);
        assert!(!selected.manual_confirmed());
    }

    #[test]
    fn selected_profile_manual_confirm_is_carried() {
        let selected = SelectedProfile::seal(ProfileId::new("manual"), 7, true);

        assert!(selected.manual_confirmed());
    }

    #[test]
    fn selected_profile_generation_distinguishes_tokens() {
        let id = ProfileId::new("gm.gmt800.lly.class2");
        let generation_7 = SelectedProfile::seal(id, 7, false);
        let generation_8 = SelectedProfile::seal(id, 8, false);

        assert_ne!(generation_7, generation_8);
        assert!(generation_7.is_valid_for(7));
        assert!(!generation_7.is_valid_for(8));
    }

    #[test]
    fn profile_match_partial_carries_reason() {
        let profile_match = ProfileMatch::Partial {
            reason: "vin 8th digit mismatch".into(),
        };

        let reason = match profile_match {
            ProfileMatch::Exact { .. } => panic!("expected partial"),
            ProfileMatch::Partial { reason } => reason,
            ProfileMatch::NoMatch => panic!("expected partial"),
        };

        assert_eq!(reason, "vin 8th digit mismatch");
    }

    #[test]
    fn address_template_variants_are_exhaustive() {
        fn kind(template: AddressTemplate) -> &'static str {
            match template {
                AddressTemplate::J1850 { .. } => "j1850",
                AddressTemplate::Can11 { .. } => "can11",
                AddressTemplate::Can29 { .. } => "can29",
            }
        }

        assert_eq!(kind(AddressTemplate::J1850 { node: 0x18 }), "j1850");
        assert_eq!(
            kind(AddressTemplate::Can11 {
                request_id: 0x7E0,
                response_id: 0x7E8,
            }),
            "can11"
        );
        assert_eq!(
            kind(AddressTemplate::Can29 {
                request_id: 0x18DA1022,
                response_id: 0x18DA2210,
            }),
            "can29"
        );
    }

    #[test]
    fn route_definition_uses_module_key_not_label() {
        let route = RouteDefinition {
            module: ModuleKey::Tcm,
        };

        assert_eq!(route.module.canonical(), "tcm");
        assert_eq!(route.module.to_core_module_id(), ModuleId::new("tcm"));
    }

    #[test]
    fn route_set_single_explicit_and_discovered() {
        static ROUTES: &[RouteDefinition] = &[RouteDefinition {
            module: ModuleKey::Ecm,
        }];

        let route = RouteDefinition {
            module: ModuleKey::Ecm,
        };

        assert_eq!(RouteSet::single(route).scope, RouteScope::Single(route));
        assert_eq!(
            RouteSet::explicit(ROUTES).scope,
            RouteScope::Explicit(ROUTES)
        );
        assert_eq!(
            RouteSet::discovered_on_bus(BusKey::new("j1850vpw")).scope,
            RouteScope::DiscoveredOnBus {
                bus: BusKey::new("j1850vpw"),
            }
        );
    }

    #[test]
    fn module_map_types_are_static_data() {
        const CLASS2: BusKey = BusKey::new("class2");
        const FIXTURE_CONVENTION: J1850HeaderConvention = J1850HeaderConvention {
            priority: 0x01,
            source: 0x02,
        };
        static BUSES: &[BusDefinition] = &[BusDefinition {
            key: CLASS2,
            family: BusFamily::J1850,
            protocol: Protocol::J1850Vpw,
            j1850: Some(FIXTURE_CONVENTION),
            label: "Fixture J1850",
        }];
        static MODULES: &[ModuleDefinition] = &[ModuleDefinition {
            key: ModuleKey::Tcm,
            display_label: "TCM",
            bus: CLASS2,
            address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x18 }),
            safety_class: ModuleSafetyClass::Powertrain,
            coresident_with: None,
        }];
        static MAP: ModuleMap = ModuleMap {
            buses: BUSES,
            modules: MODULES,
        };

        assert_eq!(MAP.buses[0].key.as_str(), "class2");
        assert_eq!(MAP.modules[0].key, ModuleKey::Tcm);
    }

    #[test]
    fn module_evidence_state_is_exhaustive() {
        fn label(state: ModuleEvidenceState) -> &'static str {
            match state {
                ModuleEvidenceState::Candidate => "candidate",
                ModuleEvidenceState::Unsupported { .. } => "unsupported",
                ModuleEvidenceState::Probed { .. } => "probed",
                ModuleEvidenceState::Confirmed { .. } => "confirmed",
            }
        }

        assert_eq!(label(ModuleEvidenceState::Candidate), "candidate");
        assert_eq!(
            label(ModuleEvidenceState::Unsupported {
                nrc: Some(0x11),
                evidence_id: "cap",
            }),
            "unsupported"
        );
        assert_eq!(
            label(ModuleEvidenceState::Probed { evidence_id: "cap" }),
            "probed"
        );
        assert_eq!(
            label(ModuleEvidenceState::Confirmed { evidence_id: "cap" }),
            "confirmed"
        );
    }

    #[test]
    fn passive_capability_state_is_separate_from_active_evidence() {
        let state = PassiveCapabilityState::Observed {
            frames_ref: EvidenceRef::new("bus-watch"),
            lines: 12,
            last_offset_ms: 350,
        };

        match state {
            PassiveCapabilityState::NotSeen => panic!("expected observed"),
            PassiveCapabilityState::Observed {
                frames_ref,
                lines,
                last_offset_ms,
            } => {
                assert_eq!(frames_ref.as_str(), "bus-watch");
                assert_eq!(lines, 12);
                assert_eq!(last_offset_ms, 350);
            }
        }
    }

    #[test]
    fn source_fields_preserve_strings() {
        let fields = SourceFields {
            txd: "TXD123",
            rxf: Some("044105"),
            rxd: Some(RxdSource {
                raw: "3008",
                bit_width: Some(16),
            }),
            raw_mth: Some("000100010000"),
            source_ref: Some("scangauge"),
        };

        let rxd = fields.rxd.unwrap();
        assert_eq!(fields.txd, "TXD123");
        assert_eq!(rxd.raw, "3008");
        assert_eq!(rxd.bit_width, Some(16));
        assert_eq!(fields.raw_mth, Some("000100010000"));
        assert_eq!(SourceFields::NONE, SourceFields::default());
    }

    #[test]
    fn signal_definition_is_pure_static_data() {
        const SIGNAL: SignalDefinition = SignalDefinition {
            key: "gm.lly.fuel_rail_pressure",
            label: "Fuel rail pressure",
            category: SignalCategory::Fuel,
            route: RouteDefinition {
                module: ModuleKey::Ecm,
            },
            service_id: 0x22,
            request_data: &[0x15, 0x42, 0x01],
            decoder_id: "gm.lly.test",
            unit: "psi",
            cadence: PollCadence::Medium,
            confidence: Confidence::Candidate,
            provenance: &[Provenance::ScanGaugePublished],
            source_fields: SourceFields {
                txd: "22154201",
                rxf: None,
                rxd: None,
                raw_mth: None,
                source_ref: None,
            },
            evidence_policy: EvidencePolicy::OnDemand,
            failure_policy: FailurePolicy::CandidateOnly,
            preferred_over: Some("sae.fuel_rail_pressure"),
        };

        assert_eq!(SIGNAL.service_id, 0x22);
        assert_eq!(SIGNAL.request_data, &[0x15, 0x42, 0x01]);
    }

    #[test]
    fn signal_display_definition_is_pure_static_data() {
        const INPUTS: &[&str] = &["signal.actual", "signal.desired"];
        const DISPLAY: &[SignalDisplayDefinition] = &[
            SignalDisplayDefinition {
                key: "signal.actual",
                label: "Actual",
                category: SignalCategory::Fuel,
                unit: "psi",
                source: SignalDisplaySource::StandardPid(0x23),
                composition: SignalComposition::Pair {
                    group_key: "signal.group",
                    role: PairRole::Actual,
                },
            },
            SignalDisplayDefinition {
                key: "signal.delta",
                label: "Delta",
                category: SignalCategory::Fuel,
                unit: "psi",
                source: SignalDisplaySource::Derived {
                    formula_key: "subtract",
                    input_keys: INPUTS,
                },
                composition: SignalComposition::Pair {
                    group_key: "signal.group",
                    role: PairRole::Delta,
                },
            },
            SignalDisplayDefinition {
                key: "signal.row.1",
                label: "Row",
                category: SignalCategory::Fuel,
                unit: "mm3",
                source: SignalDisplaySource::ProfileSignal("signal.row.1"),
                composition: SignalComposition::TableRow {
                    table_key: "signal.table",
                    row_index: 0,
                    row_label: "1",
                },
            },
        ];

        assert_eq!(DISPLAY[0].source, SignalDisplaySource::StandardPid(0x23));
        assert_eq!(
            DISPLAY[1].source,
            SignalDisplaySource::Derived {
                formula_key: "subtract",
                input_keys: INPUTS,
            }
        );
        assert_eq!(
            DISPLAY[2].composition,
            SignalComposition::TableRow {
                table_key: "signal.table",
                row_index: 0,
                row_label: "1",
            }
        );
    }

    #[test]
    fn diagnostic_profile_default_signal_display_is_empty() {
        let profile = ObjectSafeProfile;

        assert!(profile.signal_display().is_empty());
    }

    #[test]
    fn decoded_signal_preserves_raw_and_carries_module() {
        let decoded = DecodedSignal {
            key: "signal",
            value: 12.0,
            unit: "psi",
            raw: vec![0x62, 0x15, 0x42, 0x0A],
            selected_raw: vec![0x0A],
            module: ModuleId::new("ecm"),
            confidence: Confidence::Verified,
        };

        assert_eq!(decoded.raw, vec![0x62, 0x15, 0x42, 0x0A]);
        assert_eq!(decoded.selected_raw, vec![0x0A]);
        assert_eq!(decoded.module, ModuleId::new("ecm"));
        assert_eq!(decoded.confidence, Confidence::Verified);
    }

    #[test]
    fn profile_decode_error_is_exhaustive() {
        fn code(error: ProfileDecodeError) -> u8 {
            match error {
                ProfileDecodeError::PayloadTooShort { expected, got } => {
                    assert!(expected > got);
                    0
                }
                ProfileDecodeError::MismatchedResponse => 1,
                ProfileDecodeError::UnknownDecoder("x") => 2,
                ProfileDecodeError::UnknownDecoder(_) => 3,
                ProfileDecodeError::NegativeResponse { service, nrc } => {
                    assert_eq!(service, 0x19);
                    assert_eq!(nrc, 0x11);
                    4
                }
                ProfileDecodeError::Decode(message) => {
                    assert_eq!(message, "bad");
                    5
                }
                ProfileDecodeError::Other(message) => {
                    assert_eq!(message, "other");
                    6
                }
            }
        }

        assert_eq!(
            code(ProfileDecodeError::PayloadTooShort {
                expected: 4,
                got: 2,
            }),
            0
        );
        assert_eq!(code(ProfileDecodeError::MismatchedResponse), 1);
        assert_eq!(code(ProfileDecodeError::UnknownDecoder("x")), 2);
        assert_eq!(
            code(ProfileDecodeError::NegativeResponse {
                service: 0x19,
                nrc: 0x11,
            }),
            4
        );
        assert_eq!(code(ProfileDecodeError::Decode("bad".into())), 5);
        assert_eq!(code(ProfileDecodeError::Other("other".into())), 6);
    }

    #[test]
    fn diagnostic_profile_is_object_safe() {
        let profile = ObjectSafeProfile;
        let boxed: Box<dyn DiagnosticProfile> = Box::new(ObjectSafeProfile);

        _assert_object_safe(&profile);
        _assert_object_safe(boxed.as_ref());
    }
}
