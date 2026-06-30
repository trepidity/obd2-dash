use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use obd2_core::adapter::{
    Adapter, AdapterInfo, Capabilities, Chipset, InitializationReport, PhysicalTarget,
    RoutedRequest,
};
use obd2_core::error::Obd2Error;
use obd2_core::protocol::dtc::DtcStatus;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::ServiceRequest;
use obd2_core::session::Session;
use obd2_core::vehicle::{ModuleId, PhysicalAddress, Protocol};
use obd2_dash::profiles::{
    AddressState, AddressTemplate, BackoffPolicy, BusDefinition, BusKey, CapabilityId, Confidence,
    DecodedDtc, DecodedSignal, DiagnosticProfile, DispatchError, DtcServiceDefinition,
    EvidencePolicy, FailurePolicy, IdentityConfidence, J1850HeaderConvention, Manufacturer,
    MatchConfidence, ModuleDefinition, ModuleKey, ModuleMap, ModuleSafetyClass, NullEvidenceSink,
    PassiveMonitorDefinition, PollCadence, ProfileDecodeError, ProfileId, ProfileMatch,
    ProfileRegistry, ProfileResponse, ProfileRuntime, Provenance, RequestId, RouteDefinition,
    RouteSet, SignalCategory, SignalDefinition, SourceFields, StandardPidOverride, VehicleContext,
};

const PROFILE_ID: ProfileId = ProfileId::new("test.fixture.dispatcher");
const PARTIAL_PROFILE_ID: ProfileId = ProfileId::new("test.fixture.partial");
const J1850_BUS: BusKey = BusKey::new("j1850vpw");
const FIXTURE_VIN: &str = "1GTHK29294E391526";

static FIXTURE_PROFILE: FixtureProfile = FixtureProfile {
    id: PROFILE_ID,
    partial: false,
};
static PARTIAL_PROFILE: FixtureProfile = FixtureProfile {
    id: PARTIAL_PROFILE_ID,
    partial: true,
};

const BUSES: &[BusDefinition] = &[BusDefinition {
    key: J1850_BUS,
    family: obd2_core::protocol::codec::BusFamily::J1850,
    protocol: Protocol::J1850Vpw,
    j1850: Some(J1850HeaderConvention {
        priority: 0x6C,
        source: 0xF1,
    }),
    label: "fixture J1850",
}];

const MODULES: &[ModuleDefinition] = &[
    ModuleDefinition {
        key: ModuleKey::Ecm,
        display_label: "ECM",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x10 }),
        safety_class: ModuleSafetyClass::Powertrain,
        coresident_with: None,
    },
    ModuleDefinition {
        key: ModuleKey::Tcm,
        display_label: "TCM",
        bus: J1850_BUS,
        address: AddressState::Confirmed(AddressTemplate::J1850 { node: 0x18 }),
        safety_class: ModuleSafetyClass::Powertrain,
        coresident_with: None,
    },
];

const MODULE_MAP: ModuleMap = ModuleMap {
    buses: BUSES,
    modules: MODULES,
};

const FIXTURE_SIGNAL: SignalDefinition = SignalDefinition {
    key: "fix_signal",
    label: "Fixture Signal",
    category: SignalCategory::Other,
    route: RouteDefinition {
        module: ModuleKey::Tcm,
    },
    service_id: 0x22,
    request_data: &[0x15, 0x40],
    decoder_id: "fix.signal",
    unit: "count",
    cadence: PollCadence::OnDemand,
    confidence: Confidence::Verified,
    provenance: &[Provenance::LocalFixture],
    source_fields: SourceFields::NONE,
    evidence_policy: EvidencePolicy::None,
    failure_policy: FailurePolicy::SurfaceUnavailable,
    preferred_over: None,
};

const FIXTURE_DTC: DtcServiceDefinition = DtcServiceDefinition {
    key: "fix.dtc",
    label: "Fixture DTC",
    route_set: RouteSet::discovered_on_bus(J1850_BUS),
    service_id: 0x19,
    request_data: &[0xFF, 0xFF, 0x00],
    decoder_id: "fix.dtc",
    backoff_policy: BackoffPolicy::NONE,
    cadence: PollCadence::Medium,
};

