//! Adapter from the bounded GitHub Actions attempt reader to compare ports.
//!
//! The adapter accepts only the reader's `Complete` variant. It validates the
//! requested repository/run/attempt/head and row identities again before
//! exposing the existing provider-view trait. `Complete` is a data shape, not
//! an authentication capability: callers must pass the result of
//! `read_actions_workflow_attempt_provider_evidence_async` using the bounded
//! host transport. Run-level artifacts remain run-scoped and are not claimed
//! to identify a particular job or attempt.

use std::collections::BTreeSet;

use velnor_actions_orchestrator_merge_ports::{
    ActionsAttemptArtifactView, ActionsAttemptJobView, CompleteActionsAttemptView,
    ScopedCompareRequest,
};
use velnor_runner_github::{
    ActionsWorkflowAttemptEvidenceGap, ActionsWorkflowAttemptJobEvidence,
    ActionsWorkflowAttemptProviderEvidence, ActionsWorkflowAttemptProviderRead,
    ActionsWorkflowRunArtifactEvidence,
};

/// Fail-closed outcome while adapting one exact provider read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionsAttemptAdapterError {
    /// The requested repository/run/attempt/head was malformed.
    InvalidRequestScope,
    /// The complete provider response did not match the requested scope.
    ProviderScopeMismatch,
    /// A provider job or artifact row had an invalid or duplicate identity.
    InvalidProviderInventory,
}

/// Complete provider view, or the bounded reader's non-positive result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionsAttemptAdapterOutcome {
    /// All bounded pages were accepted by the GitHub reader and scope checks.
    Complete(BoundedActionsAttemptView),
    /// The reader could not establish a complete inventory; no rows are kept.
    Unavailable(ActionsWorkflowAttemptEvidenceGap),
}

/// Read-only adapter implementing the provider view consumed by merge ports.
///
/// Its private field prevents callers from constructing a view directly from
/// a partial row collection. The public GH read enum is still forgeable, so
/// this is not a transport-authentication or source-attestation token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedActionsAttemptView {
    repository_id: i64,
    repository_full_name: String,
    workflow_run_id: i64,
    attempt: u32,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    jobs: Vec<BoundedActionsAttemptJob>,
    artifacts: Vec<BoundedActionsAttemptArtifact>,
}

/// One exact-attempt job row adapted for merge ports.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedActionsAttemptJob(ActionsWorkflowAttemptJobEvidence);

/// One run-scoped artifact row adapted for merge ports.
#[doc(hidden)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedActionsAttemptArtifact(ActionsWorkflowRunArtifactEvidence);

/// Validate a complete bounded-reader result and adapt it for the existing
/// scoped compare join.
///
/// Callers should pass the result of
/// `velnor_runner_github::read_actions_workflow_attempt_provider_evidence_async`
/// after binding `AsyncDiscoveryTransport` to GitHub's fixed API origin. Only
/// `Complete` can yield a provider view. `Unavailable` remains a gap and
/// carries no partial jobs or artifacts.
///
/// # Errors
///
/// Returns a typed error for invalid requested scope, provider scope mismatch,
/// or a malformed/ambiguous provider identity inventory.
pub fn adapt_actions_attempt_read(
    read: ActionsWorkflowAttemptProviderRead,
    request: ScopedCompareRequest<'_>,
    expected_head_sha: &str,
) -> Result<ActionsAttemptAdapterOutcome, ActionsAttemptAdapterError> {
    if !super::valid_repository(request.repository)
        || request.run_id <= 0
        || request.attempt == 0
        || !valid_sha(expected_head_sha)
    {
        return Err(ActionsAttemptAdapterError::InvalidRequestScope);
    }

    let evidence = match read {
        ActionsWorkflowAttemptProviderRead::Complete(evidence) => *evidence,
        ActionsWorkflowAttemptProviderRead::Unavailable(gap) => {
            return Ok(ActionsAttemptAdapterOutcome::Unavailable(gap));
        }
    };
    validate_scope(&evidence, request, expected_head_sha)?;
    validate_rows(&evidence)?;
    Ok(ActionsAttemptAdapterOutcome::Complete(adapt_evidence(
        evidence,
    )))
}

