//! Pure, fail-closed identity join for selected named-check lanes.
//!
//! This returns a binding map, not a parity verdict. It does not establish
//! that the Actions reader authenticated its source, that the producer-key
//! census is complete, or that a runner matched the plan's lane. Implementors
//! of [`CompleteActionsAttemptView`] must adapt only the provider reader's
//! `Complete` result; the trait itself is intentionally not an authority
//! token. In particular, this module never infers identities from display
//! names or runner names.

/// Read-only view of one exact-attempt Actions REST job row.
pub trait ActionsAttemptJobView {
    /// Numeric Actions workflow-job ID, distinct from the Check Run ID.
    fn actions_job_id(&self) -> i64;
    /// Check Run ID parsed from the repository-scoped `check_run_url`.
    fn check_run_id(&self) -> Option<i64>;
    /// Parent workflow run ID.
    fn workflow_run_id(&self) -> i64;
    /// Job source SHA.
    fn head_sha(&self) -> &str;
    /// Provider job state (`completed` is required for a selected lane).
    fn status(&self) -> &str;
    /// Provider conclusion (`success` is required for a selected lane).
    fn conclusion(&self) -> Option<&str>;
}

/// Read-only view of one run-level artifact REST row.
pub trait ActionsAttemptArtifactView {
    /// Numeric upload-artifact ID.
    fn artifact_id(&self) -> i64;
    /// Exact upload artifact name.
    fn artifact_name(&self) -> &str;
    /// Whether GitHub reports this artifact expired.
    fn expired(&self) -> bool;
    /// Parent workflow run ID from nested artifact metadata.
    fn workflow_run_id(&self) -> i64;
    /// Repository ID from nested artifact metadata.
    fn repository_id(&self) -> i64;
    /// Source SHA from nested artifact metadata.
    fn head_sha(&self) -> &str;
}

/// View of a complete Actions attempt inventory.
///
/// The production adapter must implement this only for
/// `ActionsWorkflowAttemptProviderRead::Complete`; `Unavailable` has no
/// partial-data path into this join. This trait does not itself attest to that
/// condition, and callers must not treat the result as provider-authenticated
/// or graph-complete evidence.
pub trait CompleteActionsAttemptView {
    /// Job-row view type.
    type Job: ActionsAttemptJobView;
    /// Artifact-row view type.
    type Artifact: ActionsAttemptArtifactView;

    /// Numeric repository ID returned by GitHub.
    fn repository_id(&self) -> i64;
    /// Canonical `owner/repository` full name returned by GitHub.
    fn repository_full_name(&self) -> &str;
    /// Workflow run ID.
    fn workflow_run_id(&self) -> i64;
    /// Attempt number.
    fn attempt(&self) -> u32;
    /// Selected attempt's source SHA.
    fn head_sha(&self) -> &str;
    /// Workflow-run status.
    fn run_status(&self) -> &str;
    /// Workflow-run conclusion when terminal.
    fn run_conclusion(&self) -> Option<&str>;
    /// All job rows returned for the exact attempt.
    fn jobs(&self) -> &[Self::Job];
    /// All run-level artifact rows returned for the workflow run.
    fn artifacts(&self) -> &[Self::Artifact];
}

/// Requested repository and attempt scope.
#[derive(Debug, Clone, Copy)]
pub struct ScopedCompareRequest<'a> {
    /// Requested `owner/repository` slug.
    pub repository: &'a str,
    /// Positive GitHub workflow run ID.
    pub run_id: i64,
    /// Positive workflow run attempt.
    pub attempt: u32,
}

/// Declared lane identity copied from the authoritative plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopedCompareLane {
    /// The plan's GitHub-hosted lane.
    Hosted,
    /// The plan's Velnor Scale Set lane.
    ScaleSet,
}

/// One exact plan lane joined to its runtime receipt, output IDs, and REST rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCompareLaneBinding {
    /// Plan matrix-entry ID.
    pub matrix_id: String,
    /// Plan matrix key.
    pub matrix_key: String,
    /// Stable logical task ID from the plan, not a provider display name.
    pub task_id: String,
    /// Exact workflow job key from the plan and runtime receipt.
    pub workflow_job_key: String,
    /// Lane declared by the typed `MatrixEntry::lane_variant`.
    pub lane: ScopedCompareLane,
    /// Runtime receipt's existing task-report ID.
    pub task_report_id: String,
    /// Exact report artifact name declared by the plan.
    pub artifact_name: String,
    /// Numeric upload-artifact ID emitted by the job.
    pub artifact_id: i64,
    /// Numeric Check Run ID emitted by the job and returned in `check_run_url`.
    pub check_run_id: i64,
    /// Numeric Actions REST workflow-job ID, kept distinct from Check Run ID.
    pub actions_job_id: i64,
}

/// Scoped identity bindings. This is not a parity proof or runner-placement proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedCompareResult {
    /// Numeric GitHub repository ID returned by the provider.
    pub repository_id: i64,
    /// Normalized repository slug.
    pub repository: String,
    /// Workflow run ID.
    pub run_id: i64,
    /// Attempt number.
    pub attempt: u32,
    /// Plan and provider source SHA.
    pub head_sha: String,
    /// Canonical digest of the validated plan.
    pub plan_digest: String,
    /// Paired named-check bindings in plan order.
    pub lanes: Vec<ScopedCompareLaneBinding>,
}

/// Fail-closed reason a scoped binding could not be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopedCompareError {
    /// Request repository/run/attempt was malformed.
    InvalidRequest,
    /// The plan failed validation or had an invalid head SHA.
    InvalidPlan,
    /// Plan run key differs from the requested run and attempt.
    PlanScopeMismatch,
    /// Producer output sidecar failed its internal contract.
    InvalidProducerOutputs,
    /// Sidecar scope, head, or plan digest differed from the plan/provider.
    ProducerScopeMismatch,
    /// No typed hosted/Scale Set lane exists in the plan.
    NoTypedLanes,
    /// A receipt was invalid, duplicated, or did not match a plan entry/scope.
    InvalidReceipt,
    /// A required lane receipt or producer output was absent.
    MissingLaneInput,
    /// Provider run/repository/attempt/head did not match the request.
    ProviderScopeMismatch,
    /// Provider run was not completed successfully.
    ProviderRunUnsuccessful,
    /// Provider rows had invalid identities or duplicate IDs.
    InvalidProviderInventory,
    /// No unique provider job matched the emitted Check Run ID.
    JobBindingMismatch,
    /// A mapped provider job had not completed successfully.
    JobUnsuccessful,
    /// No unique provider artifact matched the emitted artifact ID.
    ArtifactBindingMismatch,
}