const SIGNALS: &[SignalDefinition] = &[FIXTURE_SIGNAL];
const DTC_SERVICES: &[DtcServiceDefinition] = &[FIXTURE_DTC];

struct FixtureProfile {
    id: ProfileId,
    partial: bool,
}

impl DiagnosticProfile for FixtureProfile {
    fn id(&self) -> ProfileId {
        self.id
    }

    fn manufacturer(&self) -> Manufacturer {
        Manufacturer::Generic
    }

    fn allowed_protocols(&self) -> &'static [Protocol] {
        &[Protocol::J1850Vpw]
    }

    fn module_map(&self) -> Option<&ModuleMap> {
        Some(&MODULE_MAP)
    }

    fn matches(&self, _ctx: &VehicleContext) -> ProfileMatch {
        if self.partial {
            ProfileMatch::Partial {
                reason: "fixture partial".into(),
            }
        } else {
            ProfileMatch::Exact {
                confidence: MatchConfidence::VinPlusSpec,
            }
        }
    }

    fn standard_pid_overrides(&self) -> &[StandardPidOverride] {
        &[]
    }

    fn signals(&self) -> &[SignalDefinition] {
        SIGNALS
    }

    fn dtc_services(&self) -> &[DtcServiceDefinition] {
        DTC_SERVICES
    }

    fn active_tests(&self) -> &[obd2_dash::profiles::ActiveTestDefinition] {
        &[]
    }

    fn passive_monitors(&self) -> &[PassiveMonitorDefinition] {
        &[]
    }

    fn decode_signal(
        &self,
        signal: &SignalDefinition,
        payload: &[u8],
    ) -> Result<DecodedSignal, ProfileDecodeError> {
        if signal.decoder_id != "fix.signal" {
            return Err(ProfileDecodeError::UnknownDecoder(signal.decoder_id));
        }
        Ok(DecodedSignal {
            key: signal.key,
            value: payload.first().copied().unwrap_or_default() as f64,
            unit: signal.unit,
            raw: payload.to_vec(),
            selected_raw: payload.first().copied().into_iter().collect(),
            module: ModuleId::new("tcm"),
            confidence: Confidence::Verified,
        })
    }

    fn decode_dtc_response(
        &self,
        service: &DtcServiceDefinition,
        payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
        if service.decoder_id != "fix.dtc" {
            return Err(ProfileDecodeError::UnknownDecoder(service.decoder_id));
        }
        Ok(vec![DecodedDtc {
            code: "P1234".into(),
            status: DtcStatus::Stored,
            status_raw: payload.first().copied(),
            status_flags: vec!["fixture".into()],
            raw: payload.to_vec(),
            module: None,
            notes: None,
        }])
    }
}

#[derive(Clone)]
struct CountingAdapter {
    writes: Arc<AtomicUsize>,
    response: Arc<Mutex<Vec<u8>>>,
    last_request: Arc<Mutex<Option<RoutedRequest>>>,
    protocol: Protocol,
}

impl CountingAdapter {
    fn new(protocol: Protocol, response: Vec<u8>) -> Self {
        Self {
            writes: Arc::new(AtomicUsize::new(0)),
            response: Arc::new(Mutex::new(response)),
            last_request: Arc::new(Mutex::new(None)),
            protocol,
        }
    }

    fn writes(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.writes)
    }

    fn last_request(&self) -> Arc<Mutex<Option<RoutedRequest>>> {
        Arc::clone(&self.last_request)
    }
}

#[async_trait]
impl Adapter for CountingAdapter {
    async fn initialize(&mut self) -> Result<InitializationReport, Obd2Error> {
        Ok(InitializationReport {
            info: AdapterInfo {
                chipset: Chipset::Elm327Genuine,
                firmware: "test".into(),
                protocol: self.protocol,
                capabilities: Capabilities::default(),
            },
            probe_attempts: Vec::new(),
            events: Vec::new(),
        })
    }

