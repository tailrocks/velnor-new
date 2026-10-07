use std::time::SystemTime;

use super::workflow::WorkflowTrustField;

/// Repository-scoped set identity expected by the controller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolBinding {
    /// Must be positive.
    pub repository_id: i64,
    /// Exact `owner/repository` name.
    pub repository_full_name: String,
    /// Must be positive.
    pub scale_set_id: i64,
    /// Exact set name.
    pub scale_set_name: String,
    /// Runner group ID at the registered scope.
    pub runner_group_id: i64,
    /// Exact runner group name.
    pub runner_group_name: String,
    /// Digest of the policy expected for this pool.
    pub policy_digest: String,
}

/// Bounded metadata snapshot. These fields can establish a mismatch or
/// consistency; they cannot by themselves prove that a repository-scoped set
/// is governed by an organization group's effective workflow restrictions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolPolicySnapshot {
    /// Binding reported by the source-specific readers.
    pub binding: WorkflowTrustField<PoolBinding>,
    /// Repository privacy status.
    pub repository_private: WorkflowTrustField<bool>,
    /// Whether private-repository fork workflows are disabled.
    pub forks_disabled: WorkflowTrustField<bool>,
    /// Group visibility setting, such as `selected` or `all`.
    pub group_visibility: WorkflowTrustField<String>,
    /// Whether public repositories are allowed by the group.
    pub allows_public_repositories: WorkflowTrustField<bool>,
    /// Whether workflow restrictions are enabled by the group.
    pub workflow_restrictions_enabled: WorkflowTrustField<bool>,
    /// Complete list of selected repository IDs, if the bounded read proved
    /// pagination complete.
    pub selected_repository_ids: WorkflowTrustField<Vec<i64>>,
    /// Complete selected workflow identities.
    pub selected_workflows: WorkflowTrustField<Vec<String>>,
    /// Whether every required metadata page was read and reconciled.
    pub pages_complete: bool,
    /// Source versions and times for the independent reads.
    pub sources: Vec<PoolEvidenceSourceStamp>,
}

/// Source and observation time for one read-only policy input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolEvidenceSourceStamp {
    /// Which authenticated read produced the evidence.
    pub source: PoolEvidenceSource,
    /// Collection time in UTC/system time.
    pub observed_at: SystemTime,
    /// API version or source revision used by the reader.
    pub source_version: String,
}

/// Supported read sources used to describe the policy snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolEvidenceSource {
    /// Repository Actions metadata GET.
    RepositoryMetadataRest,
    /// Repository fork-workflow settings GET.
    RepositoryForkPolicyRest,
    /// Scale Set service view.
    ScaleSetServiceRest,
    /// Organization runner-group metadata/policy GET.
    OrganizationRunnerGroupRest,
}

/// Binding borrowed from validated host configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolBindingView<'a> {
    /// Exact positive GitHub repository ID.
    pub repository_id: i64,
    /// Exact `owner/repository` name.
    pub repository_full_name: &'a str,
    /// Exact positive Scale Set ID.
    pub scale_set_id: i64,
    /// Exact Scale Set name.
    pub scale_set_name: &'a str,
    /// Exact positive runner group ID.
    pub runner_group_id: i64,
    /// Exact runner group name.
    pub runner_group_name: &'a str,
    /// Digest of the canonical host trust policy.
    pub policy_digest: &'a str,
}

/// Why verification could not establish a positive proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyGap {
    /// A required trust field was omitted.
    MissingField,
    /// A required field had an unsupported or malformed shape.
    InvalidField,
    /// The event does not provide a usable request/run identity.
    MissingEventIdentity,
    /// The requested event index does not exist in the immutable parsed batch.
    InvalidEventIndex,
    /// No exact workflow tuple exists in the configured rules.
    WorkflowRuleSetEmpty,
    /// A policy page was missing, truncated, duplicated, or inconsistent.
    IncompletePolicyRead,
    /// Metadata matches, but no source proves policy enforcement applies to
    /// this exact repository-scoped Scale Set.
    EffectiveRoutingApplicabilityUnproven,
    /// The evidence is older than the bounded policy freshness interval.
    StaleEvidence,
    /// A supported source does not report the required reusable-workflow ref.
    ReusableWorkflowRefUnavailable,
    /// Fork allowance is not supported by this host trust profile.
    ForkPolicyUnsupported,
}

/// Why present evidence disagrees with a configured policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyMismatch {
    /// Repository, set, or group identity disagrees.
    PoolBindingMismatch,
    /// The repository is not private.
    RepositoryNotPrivate,
    /// Fork workflows are not disabled.
    ForkWorkflowsEnabled,
    /// The runner group includes public repositories.
    PublicRepositoriesAllowed,
    /// The group routes to all repositories or lacks selected-repository
    /// restriction.
    RepositoryRoutingUnrestricted,
    /// Workflow restriction is absent or a selected workflow differs.
    WorkflowRoutingMismatch,
    /// The event's repository or event disagrees with policy.
    EventRepositoryMismatch,
    /// The Actions run ID disagrees with the Scale Set message.
    WorkflowRunMismatch,
    /// The source repository is outside the no-forks policy.
    ForkSourceMismatch,
    /// The root workflow reference, job workflow ref, source branch, or caller
    /// chain differs from the configured exact rule.
    WorkflowReferenceMismatch,
    /// The host policy is empty, contradictory, or has invalid identifiers.
    InvalidPolicy,
    /// This message kind is not a job offer or assignment with trust fields.
    UnsupportedMessageKind,
}
