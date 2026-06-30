use std::fs;
use std::path::PathBuf;

use obd2_core::adapter::mock::MockAdapter;
use obd2_core::session::Session;
use obd2_core::vehicle::Protocol;
use obd2_dash::gm_enhanced::{
    Confidence as GmConfidence, FailurePolicy as GmFailurePolicy, LLY_ENHANCED_DIDS,
};
use obd2_dash::profiles::{
    build_vehicle_context, BackedOffRequest, CapabilityId, CoverageMap, IdentityConfidence,
    IdentityOutcome, ModuleKey, ProfileRegistry, ProfileRuntime, RequestId,
};

#[tokio::test]
async fn plan_poll_cycle_reproduces_lly_order_and_cadence() {
    let (registry, selected) = selected_lly().await;
    let runtime = ProfileRuntime::new(&registry);
    let coverage = lly_coverage();

    let off_cycle = runtime.plan_poll_cycle(Some(&selected), 4, &coverage);
    assert!(
        off_cycle.requests.is_empty(),
        "LLY profile requests must stay gated off before the enhanced cadence boundary"
    );

    let plan = runtime.plan_poll_cycle(Some(&selected), 5, &coverage);
    let planned = planned_signal_keys(&plan);
    let expected = accepted_legacy_lly_signal_keys();

    assert_eq!(planned, expected);
    assert!(!plan.maintenance.scan_standard_dtcs);
    assert!(!plan.maintenance.poll_o2_monitoring);
    assert!(!plan.maintenance.poll_readiness);

    let slow_obd = runtime.plan_poll_cycle(Some(&selected), 20, &coverage);
    assert!(slow_obd.maintenance.scan_standard_dtcs);
    assert!(slow_obd.maintenance.poll_o2_monitoring);
    assert!(slow_obd.maintenance.poll_readiness);
}

#[tokio::test]
async fn plan_poll_cycle_schedules_lly_class2_dtcs_every_sixty_cycles() {
    let (registry, selected) = selected_lly().await;
    let runtime = ProfileRuntime::new(&registry);
    let coverage = lly_coverage();

    let gated = runtime.plan_poll_cycle(Some(&selected), 10, &coverage);
    assert!(
        planned_dtc_routes(&gated).is_empty(),
        "GM Class 2 profile DTC requests must stay off the generic ten-cycle DTC boundary"
    );

    let plan = runtime.plan_poll_cycle(Some(&selected), 60, &coverage);
    let planned = planned_dtc_routes(&plan);
    let modules = ["abs", "bcm", "ecm", "ficm", "tcm"];
    let mut expected = Vec::new();
    for key in ["lly.class2.dtc.all", "lly.class2.dtc.active"] {
        for module in modules {
            expected.push((key, module));
        }
    }

    assert_eq!(planned, expected);
}

#[tokio::test]
async fn generic_only_drops_lly_forced_standard_pids() {
    let (registry, selected) = selected_lly().await;
    let runtime = ProfileRuntime::new(&registry);
    let coverage = CoverageMap::new(vec![0x05, 0x33]).with_supported_standard_pids(vec![0x05]);

    let generic = runtime.plan_poll_cycle(None, 1, &coverage);
    assert_eq!(generic.standard_pids, vec![0x05]);

    let lly = runtime.plan_poll_cycle(Some(&selected), 1, &coverage);
    assert_eq!(lly.standard_pids, vec![0x05, 0x33]);
}

