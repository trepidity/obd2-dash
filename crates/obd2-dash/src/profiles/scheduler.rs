use crate::profiles::model::{
    Confidence, DiagnosticProfile, FailurePolicy, ModuleKey, ModuleMap, PollCadence,
    RouteDefinition, RouteScope, RouteSet, SelectedProfile,
};
use crate::profiles::runtime::{CapabilityId, ProfileRuntime, RequestId};

const PROFILE_SIGNAL_INTERVAL: u64 = 5;
const STANDARD_DTC_INTERVAL: u64 = 10;
const SLOW_DTC_INTERVAL: u64 = 60;
const SLOW_OBD_INTERVAL: u64 = 20;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CoverageMap {
    pub standard_pids: Vec<u8>,
    pub supported_standard_pids: Option<Vec<u8>>,
    pub discovered_modules: Vec<ModuleKey>,
    pub backed_off: Vec<BackedOffRequest>,
}

impl CoverageMap {
    pub fn new(standard_pids: Vec<u8>) -> Self {
        Self {
            standard_pids,
            supported_standard_pids: None,
            discovered_modules: Vec::new(),
            backed_off: Vec::new(),
        }
    }

    pub fn with_supported_standard_pids(mut self, pids: Vec<u8>) -> Self {
        self.supported_standard_pids = Some(pids);
        self
    }

    pub fn with_discovered_modules(mut self, modules: Vec<ModuleKey>) -> Self {
        self.discovered_modules = modules;
        self
    }

    pub fn with_backed_off(mut self, backed_off: Vec<BackedOffRequest>) -> Self {
        self.backed_off = backed_off;
        self
    }

