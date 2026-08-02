//! Pure contracts for the reconnecting scan-mode runner.
//!
//! This module deliberately has no transport, session, or database access.
//! Lifecycle code can depend on these deterministic contracts without making
//! the scheduler responsible for I/O policy.

pub mod capability;
pub mod command;
pub mod diagnostic;
pub mod lifecycle;
pub mod persistence;
pub mod scheduler;
pub mod snapshot;
pub mod store;
/// Deterministic connector and adapter harness used by lifecycle integration tests.
pub mod testing;
pub mod verifier;

pub use capability::{
    default_probe_fingerprint, protocol_token, CapabilityKey, CapabilityKind, CapabilityOutcome,
    CapabilitySet,
};
pub use command::{reply_for, CommandReply, RunnerCommand};
pub use diagnostic::{
    expand_dtc_requests, mode05_allowed, phases as diagnostic_phases, request_plan, resolve_fuel,
    service_allowed, DiagnosticPhase, DiagnosticRequest, FuelClass, RequestExpansion,
    RequestTarget, ServiceGates,
};
pub use lifecycle::{ConnectError, ModeRunner, NewSession, SessionConnector};
pub use persistence::Persistence;
pub use scheduler::{RequestDescriptor, RequestKey, Scheduler, Tier, ViewId};
pub use snapshot::{
    CapabilityPersistence, CapabilityState, CapabilityVerification, ModeState, RunnerSnapshot,
};
pub use store::{CapabilityStore, SqliteCapabilityStore};
pub use verifier::{ProbeError, VerificationResult, Verifier};
