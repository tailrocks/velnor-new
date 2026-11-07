// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest};
use crate::crate_release_journal::JournalReceipt;
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_mise::{CheckDeadline, release_git_pin::ReleaseGitExecutable as _};

pub(crate) struct HistoryContext<'a> {
    source: &'a std::path::Path,
    intent: &'a CrateReleaseIntent,
    request: &'a GitHubAdmissionRequest,
    git: &'a crate::crate_release_git::ReleaseGit,
    deadline: CheckDeadline,
}

impl<'a> HistoryContext<'a> {
    pub(crate) fn admitted(
        source: &'a std::path::Path,
        intent: &'a CrateReleaseIntent,
        request: &'a GitHubAdmissionRequest,
        git: &'a crate::crate_release_git::ReleaseGit,
        receipt: &JournalReceipt,
        deadline: CheckDeadline,
    ) -> Result<Self, OrchestratorError> {
        let invalid = || internal("crate_release_history_custody_required");
        let deadline = deadline.effective();
        deadline.remaining().map_err(|_| invalid())?;
        if git.verified().is_none()
            || receipt.identity.repository != request.repository
            || receipt.identity.workflow_sha != request.workflow_sha
            || receipt.identity.source_sha != request.source_sha
            || receipt.identity.run_id != request.run_id
            || receipt.identity.run_attempt != request.run_attempt
        {
            return Err(invalid());
        }
        crate::crate_release_controller::verify_request_intent(intent, request)?;
        let bytes = velnor_actions_contract::canonical_json_bytes(intent).map_err(|_| invalid())?;
        if crate::planner::generator::sha256_hex(&bytes) != receipt.intent_sha256
            || crate::crate_release_journal::release_base_intent_sha256(intent)?
                != receipt.base_intent_sha256
        {
            return Err(invalid());
        }
        deadline
            .scope(|| {
                git.revalidate().map_err(|_| invalid())?;
                crate::verify_journal_source(source, receipt, git)?;
                let actual = super::crate_release_historical_intent::read_historical_intent(
                    source,
                    &request.workflow_sha,
                    git,
                    deadline,
                )?;
                if actual != *intent {
                    return Err(invalid());
                }
                crate::verify_journal_source(source, receipt, git)
            })
            .map_err(|_| invalid())??;
        Ok(Self {
            source,
            intent,
            request,
            git,
            deadline,
        })
    }

    pub(crate) fn verify_prior(
        &self,
        prior: &JournalReceipt,
        github: &dyn GitHubAdmissionRead,
    ) -> Result<CrateReleaseIntent, OrchestratorError> {
        self.deadline
            .scope(|| {
                super::crate_release_historical_intent::verify_prior_bound(
                    self.source,
                    prior,
                    self.request,
                    github,
                    self.git,
                    self.deadline,
                )
            })
            .map_err(|_| internal("crate_release_history_custody_required"))?
    }

    pub(crate) fn verify_transition(
        &self,
        prior: &JournalReceipt,
        current: &JournalReceipt,
        github: &dyn GitHubAdmissionRead,
    ) -> Result<(), OrchestratorError> {
        self.deadline
            .scope(|| {
                super::crate_release_historical_intent::verify_runner_transition(
                    self.source,
                    super::crate_release_historical_intent::TransitionReceipts {
                        prior,
                        current,
                        current_intent: self.intent,
                    },
                    self.request,
                    github,
                    self.git,
                    self.deadline,
                )
            })
            .map_err(|_| internal("crate_release_history_custody_required"))?
    }
}
