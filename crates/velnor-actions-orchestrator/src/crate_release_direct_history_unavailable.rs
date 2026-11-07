// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest};
use crate::crate_release_journal::JournalReceipt;
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::config::CrateReleaseIntent;

pub(crate) fn verify_prior(
    _: &JournalReceipt,
    _: &GitHubAdmissionRequest,
    _: &dyn GitHubAdmissionRead,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    Err(internal("crate_release_history_custody_required"))
}
pub(crate) fn verify_transition(
    _: &JournalReceipt,
    _: &JournalReceipt,
    _: &GitHubAdmissionRequest,
    _: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    Err(internal("crate_release_history_custody_required"))
}
