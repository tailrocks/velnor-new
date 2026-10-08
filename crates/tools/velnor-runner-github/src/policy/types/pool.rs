use std::time::SystemTime;

use super::workflow::WorkflowTrustField;

/// Resolved runner and private `DinD` image identity bound into a pool token.
///
/// These values are supplied by the validated host profile resolver. This
/// type records exact resolver metadata and the requalification deadline; it
/// does not independently prove image provenance or runtime executable
/// contents. `runner_release_version` is image metadata, not a runtime probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerImageIdentity {
    /// Stable host-resolved profile key.
    pub profile: String,
    /// Exact Scale Set selector paired with the profile.
    pub scale_set_name: String,
    /// OCI platform selected by the host resolver.
    pub platform: String,
    /// Exact immutable runner image reference.
    pub runner_image: String,
    /// Immutable platform-specific runner manifest digest.
    pub runner_manifest_digest: String,
    /// Immutable multi-platform runner index digest.
    pub runner_index_digest: String,
    /// Immutable runner image-config digest.
    pub runner_config_digest: String,
    /// Operating-system release metadata from the runner image.
    pub runner_os: String,
    /// Runner release version represented by the image metadata.
    pub runner_release_version: String,
    /// Upstream publication timestamp represented by the image profile.
    pub runner_release_published_at: String,
    /// Exact RFC3339 profile requalification deadline.
    pub runner_requalify_by: String,
    /// Exact private `DinD` image reference.
    pub dind_image: String,
    /// Immutable platform-specific `DinD` manifest digest.
    pub dind_manifest_digest: String,
    /// Immutable multi-platform `DinD` index digest.
    pub dind_index_digest: String,
    /// Immutable `DinD` image-config digest.
    pub dind_config_digest: String,
    /// Docker Engine release represented by the `DinD` image profile.
    pub dind_version: String,
    /// Immutable source revision for the `DinD` image entrypoint.
    pub dind_source: String,
    /// SHA-256 of the exact `DinD` entrypoint.
    pub dind_entrypoint_sha256: String,
}

/// Borrowed immutable image identity from one validated host profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerImageIdentityView<'a> {
    /// Stable host-resolved profile key.
    pub profile: &'a str,
    /// Exact Scale Set selector paired with the profile.
    pub scale_set_name: &'a str,
    /// OCI platform selected by the host resolver.
    pub platform: &'a str,
    /// Exact immutable runner image reference.
    pub runner_image: &'a str,
    /// Immutable platform-specific runner manifest digest.
    pub runner_manifest_digest: &'a str,
    /// Immutable multi-platform runner index digest.
    pub runner_index_digest: &'a str,
    /// Immutable runner image-config digest.
    pub runner_config_digest: &'a str,
    /// Operating-system release metadata from the runner image.
    pub runner_os: &'a str,
    /// Runner release version represented by the image metadata.
    pub runner_release_version: &'a str,
    /// Upstream publication timestamp represented by the image profile.
    pub runner_release_published_at: &'a str,
    /// Exact RFC3339 profile requalification deadline.
    pub runner_requalify_by: &'a str,
    /// Exact private `DinD` image reference.
    pub dind_image: &'a str,
    /// Immutable platform-specific `DinD` manifest digest.
    pub dind_manifest_digest: &'a str,
    /// Immutable multi-platform `DinD` index digest.
    pub dind_index_digest: &'a str,
    /// Immutable `DinD` image-config digest.
    pub dind_config_digest: &'a str,
    /// Docker Engine release represented by the `DinD` image profile.
    pub dind_version: &'a str,
    /// Immutable source revision for the `DinD` image entrypoint.
    pub dind_source: &'a str,
    /// SHA-256 of the exact `DinD` entrypoint.
    pub dind_entrypoint_sha256: &'a str,
}

