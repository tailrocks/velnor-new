// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest};
use crate::crate_release_journal::JournalReceipt;
use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_mise::CheckDeadline;

fn with_context<T>(
    request: &GitHubAdmissionRequest,
    operation: impl FnOnce(
        &std::path::Path,
        &CrateReleaseIntent,
        &crate::crate_release_git::ReleaseGit,
        CheckDeadline,
    ) -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let invalid = || internal("crate_release_history_custody_required");
    let workspace =
        std::path::PathBuf::from(std::env::var("GITHUB_WORKSPACE").map_err(|_| invalid())?);
    let temp = std::path::PathBuf::from(std::env::var("RUNNER_TEMP").map_err(|_| invalid())?);
    let git = crate::crate_release_git::ReleaseGit::new(
        velnor_actions_mise::release_git_pin::verify_bundle(&temp.join("velnor/git-bundle"))
            .map_err(|_| invalid())?,
    );
    let deadline =
        CheckDeadline::after(std::time::Duration::from_secs(600)).map_err(|_| invalid())?;
    deadline
        .scope(|| {
            let intent = crate::release::crate_release_runtime::admission::read_intent_at_workflow(
                &workspace,
                &request.workflow_sha,
                &git,
            )?;
            if crate::release::crate_release_runtime::environment::request_from_environment(
                &intent,
                &request.workflow_sha,
            )? != *request
            {
                return Err(invalid());
            }
            let source = workspace.join("release-source");
            crate::release::crate_release_source::verify_crate_release_source(
                &source,
                &request.source_sha,
                &git,
            )?;
            let result = operation(&source, &intent, &git, deadline)?;
            crate::release::crate_release_source::verify_crate_release_source(
                &source,
                &request.source_sha,
                &git,
            )?;
            Ok(result)
        })
        .map_err(|_| invalid())?
}

pub(crate) fn verify_prior(
    prior: &JournalReceipt,
    request: &GitHubAdmissionRequest,
    github: &dyn GitHubAdmissionRead,
) -> Result<CrateReleaseIntent, OrchestratorError> {
    with_context(request, |root, _, git, deadline| {
        crate::release::crate_release_historical_intent::verify_prior_bound(
            root,
            prior,
            request,
            github,
            git,
            deadline,
            &crate::release::crate_release_historical_intent::DirectHistoryWatch,
        )
    })
}

pub(crate) fn verify_transition(
    prior: &JournalReceipt,
    current: &JournalReceipt,
    request: &GitHubAdmissionRequest,
    github: &dyn GitHubAdmissionRead,
) -> Result<(), OrchestratorError> {
    with_context(request, |root, intent, git, deadline| {
        crate::release::crate_release_historical_intent::verify_runner_transition(
            root,
            crate::release::crate_release_historical_intent::TransitionReceipts {
                prior,
                current,
                current_intent: intent,
            },
            request,
            github,
            git,
            deadline,
            &crate::release::crate_release_historical_intent::DirectHistoryWatch,
        )
    })
}
