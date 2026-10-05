//! Provenance for JIT requests that can fail after a remote job was acquired.

use crate::journal::Outcome;

/// Whether a JIT request follows an already successful acquire operation.
pub(super) enum MintOrigin {
    /// `AcquireJobs` returned the offered job before this JIT request.
    AcquiredJob,
    /// A positive assignment count requested a runner without `AcquireJobs`.
    AssignedPopulation,
}

impl MintOrigin {
    /// A JIT conflict cannot settle an earlier successful `AcquireJobs` effect.
    #[must_use]
    pub(super) const fn conflict_outcome(self) -> Outcome {
        match self {
            Self::AcquiredJob => Outcome::Uncertain,
            Self::AssignedPopulation => Outcome::DefiniteFailure,
        }
    }
}