fn adapt_evidence(evidence: ActionsWorkflowAttemptProviderEvidence) -> BoundedActionsAttemptView {
    BoundedActionsAttemptView {
        repository_id: evidence.repository_id,
        repository_full_name: evidence.repository_full_name,
        workflow_run_id: evidence.workflow_run_id,
        attempt: evidence.attempt,
        head_sha: evidence.head_sha,
        status: evidence.status,
        conclusion: evidence.conclusion,
        jobs: evidence
            .jobs
            .into_iter()
            .map(BoundedActionsAttemptJob)
            .collect(),
        artifacts: evidence
            .artifacts
            .into_iter()
            .map(BoundedActionsAttemptArtifact)
            .collect(),
    }
}

impl CompleteActionsAttemptView for BoundedActionsAttemptView {
    type Job = BoundedActionsAttemptJob;
    type Artifact = BoundedActionsAttemptArtifact;

    fn repository_id(&self) -> i64 {
        self.repository_id
    }

    fn repository_full_name(&self) -> &str {
        &self.repository_full_name
    }

    fn workflow_run_id(&self) -> i64 {
        self.workflow_run_id
    }

    fn attempt(&self) -> u32 {
        self.attempt
    }

    fn head_sha(&self) -> &str {
        &self.head_sha
    }

    fn run_status(&self) -> &str {
        &self.status
    }

    fn run_conclusion(&self) -> Option<&str> {
        self.conclusion.as_deref()
    }

    fn jobs(&self) -> &[Self::Job] {
        &self.jobs
    }

    fn artifacts(&self) -> &[Self::Artifact] {
        &self.artifacts
    }
}

impl ActionsAttemptJobView for BoundedActionsAttemptJob {
    fn actions_job_id(&self) -> i64 {
        self.0.id
    }

    fn check_run_id(&self) -> Option<i64> {
        self.0.check_run_id
    }

    fn workflow_run_id(&self) -> i64 {
        self.0.run_id
    }

    fn head_sha(&self) -> &str {
        &self.0.head_sha
    }

    fn status(&self) -> &str {
        &self.0.status
    }

    fn conclusion(&self) -> Option<&str> {
        self.0.conclusion.as_deref()
    }
}

impl ActionsAttemptArtifactView for BoundedActionsAttemptArtifact {
    fn artifact_id(&self) -> i64 {
        self.0.id
    }

    fn artifact_name(&self) -> &str {
        &self.0.name
    }

    fn expired(&self) -> bool {
        self.0.expired
    }

    fn workflow_run_id(&self) -> i64 {
        self.0.workflow_run_id
    }

    fn repository_id(&self) -> i64 {
        self.0.repository_id
    }

    fn head_sha(&self) -> &str {
        &self.0.head_sha
    }
}

fn validate_scope(
    evidence: &ActionsWorkflowAttemptProviderEvidence,
    request: ScopedCompareRequest<'_>,
    expected_head_sha: &str,
) -> Result<(), ActionsAttemptAdapterError> {
    if evidence.repository_id <= 0
        || !evidence
            .repository_full_name
            .eq_ignore_ascii_case(request.repository)
        || evidence.workflow_run_id != request.run_id
        || evidence.attempt != request.attempt
        || evidence.head_sha != expected_head_sha
    {
        return Err(ActionsAttemptAdapterError::ProviderScopeMismatch);
    }
    Ok(())
}

fn validate_rows(
    evidence: &ActionsWorkflowAttemptProviderEvidence,
) -> Result<(), ActionsAttemptAdapterError> {
    let mut job_ids = BTreeSet::new();
    let mut check_run_ids = BTreeSet::new();
    for job in &evidence.jobs {
        if job.id <= 0
            || job.run_id != evidence.workflow_run_id
            || job.head_sha != evidence.head_sha
            || !job_ids.insert(job.id)
            || job
                .check_run_id
                .is_some_and(|id| id <= 0 || !check_run_ids.insert(id))
        {
            return Err(ActionsAttemptAdapterError::InvalidProviderInventory);
        }
    }

    let mut artifact_ids = BTreeSet::new();
    for artifact in &evidence.artifacts {
        if artifact.id <= 0
            || artifact.name.is_empty()
            || artifact.workflow_run_id != evidence.workflow_run_id
            || artifact.repository_id != evidence.repository_id
            || artifact.head_sha != evidence.head_sha
            || !artifact_ids.insert(artifact.id)
        {
            return Err(ActionsAttemptAdapterError::InvalidProviderInventory);
        }
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
