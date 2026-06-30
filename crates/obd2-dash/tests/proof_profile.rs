#![cfg(feature = "proof-profile")]

mod corpus_support;

use std::collections::HashSet;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use async_trait::async_trait;
use corpus_support::{corpus_dir, hex_to_bytes, load_jsonl, SignalGolden};
use obd2_core::adapter::{
    Adapter, AdapterInfo, Capabilities, Chipset, InitializationReport, PhysicalTarget,
    RoutedRequest,
};
use obd2_core::error::Obd2Error;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::ServiceRequest;
use obd2_core::session::Session;
use obd2_core::vehicle::{ModuleId, Protocol};
use obd2_dash::profiles::fixture::{
    profile_tabs, FIXTURE_PROFILE, FIXTURE_PROFILE_ID, FIXTURE_SIGNAL_KEY, FIXTURE_VIN,
};
use obd2_dash::profiles::gm::GM_LLY_CLASS2_PROFILE;
use obd2_dash::profiles::{
    build_vehicle_context, CapabilityId, DiagnosticProfile, DispatchError, DispatchEvidence,
    IdentityConfidence, IdentityOutcome, ModuleKey, NullEvidenceSink, ProfileDecodeError,
    ProfileEvidenceRecord, ProfileEvidenceSink, ProfileRegistry, ProfileResponse, ProfileRuntime,
    RequestId, SignalCategory, VehicleContext,
};

#[derive(Clone)]
struct CountingAdapter {
    writes: Arc<AtomicUsize>,
    response: Arc<Mutex<Vec<u8>>>,
    last_request: Arc<Mutex<Option<RoutedRequest>>>,
}

