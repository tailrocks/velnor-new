// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub use crate::tap_merge::review_completion::provider::{ProviderApp, ProviderQuery, ProviderRead};
use crate::{
    OrchestratorError,
    crate_release_github::GitHubAdmissionRead,
    crate_release_qualification_source::{ReviewedPullState, ReviewedSourceRequest},
    github_review_feedback::{FeedbackRead, FeedbackSubject},
    internal::internal,
    tap_merge::review_completion::{provider, state::CompletionState},
};
use velnor_actions_contract::homebrew_merge_gate::TapFeedbackSnapshot;

pub trait DirectQualificationRead: GitHubAdmissionRead + FeedbackRead + ProviderRead {}
impl<T: GitHubAdmissionRead + FeedbackRead + ProviderRead> DirectQualificationRead for T {}

#[derive(Debug, PartialEq, Eq)]
pub struct DirectFeedbackEvidence {
    snapshot: TapFeedbackSnapshot,
    provider_digest: String,
}

pub fn verify_direct_codex_feedback(
    candidate: &ReviewedSourceRequest,
    api: &dyn DirectQualificationRead,
) -> Result<DirectFeedbackEvidence, OrchestratorError> {
    if !matches!(candidate.state, ReviewedPullState::OpenHead) {
        return Err(internal("qualification_direct_candidate_state"));
    }
    let first_source =
        crate::crate_release_qualification_source::verify_pull_source(candidate, api)?;
    let checks = crate::crate_release_qualification_checks::complete_check_runs(
        api,
        &candidate.repository,
        &candidate.head_sha,
    )?;
    let first = observe(candidate, api, api)?;
    let second = observe(candidate, api, api)?;
    if first != second
        || checks
            != crate::crate_release_qualification_checks::complete_check_runs(
                api,
                &candidate.repository,
                &candidate.head_sha,
            )?
        || first_source
            != crate::crate_release_qualification_source::verify_pull_source(candidate, api)?
    {
        return Err(internal("qualification_direct_feedback_changed"));
    }
    Ok(first)
}

fn observe(
    candidate: &ReviewedSourceRequest,
    api: &dyn FeedbackRead,
    provider_api: &dyn ProviderRead,
) -> Result<DirectFeedbackEvidence, OrchestratorError> {
    let subject = FeedbackSubject {
        repository: &candidate.repository,
        pull_request_number: candidate.pull_request,
        head_sha: &candidate.head_sha,
    };
    crate::crate_release_qualification_deletions::require_no_deleted_comments(
        candidate,
        provider_api,
    )?;
    let feedback = crate::github_review_feedback::capture_feedback_subject(api, &subject)?;
    if feedback.snapshot.unresolved_threads != 0
        || feedback.snapshot.outstanding_change_requests != 0
        || feedback.snapshot.unaddressed_findings != 0
    {
        return Err(internal("qualification_direct_feedback_incomplete"));
    }
    let (state, digest) = provider::inspect(
        provider_api,
        &candidate.head_sha,
        &feedback.records,
        None,
        (&candidate.repository, candidate.pull_request),
        candidate.repository_id,
        None,
    )?;
    if state != CompletionState::Ready {
        return Err(internal("qualification_direct_provider_incomplete"));
    }
    let provider_digest =
        digest.ok_or_else(|| internal("qualification_direct_provider_missing"))?;
    Ok(DirectFeedbackEvidence {
        snapshot: feedback.snapshot,
        provider_digest,
    })
}

#[cfg(test)]
#[path = "crate_release_qualification_provider_tests.rs"]
mod tests;
