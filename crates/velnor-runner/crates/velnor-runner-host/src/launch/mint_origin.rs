// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::journal::Outcome;
use velnor_runner_github::Certainty;

pub(super) enum MintOrigin {
    AcquiredJob,

    AssignedPopulation,
}

impl MintOrigin {
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
