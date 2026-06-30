use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use obd2_core::adapter::mock::MockAdapter;
use obd2_core::adapter::{Adapter, AdapterInfo, Capabilities, Chipset, InitializationReport};
use obd2_core::error::Obd2Error;
use obd2_core::protocol::pid::Pid;
use obd2_core::protocol::service::ServiceRequest;
use obd2_core::session::Session;
use obd2_core::vehicle::{ModuleId, Protocol, VehicleSpec};
use obd2_dash::gm_enhanced::lly_profile_matches;
use obd2_dash::profiles::{
    acquire_identity, build_vehicle_context, validate_vin_charset, ActiveTestDefinition,
    DecodedDtc, DecodedSignal, DiagnosticProfile, DtcServiceDefinition, IdentityConfidence,
    Manufacturer, MatchConfidence, PassiveMonitorDefinition, ProfileDecodeError, ProfileId,
    ProfileMatch, ProfileRegistry, SignalDefinition, StandardPidOverride, VehicleContext,
};

#[derive(Debug)]
struct CountingVinAdapter {
    info: AdapterInfo,
    vin: String,
    vin_requests: Arc<AtomicUsize>,
}

impl CountingVinAdapter {
    fn new(vin: &str, vin_requests: Arc<AtomicUsize>) -> Self {
        Self {
            info: AdapterInfo {
                chipset: Chipset::Elm327Genuine,
                firmware: "counting-test".into(),
                protocol: Protocol::J1850Vpw,
                capabilities: Capabilities::default(),
            },
            vin: vin.into(),
            vin_requests,
        }
    }
}

#[async_trait]
impl Adapter for CountingVinAdapter {
    async fn initialize(&mut self) -> Result<InitializationReport, Obd2Error> {
        Ok(InitializationReport {
            info: self.info.clone(),
            probe_attempts: Vec::new(),
            events: Vec::new(),
        })
    }

    async fn request(&mut self, req: &ServiceRequest) -> Result<Vec<u8>, Obd2Error> {
        match (req.service_id, req.data.first()) {
            (0x09, Some(0x02)) => {
                self.vin_requests.fetch_add(1, Ordering::SeqCst);
                Ok(self.vin.as_bytes().to_vec())
            }
            _ => Err(Obd2Error::NoData),
        }
    }

    async fn supported_pids(&mut self) -> Result<HashSet<Pid>, Obd2Error> {
        Ok(HashSet::new())
    }

    async fn battery_voltage(&mut self) -> Result<Option<f64>, Obd2Error> {
        Ok(None)
    }

    fn info(&self) -> &AdapterInfo {
        &self.info
    }
}

#[derive(Clone, Copy)]
enum FixtureBehavior {
    Exact,
    Partial,
    None,
}

struct FixtureProfile {
    id: ProfileId,
    behavior: FixtureBehavior,
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

    fn matches(&self, _ctx: &VehicleContext) -> ProfileMatch {
        match self.behavior {
            FixtureBehavior::Exact => ProfileMatch::Exact {
                confidence: MatchConfidence::VinExact,
            },
            FixtureBehavior::Partial => ProfileMatch::Partial {
                reason: "fixture partial".into(),
            },
            FixtureBehavior::None => ProfileMatch::NoMatch,
        }
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
        Err(ProfileDecodeError::Other("fixture has no decoder".into()))
    }

    fn decode_dtc_response(
        &self,
        _service: &DtcServiceDefinition,
        _payload: &[u8],
    ) -> Result<Vec<DecodedDtc>, ProfileDecodeError> {
        Err(ProfileDecodeError::Other("fixture has no decoder".into()))
    }
}

static PARTIAL_PROFILE: FixtureProfile = FixtureProfile {
    id: ProfileId::new("test.partial"),
    behavior: FixtureBehavior::Partial,
};
static NO_MATCH_PROFILE: FixtureProfile = FixtureProfile {
    id: ProfileId::new("test.none"),
    behavior: FixtureBehavior::None,
};
static EXACT_A: FixtureProfile = FixtureProfile {
    id: ProfileId::new("test.exact.a"),
    behavior: FixtureBehavior::Exact,
};
static EXACT_B: FixtureProfile = FixtureProfile {
    id: ProfileId::new("test.exact.b"),
    behavior: FixtureBehavior::Exact,
};

fn test_context(protocol: Protocol, confidence: IdentityConfidence) -> VehicleContext {
    VehicleContext {
        generation: 7,
        protocol,
        vin: Some("1GCHK23224F000001".into()),
        vin_confidence: confidence,
        spec: None,
        discovered_modules: vec![ModuleId::new("ecm")],
        active_bus: Some("j1850vpw".into()),
    }
}

async fn loaded_lly_context(confidence: IdentityConfidence) -> VehicleContext {
    let adapter = MockAdapter::with_vin("1GCHK23224F000001");
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();
    session.identify_vehicle().await.unwrap();
    let identity = obd2_dash::profiles::IdentityOutcome {
        vin: Some("1GCHK23224F000001".into()),
        confidence,
    };
    let mut context = build_vehicle_context(&session, 11, &identity);
    context.protocol = Protocol::J1850Vpw;
    context
}

