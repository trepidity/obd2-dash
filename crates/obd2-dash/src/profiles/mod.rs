//! Wave 1: pure profile model. Single owner of all shared Layer-3 types.

// The profile contract intentionally includes variants before every backend
// consumes them so unsupported modules/protocols remain typed, not stringly.
#![allow(dead_code)]

pub mod evidence;
#[cfg(any(test, feature = "proof-profile"))]
pub mod fixture;
pub mod gm;
pub mod model;
#[cfg(feature = "probe")]
pub mod probe;
#[cfg(not(feature = "probe"))]
pub(crate) mod probe;
pub mod registry;
pub mod runtime;
pub mod scheduler;
pub mod selection;

pub use evidence::{
    capability_key, confidence_label, dtc_status_from_label, provenance_label,
    ProfileDecodedEvidence, ProfileDtcEvidence, ProfileEvidenceError, ProfileEvidenceRecord,
    RouteEvidence, SourceFieldsEvidence,
};
pub use model::{
    ActiveCommandProfile, ActivePrecondition, ActiveTestDefinition, AddressState, AddressTemplate,
    BackoffPolicy, BusDefinition, BusKey, Confidence, DecodedDtc, DecodedSignal, DiagnosticProfile,
    DtcServiceDefinition, EvidencePolicy, EvidenceRef, FailurePolicy, IdentityConfidence,
    J1850HeaderConvention, Manufacturer, MatchConfidence, ModuleDefinition, ModuleEvidenceState,
    ModuleKey, ModuleMap, ModuleSafetyClass, PairRole, PassiveCapabilityState,
    PassiveMonitorDefinition, PollCadence, ProfileDecodeError, ProfileId, ProfileMatch,
    ProfileRequestDefinition, Provenance, RouteDefinition, RouteScope, RouteSet, RxdSource,
    SafetyClass, SelectedProfile, Selection, SignalCategory, SignalComposition, SignalDefinition,
    SignalDisplayDefinition, SignalDisplaySource, SourceFields, StandardPidOverride,
    StandardPidPolicy, VehicleContext,
};
pub use registry::{builtin_profile, ManualConfirmError, ProfileRegistry};
pub use runtime::{
    bus_family, resolve_route, resolve_route_target, CapabilityId, DispatchError, DispatchEvidence,
    NullEvidenceSink, ProfileEvidenceSink, ProfileResponse, ProfileRuntime, RequestId,
    ResolvedRoute,
};
pub use scheduler::{BackedOffRequest, CoverageMap, MaintenancePlan, PlannedRequest, PollPlan};
pub use selection::{
    acquire_identity, build_vehicle_context, confidence_warning, next_generation,
    select_into_state, validate_vin_charset, IdentityOutcome, PartialProfileMatch,
    ProfileAmbiguity, ProfileState, ProfileStateSnapshot,
};