    fn standard_pid_available(&self, pid: u8) -> bool {
        match &self.supported_standard_pids {
            Some(supported) => supported.contains(&pid),
            None => self.standard_pids.contains(&pid),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BackedOffRequest {
    pub capability: CapabilityId,
    pub request: RequestId,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PollPlan {
    pub standard_pids: Vec<u8>,
    pub requests: Vec<PlannedRequest>,
    pub maintenance: MaintenancePlan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlannedRequest {
    pub capability: CapabilityId,
    pub request: RequestId,
    pub route: RouteDefinition,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaintenancePlan {
    pub scan_standard_dtcs: bool,
    pub poll_o2_monitoring: bool,
    pub poll_readiness: bool,
}

impl ProfileRuntime<'_> {
    pub fn plan_poll_cycle(
        &self,
        selected: Option<&SelectedProfile>,
        cycle: u64,
        coverage: &CoverageMap,
    ) -> PollPlan {
        let mut plan = PollPlan::default();
        plan.maintenance.scan_standard_dtcs = cycle_due(cycle, STANDARD_DTC_INTERVAL);
        plan.maintenance.poll_o2_monitoring = cycle_due(cycle, SLOW_OBD_INTERVAL);
        plan.maintenance.poll_readiness = cycle_due(cycle, SLOW_OBD_INTERVAL);

        let profile = selected.and_then(|selected| self.registry.get(selected.profile_id()));
        plan.standard_pids = planned_standard_pids(profile, coverage);

        let Some(profile) = profile else {
            return plan;
        };

        if cycle_due(cycle, PROFILE_SIGNAL_INTERVAL) {
            append_signal_requests(profile, coverage, &mut plan.requests);
        }

        append_dtc_requests(profile, cycle, coverage, &mut plan.requests);
        plan
    }
}

fn planned_standard_pids(
    profile: Option<&dyn DiagnosticProfile>,
    coverage: &CoverageMap,
) -> Vec<u8> {
    let forced = profile
        .map(|profile| profile.standard_pid_policy().forced)
        .unwrap_or(&[]);
    let mut out = Vec::new();
    for pid in &coverage.standard_pids {
        if coverage.standard_pid_available(*pid) || forced.contains(pid) {
            out.push(*pid);
        }
    }
    out
}

fn append_signal_requests(
    profile: &dyn DiagnosticProfile,
    coverage: &CoverageMap,
    requests: &mut Vec<PlannedRequest>,
) {
    for signal in profile.signals() {
        if !should_poll_signal(signal.cadence, signal.confidence, signal.failure_policy) {
            continue;
        }
        if preferred_standard_pid_available(signal.preferred_over, coverage) {
            continue;
        }
        if !module_available(signal.route.module, coverage) {
            continue;
        }
        let capability = CapabilityId::Signal(signal.key);
        let request = RequestId::SINGLE;
        if is_backed_off(capability, request, coverage) {
            continue;
        }
        requests.push(PlannedRequest {
            capability,
            request,
            route: signal.route,
        });
    }
}

fn module_available(module: ModuleKey, coverage: &CoverageMap) -> bool {
    coverage.discovered_modules.is_empty() || coverage.discovered_modules.contains(&module)
}

fn append_dtc_requests(
    profile: &dyn DiagnosticProfile,
    cycle: u64,
    coverage: &CoverageMap,
    requests: &mut Vec<PlannedRequest>,
) {
    let Some(map) = profile.module_map() else {
        return;
    };

    for service in profile.dtc_services() {
        if !cycle_due(cycle, dtc_interval(service.cadence)) {
            continue;
        }
        let capability = CapabilityId::DtcService(service.key);
        append_route_set_requests(capability, service.route_set, map, coverage, requests);
    }
}

fn append_route_set_requests(
    capability: CapabilityId,
    route_set: RouteSet,
    map: &ModuleMap,
    coverage: &CoverageMap,
    requests: &mut Vec<PlannedRequest>,
) {
    match route_set.scope {
        RouteScope::Single(route) => {
            push_if_not_backed_off(capability, RequestId::SINGLE, route, coverage, requests);
        }
        RouteScope::Explicit(routes) => {
            for (idx, route) in routes.iter().copied().enumerate() {
                push_if_not_backed_off(capability, RequestId(idx), route, coverage, requests);
            }
        }
        RouteScope::DiscoveredOnBus { bus } => {
            let mut request = 0usize;
            for module in &coverage.discovered_modules {
                if !map
                    .modules
                    .iter()
                    .any(|entry| entry.key == *module && entry.bus == bus)
                {
                    continue;
                }
                push_if_not_backed_off(
                    capability,
                    RequestId(request),
                    RouteDefinition { module: *module },
                    coverage,
                    requests,
                );
                request += 1;
            }
        }
    }
}

fn push_if_not_backed_off(
    capability: CapabilityId,
    request: RequestId,
    route: RouteDefinition,
    coverage: &CoverageMap,
    requests: &mut Vec<PlannedRequest>,
) {
    if is_backed_off(capability, request, coverage) {
        return;
    }
    requests.push(PlannedRequest {
        capability,
        request,
        route,
    });
}

fn should_poll_signal(
    cadence: PollCadence,
    confidence: Confidence,
    failure_policy: FailurePolicy,
) -> bool {
    !matches!(cadence, PollCadence::OnDemand)
        && !matches!(confidence, Confidence::Candidate | Confidence::Rejected)
        && !matches!(
            failure_policy,
            FailurePolicy::CandidateOnly | FailurePolicy::DoNotPoll
        )
}

fn preferred_standard_pid_available(preferred_over: Option<&str>, coverage: &CoverageMap) -> bool {
    let Some(preferred) = preferred_over else {
        return false;
    };
    let Some(hex) = preferred.strip_prefix("standard:") else {
        return false;
    };
    let Ok(pid) = u8::from_str_radix(hex, 16) else {
        return false;
    };
    coverage.standard_pid_available(pid)
}

fn dtc_interval(cadence: PollCadence) -> u64 {
    match cadence {
        PollCadence::Fast | PollCadence::Medium => STANDARD_DTC_INTERVAL,
        PollCadence::Slow => SLOW_DTC_INTERVAL,
        PollCadence::OnDemand => u64::MAX,
    }
}

fn cycle_due(cycle: u64, interval: u64) -> bool {
    interval != 0 && cycle != 0 && cycle % interval == 0
}

fn is_backed_off(capability: CapabilityId, request: RequestId, coverage: &CoverageMap) -> bool {
    coverage
        .backed_off
        .iter()
        .any(|entry| entry.capability == capability && entry.request == request)
}
