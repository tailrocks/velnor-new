use crate::{InnerJob, InnerKind};

mod evidence;

use super::types::{
    ActionsWorkflowTrustRun, JobTrustPolicyView, PolicyGap, PolicyMismatch,
    ReusableWorkflowEvidence, ReusableWorkflowRuleView, WorkflowTrustField,
    workflow_ref_for_repository,
};
use super::wire::ParsedTrustBatch;
pub use evidence::{JobTrustEvidence, VerifiedJobTrust};

/// Evaluate the trust facts for one explicit event. This does not authorize
/// `AcquireJobs`, runner minting, or any mapping from a request ID to a JIT
/// runner; the caller must independently hold pool-policy and capacity proofs.
#[must_use]
pub fn verify_job_offer(
    batch: &ParsedTrustBatch,
    event_index: usize,
    run: &ActionsWorkflowTrustRun,
    policy: &JobTrustPolicyView<'_>,
) -> JobTrustEvidence {
    let Some(event) = batch.event(event_index) else {
        return JobTrustEvidence::Unknown(PolicyGap::InvalidEventIndex);
    };
    let message_id = batch.message_id();
    let message = event.job();
    let identity = match validate_offer_identity(message_id, message, run, policy) {
        Ok(identity) => identity,
        Err(evidence) => return evidence,
    };
    let source = match validate_offer_source(run, policy) {
        Ok(source) => source,
        Err(evidence) => return evidence,
    };
    let workflow = match validate_offer_workflow(
        event.job_workflow_ref(),
        run,
        policy,
        identity.event,
        source.head_branch,
    ) {
        Ok(workflow) => workflow,
        Err(evidence) => return evidence,
    };

    JobTrustEvidence::Verified(Box::new(VerifiedJobTrust {
        message_id,
        request_id: identity.request_id,
        scale_set_job_id: message.job_id.clone(),
        workflow_run_id: run.id,
        repository_full_name: policy.repository_full_name.to_owned(),
        head_repository_full_name: source.head_repository.to_owned(),
        event: identity.event.to_owned(),
        workflow_ref: workflow.workflow_ref,
        job_workflow_ref: workflow.job_workflow_ref.to_owned(),
        head_branch: source.head_branch.to_owned(),
        head_sha: run.head_sha.clone(),
        policy_digest: policy.policy_digest.to_owned(),
    }))
}

struct OfferIdentity<'a> {
    request_id: i64,
    event: &'a str,
}

fn validate_offer_identity<'a>(
    message_id: i64,
    message: &'a InnerJob,
    run: &ActionsWorkflowTrustRun,
    policy: &JobTrustPolicyView<'_>,
) -> Result<OfferIdentity<'a>, JobTrustEvidence> {
    if !matches!(message.kind, InnerKind::Available | InnerKind::Assigned) {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::UnsupportedMessageKind,
        ));
    }
    if policy.repository_full_name.is_empty()
        || policy.allowed_repositories.len() != 1
        || !policy.allowed_repositories[0].eq_ignore_ascii_case(policy.repository_full_name)
        || policy.allowed_events.is_empty()
        || policy.allowed_workflow_paths.is_empty()
        || policy.allowed_head_branches.is_empty()
        || policy.workflow_rules.is_empty()
        || policy.policy_digest.is_empty()
    {
        return Err(JobTrustEvidence::Rejected(PolicyMismatch::InvalidPolicy));
    }
    if policy.allow_forks {
        return Err(JobTrustEvidence::Unknown(PolicyGap::ForkPolicyUnsupported));
    }
    let Some(request_id) = message.request_id.filter(|id| *id > 0) else {
        return Err(JobTrustEvidence::Unknown(PolicyGap::MissingEventIdentity));
    };
    let Some(message_run_id) = message.workflow_run_id.filter(|id| *id > 0) else {
        return Err(JobTrustEvidence::Unknown(PolicyGap::MissingEventIdentity));
    };
    if message_id < 0 || run.head_sha.is_empty() {
        return Err(JobTrustEvidence::Unknown(PolicyGap::MissingEventIdentity));
    }
    let (Some(owner), Some(repository), Some(event)) = (
        message.owner_name.as_deref(),
        message.repository_name.as_deref(),
        message.event_name.as_deref(),
    ) else {
        return Err(JobTrustEvidence::Unknown(PolicyGap::MissingField));
    };
    let Some((expected_owner, expected_repository)) = policy.repository_full_name.split_once('/')
    else {
        return Err(JobTrustEvidence::Rejected(PolicyMismatch::InvalidPolicy));
    };
    if !owner.eq_ignore_ascii_case(expected_owner)
        || !repository.eq_ignore_ascii_case(expected_repository)
    {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::EventRepositoryMismatch,
        ));
    }
    if run.id != message_run_id {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::WorkflowRunMismatch,
        ));
    }
    if run.event != event || !policy.allowed_events.iter().any(|allowed| allowed == event) {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::EventRepositoryMismatch,
        ));
    }
    Ok(OfferIdentity { request_id, event })
}

struct OfferSource<'a> {
    head_repository: &'a str,
    head_branch: &'a str,
}

