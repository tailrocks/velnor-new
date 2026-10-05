//! Pure scale-set state: identifiers, capacity, transitions, and parity.
//!
//! No HTTP, Docker, database, or process effects live here.

mod capacity;
mod error;
mod evidence;
mod identity;
mod lifecycle;
mod ownership;

pub use capacity::Capacity;
pub use error::{EvidenceError, IdError, StateError};
pub use evidence::{
    ArchiveSafety, Conclusion, ExecutionKey, ExpectedExecutionSet, ExpectedItem, ParityProof,
    VerifiedExecutionReport, VerifiedJobCensus, verify_complete_results,
};
pub use identity::{
    AcquireIntentId, Epoch, GrantId, MessageId, ProvisionIntentId, RequestId, WorkerId,
};
pub use lifecycle::{Effect, Transition, WorkerEvent, WorkerState, transition};
pub use ownership::{CleanupProof, OwnedIds, OwnershipFailure};

#[cfg(test)]
mod lifecycle_tests;