impl From<RunnerImageIdentityView<'_>> for RunnerImageIdentity {
    fn from(view: RunnerImageIdentityView<'_>) -> Self {
        Self {
            profile: view.profile.to_owned(),
            scale_set_name: view.scale_set_name.to_owned(),
            platform: view.platform.to_owned(),
            runner_image: view.runner_image.to_owned(),
            runner_manifest_digest: view.runner_manifest_digest.to_owned(),
            runner_index_digest: view.runner_index_digest.to_owned(),
            runner_config_digest: view.runner_config_digest.to_owned(),
            runner_os: view.runner_os.to_owned(),
            runner_release_version: view.runner_release_version.to_owned(),
            runner_release_published_at: view.runner_release_published_at.to_owned(),
            runner_requalify_by: view.runner_requalify_by.to_owned(),
            dind_image: view.dind_image.to_owned(),
            dind_manifest_digest: view.dind_manifest_digest.to_owned(),
            dind_index_digest: view.dind_index_digest.to_owned(),
            dind_config_digest: view.dind_config_digest.to_owned(),
            dind_version: view.dind_version.to_owned(),
            dind_source: view.dind_source.to_owned(),
            dind_entrypoint_sha256: view.dind_entrypoint_sha256.to_owned(),
        }
    }
}

/// Registration scope that owns the Actions Service Scale Set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolRegistrationScope {
    /// Repository registration scope; this cannot use an organization REST
    /// runner-group policy as an effective-route proof.
    Repository {
        /// Exact owner in the registration API scope.
        owner: String,
        /// Exact repository in the registration API scope.
        repository: String,
    },
    /// Organization registration scope.
    Organization {
        /// Exact organization login in the registration API scope.
        organization: String,
    },
}

/// Borrowed registration scope from one validated host configuration snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolRegistrationScopeView<'a> {
    /// Repository registration scope.
    Repository {
        /// Exact owner in the validated registration configuration.
        owner: &'a str,
        /// Exact repository in the validated registration configuration.
        repository: &'a str,
    },
    /// Organization registration scope.
    Organization {
        /// Exact organization login in the validated registration configuration.
        organization: &'a str,
    },
}

/// Exact Scale Set identity and the distinct REST/runtime group identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolBinding {
    /// Registration scope that produced the Actions Service connection.
    pub registration_scope: PoolRegistrationScope,
    /// Must be positive.
    pub repository_id: i64,
    /// Exact `owner/repository` name.
    pub repository_full_name: String,
    /// Must be positive.
    pub scale_set_id: i64,
    /// Exact set name.
    pub scale_set_name: String,
    /// Actions Service group ID returned inside the registration scope.
    pub actions_runner_group_id: i64,
    /// Exact Actions Service group name.
    pub actions_runner_group_name: String,
    /// REST group ID, retained separately from the Actions Service ID.
    pub rest_runner_group_id: Option<i64>,
    /// Host-resolved runner image profile key.
    pub runner_image_profile: Option<String>,
    /// Complete image identity from the host's validated profile resolver.
    pub runner_image: Option<RunnerImageIdentity>,
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
    /// Registration scope from this exact validated config snapshot.
    pub registration_scope: PoolRegistrationScopeView<'a>,
    /// Expected target repository ID, if already pinned. `None` allows the
    /// bounded reader to discover and then bind the immutable ID.
    pub target_repository_id: Option<i64>,
    /// Exact `owner/repository` target name.
    pub target_repository_full_name: &'a str,
    /// Expected immutable Scale Set ID when already known; `None` is allowed
    /// only while performing initial read-only discovery.
    pub scale_set_id: Option<i64>,
    /// Exact Scale Set name.
    pub scale_set_name: &'a str,
    /// Exact positive Actions Service runner-group ID.
    pub actions_runner_group_id: i64,
    /// Exact Actions Service runner-group name.
    pub actions_runner_group_name: &'a str,
    /// Expected REST group ID when already pinned; this is not the Actions
    /// Service group ID. `None` is permitted only during initial discovery.
    pub rest_runner_group_id: Option<i64>,
    /// Complete immutable runner/DinD identity from the validated host profile.
    /// Organization pool proof requires this value; repository-scope paths may
    /// omit it while remaining non-admissible.
    pub runner_image: Option<RunnerImageIdentityView<'a>>,
    /// Exact REST group workflow identities to enforce at the scope boundary.
    pub allowed_group_workflows: &'a [String],
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
    /// The resolved runner profile has reached its requalification deadline.
    StaleImageProfile,
    /// No source-verified Ubuntu 26.04 runner profile is available for new
    /// Linux admission.
    RequiredRunnerProfileUnavailable,
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