#[tokio::test]
async fn backoff_candidate_and_preferred_standard_policy_are_applied() {
    let (registry, selected) = selected_lly().await;
    let runtime = ProfileRuntime::new(&registry);
    let coverage = lly_coverage()
        .with_supported_standard_pids(vec![0x23])
        .with_backed_off(vec![BackedOffRequest {
            capability: CapabilityId::DtcService("lly.class2.dtc.all"),
            request: RequestId(0),
        }]);

    let signal_plan = runtime.plan_poll_cycle(Some(&selected), 5, &coverage);
    let signals = planned_signal_keys(&signal_plan);
    assert!(!signals.iter().any(|key| key == "lly.1542"));
    assert!(!signals.iter().any(|key| key == "lly.163E"));

    let dtc_plan = runtime.plan_poll_cycle(Some(&selected), 60, &coverage);
    let dtcs = planned_dtc_routes(&dtc_plan);
    assert!(!dtcs
        .iter()
        .any(|(key, module)| *key == "lly.class2.dtc.all" && *module == "abs"));
    assert!(dtcs
        .iter()
        .any(|(key, module)| *key == "lly.class2.dtc.active" && *module == "abs"));
}

#[tokio::test]
async fn plan_poll_cycle_does_not_poll_undiscovered_signal_modules() {
    let (registry, selected) = selected_lly().await;
    let runtime = ProfileRuntime::new(&registry);
    let coverage = CoverageMap::new(Vec::new()).with_discovered_modules(vec![ModuleKey::Ecm]);

    let plan = runtime.plan_poll_cycle(Some(&selected), 5, &coverage);
    let signals = planned_signal_keys(&plan);

    assert!(signals.iter().any(|key| key == "lly.1540"));
    assert!(!signals.iter().any(|key| key == "lly.1940"));
}

#[test]
fn scheduler_has_no_manufacturer_branch() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/profiles/scheduler.rs");
    let text = fs::read_to_string(path).unwrap();
    for forbidden in ["gm_", "gm::", "Gm", "Manufacturer::", "LLY", "lly", "0x"] {
        assert!(
            !text.contains(forbidden),
            "scheduler.rs contains manufacturer-specific policy marker `{forbidden}`"
        );
    }
}

async fn selected_lly() -> (ProfileRegistry, obd2_dash::profiles::SelectedProfile) {
    let adapter = MockAdapter::with_vin("1GCHK23224F000001");
    let mut session = Session::new(adapter);
    session.initialize().await.unwrap();
    session.identify_vehicle().await.unwrap();
    let identity = IdentityOutcome {
        vin: Some("1GCHK23224F000001".into()),
        confidence: IdentityConfidence::Confirmed,
    };
    let mut context = build_vehicle_context(&session, 11, &identity);
    context.protocol = Protocol::J1850Vpw;

    let registry = ProfileRegistry::with_builtins();
    let selected = registry
        .select(&context)
        .selected
        .expect("confirmed LLY fixture should select the LLY profile");
    (registry, selected)
}

fn lly_coverage() -> CoverageMap {
    CoverageMap::new(Vec::new()).with_discovered_modules(vec![
        ModuleKey::Ebcm,
        ModuleKey::Bcm,
        ModuleKey::Ecm,
        ModuleKey::Ficm,
        ModuleKey::Tcm,
    ])
}

fn accepted_legacy_lly_signal_keys() -> Vec<String> {
    LLY_ENHANCED_DIDS
        .iter()
        .filter(|definition| definition.confidence != GmConfidence::Candidate)
        .filter(|definition| definition.failure_policy != GmFailurePolicy::CandidateOnly)
        .map(|definition| format!("lly.{:04X}", definition.did))
        .collect()
}

fn planned_signal_keys(plan: &obd2_dash::profiles::PollPlan) -> Vec<String> {
    plan.requests
        .iter()
        .filter_map(|request| match request.capability {
            CapabilityId::Signal(key) => Some(key.to_string()),
            _ => None,
        })
        .collect()
}

fn planned_dtc_routes(plan: &obd2_dash::profiles::PollPlan) -> Vec<(&'static str, &'static str)> {
    plan.requests
        .iter()
        .filter_map(|request| match request.capability {
            CapabilityId::DtcService(key) => Some((key, request.route.module.canonical())),
            _ => None,
        })
        .collect()
}