fn validate_offer_source<'a>(
    run: &'a ActionsWorkflowTrustRun,
    policy: &JobTrustPolicyView<'_>,
) -> Result<OfferSource<'a>, JobTrustEvidence> {
    let head_repository = match &run.head_repository_full_name {
        WorkflowTrustField::Missing => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::MissingField));
        }
        WorkflowTrustField::Invalid => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::InvalidField));
        }
        WorkflowTrustField::Present(repository) => repository,
    };
    if !head_repository.eq_ignore_ascii_case(policy.repository_full_name) {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::ForkSourceMismatch,
        ));
    }
    let head_branch = match &run.head_branch {
        WorkflowTrustField::Missing => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::MissingField));
        }
        WorkflowTrustField::Invalid => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::InvalidField));
        }
        WorkflowTrustField::Present(branch) => branch,
    };
    if !policy
        .allowed_head_branches
        .iter()
        .any(|branch| branch == head_branch)
    {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::WorkflowReferenceMismatch,
        ));
    }
    Ok(OfferSource {
        head_repository,
        head_branch,
    })
}

struct OfferWorkflow<'a> {
    job_workflow_ref: &'a str,
    workflow_ref: String,
}

fn validate_offer_workflow<'a>(
    job_workflow_field: &'a WorkflowTrustField<String>,
    run: &ActionsWorkflowTrustRun,
    policy: &JobTrustPolicyView<'_>,
    event: &str,
    head_branch: &str,
) -> Result<OfferWorkflow<'a>, JobTrustEvidence> {
    let job_workflow_ref = match job_workflow_field {
        WorkflowTrustField::Missing => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::MissingField));
        }
        WorkflowTrustField::Invalid => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::InvalidField));
        }
        WorkflowTrustField::Present(reference) => reference,
    };
    if !policy
        .allowed_workflow_paths
        .iter()
        .any(|path| rest_path_matches_bare_workflow(path, &run.path))
    {
        return Err(JobTrustEvidence::Rejected(
            PolicyMismatch::WorkflowReferenceMismatch,
        ));
    }
    let referenced_workflows = match &run.referenced_workflows {
        WorkflowTrustField::Missing => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::MissingField));
        }
        WorkflowTrustField::Invalid => {
            return Err(JobTrustEvidence::Unknown(PolicyGap::InvalidField));
        }
        WorkflowTrustField::Present(workflows) => workflows,
    };
    let workflow_ref = workflow_ref_for_repository(policy.repository_full_name, &run.path);
    for rule in policy.workflow_rules {
        if rule.workflow_ref != workflow_ref
            || rule.job_workflow_ref != *job_workflow_ref
            || rule.workflow_path != run.path
            || rule.event != event
            || rule.head_branch != head_branch
        {
            continue;
        }
        match job_workflow_ref_is_root_or_caller(
            job_workflow_ref,
            rule.workflow_ref,
            referenced_workflows,
        ) {
            Some(true) => {}
            Some(false) => continue,
            None => {
                return Err(JobTrustEvidence::Unknown(
                    PolicyGap::ReusableWorkflowRefUnavailable,
                ));
            }
        }
        match reusable_workflows_match(rule.referenced_workflows, referenced_workflows) {
            Some(true) => {
                return Ok(OfferWorkflow {
                    job_workflow_ref,
                    workflow_ref,
                });
            }
            Some(false) => {}
            None => {
                return Err(JobTrustEvidence::Unknown(
                    PolicyGap::ReusableWorkflowRefUnavailable,
                ));
            }
        }
    }
    Err(JobTrustEvidence::Rejected(
        PolicyMismatch::WorkflowReferenceMismatch,
    ))
}

fn reusable_workflows_match(
    expected: &[ReusableWorkflowRuleView<'_>],
    actual: &[ReusableWorkflowEvidence],
) -> Option<bool> {
    if expected.len() != actual.len() {
        return Some(false);
    }
    for (expected, actual) in expected.iter().zip(actual) {
        let reference = match &actual.git_ref {
            WorkflowTrustField::Missing | WorkflowTrustField::Invalid => return None,
            WorkflowTrustField::Present(reference) => reference,
        };
        if expected.path != actual.path
            || expected.git_ref != reference
            || expected.sha != actual.sha
        {
            return Some(false);
        }
    }
    Some(true)
}

fn job_workflow_ref_is_root_or_caller(
    job_workflow_ref: &str,
    workflow_ref: &str,
    referenced_workflows: &[ReusableWorkflowEvidence],
) -> Option<bool> {
    let (job_file, job_ref) = job_workflow_ref.rsplit_once('@')?;
    let (root_file, _) = workflow_ref.rsplit_once('@')?;
    if job_file == root_file {
        // Root REST `path` and protocol `jobWorkflowRef` have distinct ref
        // renderings. The exact pair is pinned together in one reviewed rule;
        // only the repository/file identity is compared here.
        return Some(true);
    }

    let mut matching_path_missing_ref = false;
    for referenced in referenced_workflows {
        let (referenced_file, _display_ref) = referenced.path.rsplit_once('@')?;
        if referenced_file != job_file {
            continue;
        }
        match &referenced.git_ref {
            WorkflowTrustField::Missing | WorkflowTrustField::Invalid => {
                matching_path_missing_ref = true;
            }
            WorkflowTrustField::Present(reference) if reference == job_ref => return Some(true),
            WorkflowTrustField::Present(_) => {}
        }
    }
    if matching_path_missing_ref {
        None
    } else {
        Some(false)
    }
}

/// Match the legacy coarse file-path allowlist against the raw REST path.
/// The configured string must be an exact file-path prefix followed by `@`
/// and a nonempty returned ref; no path or ref is normalized or synthesized.
fn rest_path_matches_bare_workflow(allowed_path: &str, rest_path: &str) -> bool {
    rest_path
        .strip_prefix(allowed_path)
        .and_then(|suffix| suffix.strip_prefix('@'))
        .is_some_and(|reference| !reference.is_empty())
}
