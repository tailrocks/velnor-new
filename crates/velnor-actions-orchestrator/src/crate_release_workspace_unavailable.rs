// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CrateReleaseIntent, HistoricalIntentReader, OrchestratorError};

impl HistoricalIntentReader<'_> {
    pub(crate) fn verify_transition(
        &self,
        _prior: &crate::crate_release_journal::JournalReceipt,
        _current: &crate::crate_release_journal::JournalReceipt,
        _request: &crate::crate_release_github::GitHubAdmissionRequest,
        _github: &dyn crate::crate_release_github::GitHubAdmissionRead,
    ) -> Result<(), OrchestratorError> {
        match self.unavailable {}
    }

    pub(crate) fn verify_prior(
        &self,
        _prior: &crate::crate_release_journal::JournalReceipt,
        _request: &crate::crate_release_github::GitHubAdmissionRequest,
        _github: &dyn crate::crate_release_github::GitHubAdmissionRead,
    ) -> Result<CrateReleaseIntent, OrchestratorError> {
        match self.unavailable {}
    }
}