impl CountingAdapter {
    fn new(response: Vec<u8>) -> Self {
        Self {
            writes: Arc::new(AtomicUsize::new(0)),
            response: Arc::new(Mutex::new(response)),
            last_request: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl Adapter for CountingAdapter {
    async fn initialize(&mut self) -> Result<InitializationReport, Obd2Error> {
        Ok(InitializationReport {
            info: self.info().clone(),
            probe_attempts: Vec::new(),
            events: Vec::new(),
        })
    }

    async fn request(&mut self, _req: &ServiceRequest) -> Result<Vec<u8>, Obd2Error> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    async fn routed_request(&mut self, req: &RoutedRequest) -> Result<Vec<u8>, Obd2Error> {
        self.writes.fetch_add(1, Ordering::SeqCst);
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
            firmware: "fixture".into(),
            protocol: Protocol::Can11Bit500,
            capabilities: Capabilities::default(),
        })
    }
}

#[derive(Default)]
struct CollectEvidence {
    records: Vec<ProfileEvidenceRecord>,
}

impl ProfileEvidenceSink for CollectEvidence {
    fn record(&mut self, evidence: &DispatchEvidence<'_>) {
        self.records.push(ProfileEvidenceRecord::from_dispatch(
            evidence,
            Some("confirmed"),
            false,
        ));
    }
}

#[tokio::test]
async fn dispatch_executes_fixture_signal_end_to_end() {
    let registry = ProfileRegistry::with_builtins();
    let selected = registry
        .select(&fixture_context(7, IdentityConfidence::Confirmed))
        .selected
        .expect("fixture should select");
    let runtime = ProfileRuntime::new(&registry);
    let adapter = CountingAdapter::new(vec![0x13, 0x88]);
    let writes = Arc::clone(&adapter.writes);
    let last_request = Arc::clone(&adapter.last_request);
    let mut session = Session::new(adapter);
    let mut evidence = CollectEvidence::default();

    let response = runtime
        .execute_request(
            &mut session,
            &fixture_context(7, IdentityConfidence::Confirmed),
            &selected,
            CapabilityId::Signal(FIXTURE_SIGNAL_KEY),
            RequestId::SINGLE,
            &mut evidence,
        )
        .await
        .unwrap();

    match response {
        ProfileResponse::Signal(signal) => {
            assert_eq!(signal.key, FIXTURE_SIGNAL_KEY);
            assert_eq!(signal.value, 50.0);
            assert_eq!(signal.unit, "C");
        }
        other => panic!("unexpected response: {other:?}"),
    }
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    let request = last_request.lock().unwrap().clone().unwrap();
    assert_eq!(request.service_id, 0x22);
    assert_eq!(request.data, vec![0xF0, 0x01]);
    assert!(matches!(
        request.target,
        PhysicalTarget::Addressed(obd2_core::vehicle::PhysicalAddress::Can11Bit {
            request_id: 0x7E0,
            response_id: 0x7E8
        })
    ));
    assert_eq!(evidence.records.len(), 1);
    assert_eq!(evidence.records[0].profile_id, FIXTURE_PROFILE_ID.as_str());
    assert_eq!(evidence.records[0].capability_id, FIXTURE_SIGNAL_KEY);
    assert_eq!(evidence.records[0].parsed_response_bytes, vec![0x13, 0x88]);
}

#[tokio::test]
async fn dispatch_rejects_capability_not_owned_by_selected_profile() {
    let registry = ProfileRegistry::with_builtins();
    let selected = registry
        .select(&fixture_context(9, IdentityConfidence::Confirmed))
        .selected
        .unwrap();
    let runtime = ProfileRuntime::new(&registry);
    let adapter = CountingAdapter::new(vec![0x13, 0x88]);
    let writes = Arc::clone(&adapter.writes);
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &fixture_context(9, IdentityConfidence::Confirmed),
            &selected,
            CapabilityId::Signal("lly.1540"),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DispatchError::CapabilityNotOwned {
            capability: CapabilityId::Signal("lly.1540"),
            ..
        }
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn partial_match_yields_no_token() {
    let registry = ProfileRegistry::with_builtins();
    let state = registry.select(&fixture_context(10, IdentityConfidence::Corrupted));

    assert!(state.selected.is_none());
    assert!(state
        .partial_matches
        .iter()
        .any(|entry| entry.profile_id == FIXTURE_PROFILE_ID));
}

#[tokio::test]
async fn stale_fixture_token_fails_validation() {
    let registry = ProfileRegistry::with_builtins();
    let selected = registry
        .select(&fixture_context(10, IdentityConfidence::Confirmed))
        .selected
        .unwrap();
    let runtime = ProfileRuntime::new(&registry);
    let adapter = CountingAdapter::new(vec![0x13, 0x88]);
    let writes = Arc::clone(&adapter.writes);
    let mut session = Session::new(adapter);

    let err = runtime
        .execute_request(
            &mut session,
            &fixture_context(11, IdentityConfidence::Confirmed),
            &selected,
            CapabilityId::Signal(FIXTURE_SIGNAL_KEY),
            RequestId::SINGLE,
            &mut NullEvidenceSink,
        )
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        DispatchError::StaleGeneration {
            token: 10,
            current: 11
        }
    ));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn registry_select_fixture_and_lly_are_mutually_exclusive() {
    let registry = ProfileRegistry::with_builtins();
    let fixture = registry.select(&fixture_context(1, IdentityConfidence::Confirmed));
    assert_eq!(
        fixture
            .exact_matches
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        vec![FIXTURE_PROFILE_ID.as_str()]
    );

    let lly = registry.select(&lly_context().await);
    assert_eq!(
        lly.exact_matches
            .iter()
            .map(|id| id.as_str())
            .collect::<Vec<_>>(),
        vec!["gm.gmt800.lly.class2"]
    );
}

#[test]
fn fixture_payload_through_own_decoder_errors_for_foreign_shape() {
    let signal = FIXTURE_PROFILE.signals()[0];
    let err = FIXTURE_PROFILE
        .decode_signal(&signal, &[0x62, 0x15, 0x40, 0xE2])
        .unwrap_err();

    assert_eq!(err, ProfileDecodeError::MismatchedResponse);
}

#[test]
fn fixture_capability_is_not_in_other_profile() {
    assert!(GM_LLY_CLASS2_PROFILE
        .signals()
        .iter()
        .all(|signal| signal.key != FIXTURE_SIGNAL_KEY));
}

#[test]
fn corpus_profile_fixture_can11() {
    let goldens: Vec<SignalGolden> = load_jsonl(
        &corpus_dir()
            .join("profile")
            .join("fixture.can11.readonly.v1"),
        "signal-",
    );
    assert!(!goldens.is_empty());

    for golden in goldens {
        assert_eq!(golden.profile_id, FIXTURE_PROFILE_ID.as_str());
        assert_eq!(golden.signal_key.as_deref(), Some(FIXTURE_SIGNAL_KEY));
        let decoded = FIXTURE_PROFILE
            .decode_signal(
                &FIXTURE_PROFILE.signals()[0],
                &hex_to_bytes(&golden.payload_hex),
            )
            .unwrap();
        assert_eq!(decoded.value.to_bits(), golden.expected.value.to_bits());
        assert_eq!(decoded.unit, golden.expected.unit);
    }
}

#[test]
fn fixture_profile_tabs_data_mapping() {
    assert_eq!(
        profile_tabs(&FIXTURE_PROFILE),
        vec![SignalCategory::Powertrain]
    );
}

#[test]
fn fixture_has_no_active_tests() {
    assert!(FIXTURE_PROFILE.active_tests().is_empty());
}

fn fixture_context(generation: u64, confidence: IdentityConfidence) -> VehicleContext {
    VehicleContext {
        generation,
        protocol: Protocol::Can11Bit500,
        vin: Some(FIXTURE_VIN.to_string()),
        vin_confidence: confidence,
        spec: None,
        discovered_modules: vec![ModuleId::new(ModuleKey::Ecm.canonical())],
        active_bus: Some("can11-500".to_string()),
    }
}

async fn lly_context() -> VehicleContext {
    let adapter = obd2_core::adapter::mock::MockAdapter::with_vin("1GCHK23224F000001");
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();
    session.identify_vehicle().await.unwrap();
    let identity = IdentityOutcome {
        vin: Some("1GCHK23224F000001".into()),
        confidence: IdentityConfidence::Confirmed,
    };
    let mut context = build_vehicle_context(&session, 5, &identity);
    context.protocol = Protocol::J1850Vpw;
    context
}