    async fn request(&mut self, req: &ServiceRequest) -> Result<Vec<u8>, Obd2Error> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if req.service_id == 0x09 && req.data == [0x02] {
            Ok(FIXTURE_VIN.as_bytes().to_vec())
        } else {
            Ok(Vec::new())
        }
    }

    async fn routed_request(&mut self, req: &RoutedRequest) -> Result<Vec<u8>, Obd2Error> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if matches!(req.target, PhysicalTarget::Broadcast)
            && req.service_id == 0x09
            && req.data == [0x02]
        {
            return Ok(FIXTURE_VIN.as_bytes().to_vec());
        }
        *self.last_request.lock().unwrap() = Some(req.clone());
        Ok(self.response.lock().unwrap().clone())
    }

    async fn supported_pids(&mut self) -> Result<HashSet<Pid>, Obd2Error> {
        Ok(HashSet::new())
    }

    async fn battery_voltage(&mut self) -> Result<Option<f64>, Obd2Error> {
        Ok(Some(12.6))
    }

    fn info(&self) -> &AdapterInfo {
        static INFO: std::sync::OnceLock<AdapterInfo> = std::sync::OnceLock::new();
        INFO.get_or_init(|| AdapterInfo {
            chipset: Chipset::Elm327Genuine,
            firmware: "test".into(),
            protocol: Protocol::J1850Vpw,
            capabilities: Capabilities::default(),
        })
    }
}

fn registry() -> ProfileRegistry {
    let mut registry = ProfileRegistry::new();
    registry.register(&FIXTURE_PROFILE);
    registry
}

fn context(generation: u64, protocol: Protocol) -> VehicleContext {
    VehicleContext {
        generation,
        protocol,
        vin: Some(FIXTURE_VIN.into()),
        vin_confidence: IdentityConfidence::Confirmed,
        spec: None,
        discovered_modules: vec![ModuleId::new("tcm")],
        active_bus: Some(J1850_BUS.as_str().into()),
    }
}

async fn selected_fixture(
    registry: &ProfileRegistry,
    generation: u64,
) -> obd2_dash::profiles::SelectedProfile {
    registry
        .confirm_manual(&context(generation, Protocol::J1850Vpw), PROFILE_ID)
        .unwrap()
}

#[tokio::test]
async fn execute_signal_reads_and_decodes() {
    let registry = registry();
    let runtime = ProfileRuntime::new(&registry);
    let selected = selected_fixture(&registry, 7).await;
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A, 0x00]);
    let writes = adapter.writes();
    let last_request = adapter.last_request();
    let mut session = Session::new(adapter);
    session.identify_vehicle().await.unwrap();

    let response = runtime
        .execute_request(
            &mut session,
            &context(7, Protocol::J1850Vpw),
            &selected,
            CapabilityId::Signal("fix_signal"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap();

    assert!(writes.load(Ordering::SeqCst) >= 2);
    match response {
        ProfileResponse::Signal(signal) => {
            assert_eq!(signal.key, "fix_signal");
            assert_eq!(signal.value, 42.0);
            assert_eq!(signal.module, ModuleId::new("tcm"));
        }
        other => panic!("unexpected response: {other:?}"),
    }

    let request = last_request.lock().unwrap().clone().unwrap();
    assert_eq!(request.service_id, 0x22);
    assert_eq!(request.data, vec![0x15, 0x40]);
    assert!(matches!(
        request.target,
        PhysicalTarget::Addressed(PhysicalAddress::J1850 {
            node: 0x18,
            header: [0x6C, 0x18, 0xF1],
        })
    ));
}

#[tokio::test]
async fn dtc_service_dispatches_per_module() {
    let registry = registry();
    let runtime = ProfileRuntime::new(&registry);
    let selected = selected_fixture(&registry, 7).await;
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x93]);
    let last_request = adapter.last_request();
    let mut session = Session::new(adapter);
    session.identify_vehicle().await.unwrap();

    let response = runtime
        .execute_request(
            &mut session,
            &context(7, Protocol::J1850Vpw),
            &selected,
            CapabilityId::DtcService("fix.dtc"),
            RequestId(0),
            &mut NullEvidenceSink,
        )
        .await
        .unwrap();

    match response {
        ProfileResponse::Dtcs(dtcs) => {
            assert_eq!(dtcs.len(), 1);
            assert_eq!(dtcs[0].code, "P1234");
            assert_eq!(dtcs[0].module, Some(ModuleId::new("tcm")));
        }
        other => panic!("unexpected response: {other:?}"),
    }

    let request = last_request.lock().unwrap().clone().unwrap();
    assert_eq!(request.service_id, 0x19);
    assert_eq!(request.data, vec![0xFF, 0xFF, 0x00]);
}

