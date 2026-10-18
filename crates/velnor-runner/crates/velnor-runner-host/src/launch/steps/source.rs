//! Authenticate one job source before the controller changes remote state.

use velnor_runner_github::SourceMetadata;
use velnor_runner_github::workflow_run::{
    RepositoryTarget, VerifiedPullRequest, VerifiedWorkflowSource, WorkflowRefKind,
    get_pull_request, get_workflow_run, verify_pull_request, verify_workflow_source,
};

use crate::config::{RepositoryConnection, SourcePolicy, SourcePolicyMode, WorkflowEvent};
use crate::https::HttpsTransport;

const GITHUB_API: &str = "https://api.github.com";

/// Nonsecret source facts matched to an authenticated workflow run and policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthorizedSourceReceipt {
    request_id: i64,
    message_id: i64,
    source: VerifiedWorkflowSource,
    policy_mode: SourcePolicyMode,
}

impl AuthorizedSourceReceipt {
    /// Stable Scale Set request identity that the policy approved.
    #[must_use]
    pub(crate) const fn request_id(&self) -> i64 {
        self.request_id
    }

    /// Queue message that carried this offer. This is audit data only.
    #[must_use]
    pub(crate) const fn message_id(&self) -> i64 {
        self.message_id
    }

    /// Source values checked against the authenticated GitHub run record.
    #[must_use]
    pub(crate) const fn source(&self) -> &VerifiedWorkflowSource {
        &self.source
    }

    /// Policy mode that approved this source.
    #[must_use]
    pub(crate) const fn policy_mode(&self) -> SourcePolicyMode {
        self.policy_mode
    }
}

#[cfg(test)]
pub(crate) fn test_receipt(
    request_id: i64,
    message_id: i64,
    source: VerifiedWorkflowSource,
    policy_mode: SourcePolicyMode,
) -> AuthorizedSourceReceipt {
    AuthorizedSourceReceipt {
        request_id,
        message_id,
        source,
        policy_mode,
    }
}

/// Why one job cannot pass the configured source policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceBlock {
    /// One required source field is absent.
    Missing,
    /// The authenticated GitHub lookup failed.
    ApiUnavailable,
    /// The job source did not match the authenticated repository record.
    SourceMismatch,
    /// The event is outside the configured policy.
    EventDenied,
    /// The ref is outside the configured policy.
    RefDenied,
    /// The pull request does not match one configured rule.
    PullRequestDenied,
    /// The API cannot prove the tested pull request merge commit.
    TestedMergeProofUnavailable,
    /// The whole set of jobs that a generic JIT runner may receive is not proven.
    ClosedWorldUnproven,
}

impl SourceBlock {
    /// Return a stable status value without source data.
    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::Missing => "source_metadata_missing",
            Self::ApiUnavailable => "source_api_unavailable",
            Self::SourceMismatch => "source_mismatch",
            Self::EventDenied => "event_denied",
            Self::RefDenied => "ref_denied",
            Self::PullRequestDenied => "pull_request_denied",
            Self::TestedMergeProofUnavailable => "tested_merge_proof_unavailable",
            Self::ClosedWorldUnproven => "closed_world_unproven",
        }
    }
}

/// Verify message source fields and apply the connection policy.
pub(super) fn authorize_job(
    connection: &RepositoryConnection,
    source: &SourceMetadata,
    request_id: i64,
    message_id: i64,
    token: &str,
) -> Result<AuthorizedSourceReceipt, SourceBlock> {
    if request_id <= 0 || message_id < 0 {
        return Err(SourceBlock::Missing);
    }
    let complete = source.complete().ok_or(SourceBlock::Missing)?;
    let (owner, repository) = split_repository(&connection.repository)?;
    let mut transport = HttpsTransport::new(GITHUB_API).map_err(|_| SourceBlock::ApiUnavailable)?;
    let run = get_workflow_run(
        &mut transport,
        owner,
        repository,
        complete.workflow_run_id,
        token,
    )
    .map_err(|_| SourceBlock::ApiUnavailable)?;
    let verified = verify_workflow_source(
        complete,
        &run,
        RepositoryTarget {
            id: connection.repository_id,
            owner,
            name: repository,
        },
    )
    .map_err(|_| SourceBlock::SourceMismatch)?;
    authorize_policy(&connection.source_policy, &verified)?;
    if verified.event_name == "pull_request"
        && connection.source_policy.mode == SourcePolicyMode::BranchRestricted
    {
        verify_pull(connection, &mut transport, token, &run, &verified)?;
        return Err(SourceBlock::TestedMergeProofUnavailable);
    }
    Ok(AuthorizedSourceReceipt {
        request_id,
        message_id,
        source: verified,
        policy_mode: connection.source_policy.mode,
    })
}

