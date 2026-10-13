/// Why one requested repository/run/attempt did not yield a complete provider
/// inventory. These outcomes carry no partial rows and cannot be used as a
/// positive census.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionsWorkflowAttemptEvidenceGap {
    /// The requested attempt exceeded the bounded supported range.
    AttemptLimitExceeded,
    /// GitHub returned 404 for the exact attempt metadata request.
    WorkflowAttemptNotFound,
    /// GitHub returned 404 for the exact attempt jobs listing.
    AttemptJobsNotFound,
    /// GitHub returned 404 for the run-level artifact listing.
    RunArtifactsNotFound,
    /// The first jobs page reported more rows than the bounded reader accepts.
    JobPageLimitExceeded,
    /// The first artifact page reported more rows than the bounded reader accepts.
    ArtifactPageLimitExceeded,
    /// Jobs pages changed total count or had a short/oversized page.
    InconsistentJobPages,
    /// Artifact pages changed total count or had a short/oversized page.
    InconsistentArtifactPages,
    /// An exact-attempt jobs listing repeated a REST job ID.
    DuplicateJobId,
    /// A run-level artifact listing repeated an artifact ID.
    DuplicateArtifactId,
    /// The response run ID did not match the requested run ID.
    RunIdMismatch,
    /// The response attempt did not match the requested attempt.
    AttemptMismatch,
    /// The base repository identity did not match the requested repository.
    RepositoryMismatch,
    /// The selected attempt's head SHA did not match the expected source SHA.
    HeadShaMismatch,
    /// A job row did not belong to the exact run/selected head SHA.
    JobIdentityMismatch,
    /// Artifact metadata did not bind to the exact run/repository/head SHA.
    ArtifactIdentityMismatch,
}

/// Exact workflow-attempt inventory read from GitHub Actions REST.
///
/// `jobs` belong to the requested attempt. `artifacts` are only run-scoped:
/// GitHub's artifact REST metadata has no attempt or job field. This struct
/// intentionally exposes no `ExecutionKey`, logical-job, plan, or profile
/// mapping; the provider endpoints do not supply those identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsWorkflowAttemptProviderEvidence {
    /// Numeric GitHub repository ID returned by the run response.
    pub repository_id: i64,
    /// Canonical repository full name returned by the run response.
    pub repository_full_name: String,
    /// Requested workflow run ID, confirmed by the run response and every row.
    pub workflow_run_id: i64,
    /// Requested attempt number, confirmed by the attempt response.
    pub attempt: u32,
    /// Selected attempt's exact source commit SHA.
    pub head_sha: String,
    /// Workflow path reported by the exact attempt response.
    pub workflow_path: String,
    /// Trigger event reported by the exact attempt response.
    pub event: String,
    /// Head branch when GitHub supplied it.
    pub head_branch: Option<String>,
    /// Source repository identity when GitHub supplied it. Absence is retained
    /// so a higher-level trust policy can fail closed.
    pub head_repository_id: Option<i64>,
    /// Source repository full name when GitHub supplied it.
    pub head_repository_full_name: Option<String>,
    /// Run-level lifecycle state.
    pub status: String,
    /// Run-level terminal conclusion, if any.
    pub conclusion: Option<String>,
    /// All complete job rows from the exact attempt endpoint.
    pub jobs: Vec<ActionsWorkflowAttemptJobEvidence>,
    /// All complete artifact rows from the run-level endpoint. These rows do
    /// not prove which attempt or job produced an artifact.
    pub artifacts: Vec<ActionsWorkflowRunArtifactEvidence>,
}

/// One GitHub job row from the requested run attempt.
///
/// `name` is GitHub's display name. It is not asserted to equal a planner's
/// logical-job, plan, or profile key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsWorkflowAttemptJobEvidence {
    /// Numeric Actions REST job ID.
    pub id: i64,
    /// Parent workflow run ID.
    pub run_id: i64,
    /// Provider job display name, not a logical-job mapping.
    pub name: String,
    /// Job commit SHA.
    pub head_sha: String,
    /// Current provider job status.
    pub status: String,
    /// Terminal conclusion when present.
    pub conclusion: Option<String>,
    /// Assigned runner ID when present. GitHub's `0` sentinel is normalized
    /// to `None`.
    pub runner_id: Option<i64>,
    /// Assigned runner name when present. GitHub's empty-string sentinel is
    /// normalized to `None`.
    pub runner_name: Option<String>,
    /// Runner group ID when present. GitHub's `0` sentinel is normalized to
    /// `None`.
    pub runner_group_id: Option<i64>,
    /// Runner group name when present. GitHub's empty-string sentinel is
    /// normalized to `None`.
    pub runner_group_name: Option<String>,
    /// Workflow display name when present.
    pub workflow_name: Option<String>,
    /// Job head branch when present.
    pub head_branch: Option<String>,
    /// Runner labels when present.
    pub labels: Option<Vec<String>>,
}

/// One artifact row linked by GitHub to a workflow run, not to an attempt/job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionsWorkflowRunArtifactEvidence {
    /// Numeric GitHub artifact ID.
    pub id: i64,
    /// Provider artifact name.
    pub name: String,
    /// Size in bytes.
    pub size_in_bytes: u64,
    /// Whether GitHub reports the artifact expired.
    pub expired: bool,
    /// Provider digest when returned.
    pub digest: Option<String>,
    /// Workflow run ID from the artifact's nested `workflow_run` metadata.
    pub workflow_run_id: i64,
    /// Base repository ID from nested workflow-run metadata.
    pub repository_id: i64,
    /// Source repository ID when returned in nested metadata.
    pub head_repository_id: Option<i64>,
    /// Source branch when returned in nested metadata.
    pub head_branch: Option<String>,
    /// Source SHA from nested workflow-run metadata.
    pub head_sha: String,
}

/// Complete raw provider inventory, or a fail-closed non-positive outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionsWorkflowAttemptProviderRead {
    /// Every exact-attempt job page and run-level artifact page was read and
    /// all exposed repository/run/attempt/SHA bindings matched.
    Complete(Box<ActionsWorkflowAttemptProviderEvidence>),
    /// The requested inventory is missing, mismatched, or beyond the bounded
    /// completeness limits. No partial evidence is returned.
    Unavailable(ActionsWorkflowAttemptEvidenceGap),
}