#[tokio::test]
async fn stale_generation_rejected_before_adapter_write() {
    let registry = registry();
    let runtime = ProfileRuntime::new(&registry);
    let selected = selected_fixture(&registry, 1).await;
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
    let writes = adapter.writes();
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &context(2, Protocol::J1850Vpw),
            &selected,
            CapabilityId::Signal("fix_signal"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DispatchError::StaleGeneration {
            token: 1,
            current: 2
        }
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_profile_rejected_before_adapter_write() {
    let populated = registry();
    let selected = selected_fixture(&populated, 1).await;
    let empty = ProfileRegistry::new();
    let runtime = ProfileRuntime::new(&empty);
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
    let writes = adapter.writes();
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &context(1, Protocol::J1850Vpw),
            &selected,
            CapabilityId::Signal("fix_signal"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, DispatchError::UnknownProfile(id) if id == PROFILE_ID));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn capability_not_owned_rejected_before_adapter_write() {
    let registry = registry();
    let runtime = ProfileRuntime::new(&registry);
    let selected = selected_fixture(&registry, 1).await;
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
    let writes = adapter.writes();
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &context(1, Protocol::J1850Vpw),
            &selected,
            CapabilityId::Signal("missing"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DispatchError::CapabilityNotOwned {
            profile: PROFILE_ID,
            capability: CapabilityId::Signal("missing")
        }
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn route_not_owned_rejected_before_adapter_write() {
    let registry = registry();
    let runtime = ProfileRuntime::new(&registry);
    let selected = selected_fixture(&registry, 1).await;
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
    let writes = adapter.writes();
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &context(1, Protocol::J1850Vpw),
            &selected,
            CapabilityId::DtcService("fix.dtc"),
            RequestId(99),
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DispatchError::RouteNotOwnedByCapability {
            capability: CapabilityId::DtcService("fix.dtc"),
            request: RequestId(99)
        }
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn protocol_family_mismatch_rejected_before_adapter_write() {
    for protocol in [Protocol::Can11Bit500, Protocol::Auto] {
        let registry = registry();
        let runtime = ProfileRuntime::new(&registry);
        let selected = selected_fixture(&registry, 1).await;
        let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
        let writes = adapter.writes();
        let mut session = Session::new(adapter);

        let err = runtime
            .execute_request(
                &mut session,
                &context(1, protocol),
                &selected,
                CapabilityId::Signal("fix_signal"),
                RequestId::SINGLE,
                &mut NullEvidenceSink,
            )
            .await
            .unwrap_err();

        assert!(matches!(err, DispatchError::ProtocolFamilyMismatch { .. }));
        assert_eq!(writes.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn active_test_capability_is_locked_before_adapter_write() {
    let registry = registry();
    let runtime = ProfileRuntime::new(&registry);
    let selected = selected_fixture(&registry, 1).await;
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
    let writes = adapter.writes();
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &context(1, Protocol::J1850Vpw),
            &selected,
            CapabilityId::ActiveTest("fixture.active"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DispatchError::ActiveTestLocked {
            capability: CapabilityId::ActiveTest("fixture.active")
        }
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn partial_match_cannot_dispatch() {
    let mut registry = ProfileRegistry::new();
    registry.register(&PARTIAL_PROFILE);
    let state = registry.select(&context(1, Protocol::J1850Vpw));
    assert!(state.selected.is_none());
    assert_eq!(state.partial_matches.len(), 1);

    let foreign = registry
        .confirm_manual(&context(1, Protocol::J1850Vpw), PARTIAL_PROFILE_ID)
        .unwrap();
    let empty = ProfileRegistry::new();
    let runtime = ProfileRuntime::new(&empty);
    let adapter = CountingAdapter::new(Protocol::J1850Vpw, vec![0x2A]);
    let writes = adapter.writes();
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &context(1, Protocol::J1850Vpw),
            &foreign,
            CapabilityId::Signal("fix_signal"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(err, DispatchError::UnknownProfile(id) if id == PARTIAL_PROFILE_ID));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}