fn verify_pull(
    connection: &RepositoryConnection,
    transport: &mut HttpsTransport,
    token: &str,
    run: &velnor_runner_github::workflow_run::WorkflowRun,
    verified: &VerifiedWorkflowSource,
) -> Result<VerifiedPullRequest, SourceBlock> {
    let number = verified
        .job_workflow
        .reference
        .strip_prefix("refs/pull/")
        .and_then(|value| value.strip_suffix("/merge"))
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or(SourceBlock::PullRequestDenied)?;
    let (owner, repository) = split_repository(&connection.repository)?;
    let pull = get_pull_request(transport, owner, repository, number, token)
        .map_err(|_| SourceBlock::ApiUnavailable)?;
    let verified_pull = verify_pull_request(run, &pull, connection.repository_id)
        .map_err(|_| SourceBlock::PullRequestDenied)?;
    if !connection
        .source_policy
        .pull_request_rules
        .iter()
        .any(|rule| pull_rule_matches(rule, &verified_pull))
    {
        return Err(SourceBlock::PullRequestDenied);
    }
    Ok(verified_pull)
}

fn authorize_policy(
    policy: &SourcePolicy,
    source: &VerifiedWorkflowSource,
) -> Result<(), SourceBlock> {
    let event = match source.event_name.as_str() {
        "pull_request" => WorkflowEvent::PullRequest,
        "push" => WorkflowEvent::Push,
        "merge_group" => WorkflowEvent::MergeGroup,
        "schedule" => WorkflowEvent::Schedule,
        "workflow_dispatch" => WorkflowEvent::WorkflowDispatch,
        "release" => WorkflowEvent::Release,
        _ => return Err(SourceBlock::EventDenied),
    };
    if !policy.events.contains(&event) {
        return Err(SourceBlock::EventDenied);
    }
    if policy.mode == SourcePolicyMode::Repository {
        return Ok(());
    }
    if event == WorkflowEvent::PullRequest {
        return (source.job_workflow.kind == WorkflowRefKind::PullRequest)
            .then_some(())
            .ok_or(SourceBlock::RefDenied);
    }
    if source_ref_allowed(policy, source, event) {
        Ok(())
    } else {
        Err(SourceBlock::RefDenied)
    }
}

fn source_ref_allowed(
    policy: &SourcePolicy,
    source: &VerifiedWorkflowSource,
    event: WorkflowEvent,
) -> bool {
    match source.job_workflow.kind {
        WorkflowRefKind::Branch => allowed_branch(policy, source),
        WorkflowRefKind::Tag if matches!(event, WorkflowEvent::Push | WorkflowEvent::Release) => {
            allowed_tag(policy, source)
        }
        WorkflowRefKind::MergeQueue if event == WorkflowEvent::MergeGroup => {
            allowed_head_branch(policy, source)
        }
        WorkflowRefKind::Commit if event == WorkflowEvent::Push => {
            source.job_workflow.reference == source.head_sha && allowed_head_branch(policy, source)
        }
        WorkflowRefKind::PullRequest
        | WorkflowRefKind::MergeQueue
        | WorkflowRefKind::Other
        | WorkflowRefKind::Tag
        | WorkflowRefKind::Commit => false,
    }
}

fn allowed_branch(policy: &SourcePolicy, source: &VerifiedWorkflowSource) -> bool {
    source
        .job_workflow
        .reference
        .strip_prefix("refs/heads/")
        .is_some_and(|branch| {
            source.head_branch.as_deref() == Some(branch)
                && policy.branches.iter().any(|allowed| allowed == branch)
        })
}

fn allowed_head_branch(policy: &SourcePolicy, source: &VerifiedWorkflowSource) -> bool {
    source
        .head_branch
        .as_ref()
        .is_some_and(|branch| policy.branches.iter().any(|allowed| allowed == branch))
}

fn allowed_tag(policy: &SourcePolicy, source: &VerifiedWorkflowSource) -> bool {
    source
        .job_workflow
        .reference
        .strip_prefix("refs/tags/")
        .is_some_and(|tag| {
            source.head_branch.as_deref() == Some(tag)
                && policy
                    .tag_prefixes
                    .iter()
                    .any(|prefix| tag.starts_with(prefix))
        })
}

fn pull_rule_matches(rule: &crate::config::PullRequestRule, pull: &VerifiedPullRequest) -> bool {
    rule.number
        .is_none_or(|number| i64::try_from(number).is_ok_and(|number| number == pull.number))
        && rule.base_repository_id == pull.base_repository_id
        && rule.base_branch == pull.base_ref
        && rule.head_repository_id == pull.head_repository_id
        && rule.head_branch == pull.head_ref
}

fn split_repository(repository: &str) -> Result<(&str, &str), SourceBlock> {
    let Some((owner, name)) = repository.split_once('/') else {
        return Err(SourceBlock::SourceMismatch);
    };
    if owner.is_empty() || name.is_empty() || name.contains('/') {
        return Err(SourceBlock::SourceMismatch);
    }
    Ok((owner, name))
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;
