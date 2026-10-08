use super::super::types::{PolicyGap, PolicyMismatch};

/// Result of checking a Scale Set event against the configured trust tuple.
#[derive(Debug, PartialEq, Eq)]
pub enum JobTrustEvidence {
    /// This event's supplied identity and workflow fields match one exact
    /// configured rule. This does not authorize Acquire/JIT by itself.
    Verified(Box<VerifiedJobTrust>),
    /// Required evidence is absent, malformed, or outside the supported
    /// source contract.
    Unknown(PolicyGap),
    /// Present evidence contradicts the configured policy.
    Rejected(PolicyMismatch),
}

/// Private proof that one delivered event matches one configured rule.
///
/// This type is neither `Clone` nor publicly constructible. It says nothing
/// about which runner GitHub assigns to the job. It also omits the current
/// Actions run attempt because the Scale Set message has no attempt field and
/// this verifier does not map its opaque job ID to an exact attempt.
#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedJobTrust {
    pub(super) message_id: i64,
    pub(super) request_id: i64,
    pub(super) source_session_id: Option<String>,
    pub(super) source_scale_set_id: Option<i64>,
    pub(super) scale_set_job_id: Option<String>,
    pub(super) workflow_run_id: i64,
    pub(super) repository_full_name: String,
    pub(super) head_repository_full_name: String,
    pub(super) event: String,
    pub(super) workflow_ref: String,
    pub(super) job_workflow_ref: String,
    pub(super) head_branch: String,
    pub(super) head_sha: String,
    pub(super) policy_digest: String,
}

impl VerifiedJobTrust {
    /// Session that delivered the event when the batch came from the opaque
    /// verified session driver. Trust checks on standalone parsed fixtures do
    /// not carry session provenance.
    #[must_use]
    pub fn source_session_id(&self) -> Option<&str> {
        self.source_session_id.as_deref()
    }

    /// Actions Service Set that delivered the event, when session-bound.
    #[must_use]
    pub const fn source_scale_set_id(&self) -> Option<i64> {
        self.source_scale_set_id
    }

    /// Scale Set delivery identifier, not a job or runner ID.
    #[must_use]
    pub const fn message_id(&self) -> i64 {
        self.message_id
    }

    /// Scale Set runner request ID, not a workflow job or runner ID.
    #[must_use]
    pub const fn runner_request_id(&self) -> i64 {
        self.request_id
    }

    /// Original opaque Scale Set `jobId`, if supplied.
    #[must_use]
    pub fn scale_set_job_id(&self) -> Option<&str> {
        self.scale_set_job_id.as_deref()
    }

    /// Workflow run ID from the Scale Set message and REST response.
    #[must_use]
    pub const fn workflow_run_id(&self) -> i64 {
        self.workflow_run_id
    }

    /// Exact base repository identity.
    #[must_use]
    pub fn repository_full_name(&self) -> &str {
        &self.repository_full_name
    }

    /// Exact source repository identity.
    #[must_use]
    pub fn head_repository_full_name(&self) -> &str {
        &self.head_repository_full_name
    }

    /// Exact Actions event name.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// Full root workflow reference matched to the bound repository.
    #[must_use]
    pub fn workflow_ref(&self) -> &str {
        &self.workflow_ref
    }

    /// Exact Scale Set `jobWorkflowRef` value.
    #[must_use]
    pub fn job_workflow_ref(&self) -> &str {
        &self.job_workflow_ref
    }

    /// Exact Actions run head branch.
    #[must_use]
    pub fn head_branch(&self) -> &str {
        &self.head_branch
    }

    /// Exact source commit SHA from the run API.
    #[must_use]
    pub fn head_sha(&self) -> &str {
        &self.head_sha
    }

    /// Digest of the host policy used for this check.
    #[must_use]
    pub fn policy_digest(&self) -> &str {
        &self.policy_digest
    }
}
