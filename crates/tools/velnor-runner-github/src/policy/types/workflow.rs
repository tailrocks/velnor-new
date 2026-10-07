/// Whether a trust-relevant wire field was present and valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowTrustField<T> {
    /// The response omitted the field or returned JSON `null`.
    Missing,
    /// The field was present with a validated value.
    Present(T),
    /// The field was present but had an invalid type or value.
    Invalid,
}
/// Exact selected workflow definition allowed for one trust rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReusableWorkflowRuleView<'a> {
    /// Literal REST `referenced_workflows[].path`, including owner/repository
    /// and the `@ref` suffix.
    pub path: &'a str,
    /// Literal REST `referenced_workflows[].ref` value.
    pub git_ref: &'a str,
    /// Literal REST `referenced_workflows[].sha` value.
    pub sha: &'a str,
}

/// One allowed tuple connecting the root workflow, Scale Set job workflow,
/// triggering event, source branch, and complete reusable-workflow chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobTrustRuleView<'a> {
    /// Full root workflow reference: `<owner>/<repo>/<REST path>`.
    pub workflow_ref: &'a str,
    /// Exact `JobMessageBase.jobWorkflowRef` value.
    pub job_workflow_ref: &'a str,
    /// Literal workflow-run REST `path`, including its `@ref` suffix.
    pub workflow_path: &'a str,
    /// Exact Actions workflow-run `event` value.
    pub event: &'a str,
    /// Exact Actions workflow-run `head_branch` value.
    pub head_branch: &'a str,
    /// Ordered allowlist of every workflow reference returned by the run API.
    pub referenced_workflows: &'a [ReusableWorkflowRuleView<'a>],
}

/// Borrowed view of the host-owned trust configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobTrustPolicyView<'a> {
    /// Configured base repository, in `owner/repository` form.
    pub repository_full_name: &'a str,
    /// Existing coarse repository allowlist. Linux policy requires exactly the
    /// configured base repository when forks are disabled.
    pub allowed_repositories: &'a [String],
    /// Existing coarse event allowlist.
    pub allowed_events: &'a [String],
    /// Existing coarse workflow-path allowlist.
    pub allowed_workflow_paths: &'a [String],
    /// Exact source branch allowlist.
    pub allowed_head_branches: &'a [String],
    /// Exact full workflow tuples. Empty is an invalid policy.
    pub workflow_rules: &'a [JobTrustRuleView<'a>],
    /// Fork jobs are unsupported by this verifier and fail closed.
    pub allow_forks: bool,
    /// Stable digest of the canonical host policy, computed by its owner.
    pub policy_digest: &'a str,
}

/// One REST reusable-workflow reference. The `ref` property may be absent for
/// SHA-pinned workflow references; absence remains explicit and cannot match a
/// policy row that requires a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReusableWorkflowEvidence {
    pub(crate) path: String,
    pub(crate) git_ref: WorkflowTrustField<String>,
    pub(crate) sha: String,
}

impl ReusableWorkflowEvidence {
    /// Exact REST `path` value, including repository and any `@ref` suffix.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact REST `ref`, or missing/invalid evidence.
    #[must_use]
    pub const fn git_ref(&self) -> &WorkflowTrustField<String> {
        &self.git_ref
    }

    /// Exact REST SHA.
    #[must_use]
    pub fn sha(&self) -> &str {
        &self.sha
    }
}

/// Parsed trust-relevant portion of one Actions workflow-run REST response.
/// Fields are private so production callers receive this only from the
/// read-only parser rather than constructing a verification token from an
/// arbitrary struct literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsWorkflowTrustRun {
    pub(crate) id: i64,
    pub(crate) observed_run_attempt: i64,
    pub(crate) event: String,
    pub(crate) path: String,
    pub(crate) head_sha: String,
    pub(crate) head_branch: WorkflowTrustField<String>,
    pub(crate) head_repository_full_name: WorkflowTrustField<String>,
    pub(crate) referenced_workflows: WorkflowTrustField<Vec<ReusableWorkflowEvidence>>,
}

impl ActionsWorkflowTrustRun {
    /// Workflow run ID.
    #[must_use]
    pub const fn id(&self) -> i64 {
        self.id
    }

    /// Attempt number currently reported by the workflow-run REST resource.
    /// It is not correlated with a Scale Set job message by this DTO.
    #[must_use]
    pub const fn observed_run_attempt(&self) -> i64 {
        self.observed_run_attempt
    }

    /// Exact Actions event name.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// Literal Actions REST workflow path, including `@ref` when returned.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Exact source commit SHA.
    #[must_use]
    pub fn head_sha(&self) -> &str {
        &self.head_sha
    }

    /// Exact head branch string, or missing/invalid evidence.
    #[must_use]
    pub const fn head_branch(&self) -> &WorkflowTrustField<String> {
        &self.head_branch
    }

    /// Source repository reported by the run API, or missing/invalid evidence.
    #[must_use]
    pub const fn head_repository_full_name(&self) -> &WorkflowTrustField<String> {
        &self.head_repository_full_name
    }

    /// Complete ordered reusable-workflow chain, or missing/invalid evidence.
    #[must_use]
    pub const fn referenced_workflows(&self) -> &WorkflowTrustField<Vec<ReusableWorkflowEvidence>> {
        &self.referenced_workflows
    }
}

/// Read-only Actions REST result used for trust evaluation. The root workflow
/// reference is compared by prefixing the configured base repository to the
/// literal REST `path`; `head_branch` is never used to synthesize that ref.
#[must_use]
pub fn workflow_ref_for_repository(repository_full_name: &str, rest_path: &str) -> String {
    format!("{repository_full_name}/{rest_path}")
}

/// Fetch one workflow run and retain all fields needed for trust policy.
///
/// This uses only the Actions REST GET endpoint. It neither polls a Scale Set
/// session nor creates or changes a runner resource.
///
/// # Errors
///
/// Returns an input, transport, authorization, status, or malformed-response
/// error without exposing response bodies.
pub fn get_actions_workflow_trust_run<T>(
    transport: &mut T,
    owner: &str,
    repository: &str,
    workflow_run_id: i64,
    actions_token: &str,
) -> Result<ActionsWorkflowTrustRun, crate::SessionError>
where
    T: crate::Transport + ?Sized,
{
    crate::actions::trust::get_actions_workflow_trust_run(
        transport,
        owner,
        repository,
        workflow_run_id,
        actions_token,
    )
}
