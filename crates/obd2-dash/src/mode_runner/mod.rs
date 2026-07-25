//! Pure contracts for the reconnecting scan-mode runner.
//!
//! This module deliberately has no transport, session, or database access.
//! Lifecycle code can depend on these deterministic contracts without making
//! the scheduler responsible for I/O policy.

pub mod capability;
pub mod scheduler;
pub mod snapshot;

pub use capability::{CapabilityKey, CapabilityKind, CapabilityOutcome, CapabilitySet};
pub use scheduler::{RequestDescriptor, RequestKey, Scheduler, Tier, ViewId};
pub use snapshot::{
    CapabilityPersistence, CapabilityState, CapabilityVerification, ModeState, RunnerSnapshot,
};
