//! Provenance for JIT requests that can fail after a remote job was acquired.

use crate::journal::Outcome;
use velnor_runner_github::Certainty;

/// Whether a JIT request follows an already successful acquire operation.
pub(super) enum MintOrigin {
    /// `AcquireJobs` returned the offered job before this JIT request.
    AcquiredJob,
    /// A positive assignment count requested a runner without `AcquireJobs`.
    AssignedPopulation,
}

impl MintOrigin {
    /// Decide whether the JIT error settles this launch reservation.
    #[must_use]
    pub(super) const fn error_outcome(self, certainty: Certainty) -> Outcome {
        match self {
            Self::AcquiredJob => Outcome::Uncertain,
            Self::AssignedPopulation => match certainty {
                Certainty::Definite => Outcome::DefiniteFailure,
                Certainty::Uncertain => Outcome::Uncertain,
            },
        }
    }
}