#[test]
fn vin_charset_rejects_illegal_letters() {
    assert!(validate_vin_charset("1GTHK29294E391526"));
    assert!(!validate_vin_charset("1GTHK29294E39152I"));
    assert!(!validate_vin_charset("1GTHK29294E39152O"));
    assert!(!validate_vin_charset("1GTHK29294E39152Q"));
    assert!(!validate_vin_charset("1GTHK29294E39152"));
    assert!(!validate_vin_charset("1GTHK29294E39152é"));
}

#[tokio::test]
async fn identity_confidence_confirmed_on_agreement() {
    let count = Arc::new(AtomicUsize::new(0));
    let adapter = CountingVinAdapter::new("1GTHK29294E391526", Arc::clone(&count));
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();

    let outcome = acquire_identity(&mut session, 1).await;

    assert_eq!(outcome.confidence, IdentityConfidence::Confirmed);
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn default_build_issues_no_extra_vin_reads() {
    let count = Arc::new(AtomicUsize::new(0));
    let adapter = CountingVinAdapter::new("1GTHK29294E391526", Arc::clone(&count));
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();

    let outcome = acquire_identity(&mut session, 0).await;

    assert_eq!(outcome.confidence, IdentityConfidence::Single);
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn identity_confidence_corrupted_on_bad_charset() {
    let count = Arc::new(AtomicUsize::new(0));
    let adapter = CountingVinAdapter::new("1GTHK2I294E391526", Arc::clone(&count));
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();

    let outcome = acquire_identity(&mut session, 0).await;

    assert_eq!(outcome.confidence, IdentityConfidence::Corrupted);
}

#[tokio::test]
async fn identity_confidence_unread_when_all_fail() {
    let count = Arc::new(AtomicUsize::new(0));
    let adapter = CountingVinAdapter::new("SHORT", Arc::clone(&count));
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();

    let outcome = acquire_identity(&mut session, 0).await;

    assert_eq!(outcome.confidence, IdentityConfidence::Unread);
    assert_eq!(outcome.vin, None);
}

#[test]
fn manual_confirm_requires_partial() {
    let ctx = test_context(Protocol::J1850Vpw, IdentityConfidence::Single);
    let mut registry = ProfileRegistry::new();
    registry.register(&NO_MATCH_PROFILE);
    registry.register(&PARTIAL_PROFILE);

    assert!(registry
        .confirm_manual(&ctx, ProfileId::new("test.none"))
        .is_err());
    let selected = registry
        .confirm_manual(&ctx, ProfileId::new("test.partial"))
        .unwrap();
    assert!(selected.manual_confirmed());
    assert!(selected.is_valid_for(ctx.generation));
}

#[test]
fn floor_rejects_protocol_auto() {
    let ctx = test_context(Protocol::Auto, IdentityConfidence::Confirmed);
    let mut registry = ProfileRegistry::new();
    registry.register(&EXACT_A);

    let state = registry.select(&ctx);

    assert!(state.selected.is_none());
    assert!(state.exact_matches.is_empty());
    assert_eq!(state.partial_matches.len(), 1);
}

#[tokio::test]
async fn floor_requires_confirmed_vin_for_exact() {
    let ctx = loaded_lly_context(IdentityConfidence::Single).await;
    let registry = ProfileRegistry::with_builtins();

    let state = registry.select(&ctx);

    assert!(state.selected.is_none());
    assert!(state.exact_matches.is_empty());
    assert_eq!(
        state.partial_matches[0].profile_id.as_str(),
        "gm.gmt800.lly.class2"
    );
}

#[tokio::test]
async fn lly_exact_match_from_mock_identity() {
    let ctx = loaded_lly_context(IdentityConfidence::Confirmed).await;
    let registry = ProfileRegistry::with_builtins();

    let state = registry.select(&ctx);

    assert_eq!(
        state.exact_matches,
        vec![ProfileId::new("gm.gmt800.lly.class2")]
    );
    assert!(state.selected.is_some());
}

#[tokio::test]
async fn select_exact_iff_legacy_gate_true_for_confirmed_vin() {
    let ctx = loaded_lly_context(IdentityConfidence::Confirmed).await;
    let registry = ProfileRegistry::with_builtins();
    let selected = registry.select(&ctx).selected.is_some();
    let legacy = lly_profile_matches(
        ctx.vin.as_deref().unwrap(),
        ctx.spec.as_ref().map(|spec| spec as &VehicleSpec),
        ctx.protocol,
    );

    assert_eq!(selected, legacy);
}

#[test]
fn ambiguity_blocks_selection() {
    let ctx = test_context(Protocol::J1850Vpw, IdentityConfidence::Confirmed);
    let mut registry = ProfileRegistry::new();
    registry.register(&EXACT_A);
    registry.register(&EXACT_B);

    let state = registry.select(&ctx);

    assert!(state.selected.is_none());
    assert_eq!(state.exact_matches.len(), 2);
    assert!(state.ambiguity.is_some());
}
