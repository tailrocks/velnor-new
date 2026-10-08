//! Velnor repository configuration: `.velnor/config.toml` schema and validation.
//!
//! Owns the stack-neutral configuration surface (workflow, execution,
//! discovery, resources, runners, stacks, verification) plus validator
//! selection. Must not own identities, release manifests, detection,
//! proposals, graphs, or workflow IR. Built on
//! `velnor-actions-contract` (identifiers) and
//! `velnor-actions-contract-release` (targets, runner labels).

pub mod config;
pub mod named_check;

pub use config::{
    ArtifactBuildOutput, ArtifactBuildTask, CheckEvidence, CheckExecutor, CheckPlatform,
    CheckRunner, CheckSystemTool, CheckSystemToolKind, ContainerPlatform, DaemonIdentityPolicy,
    DeclaredCompileDriver, DeclaredTestRunner, DiscoveryConfig, DocsLaneConfig, ExecutionConfig,
    ExecutionMode, ExecutionOverride, ExecutionParity, ExecutionProfile, ExecutionRole,
    GeneratorValidation, HOSTED_PROFILE_ID, HostContainerProfile, HostDockerCli, HostDockerDaemon,
    HostOrbStackSdk, MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES, MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES, MAX_CHECK_CONTAINER_PATH_BYTES,
    MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES, MAX_CHECK_CONTAINER_RUNTIME_ENTRIES,
    MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES, MAX_CHECK_EXECUTION_RECEIPT_BYTES,
    MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES, MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES,
    MAX_CHECK_SOURCE_BYTES, MAX_CHECK_SYSTEM_VERSION_BYTES, MiseCheck, ProfileKind,
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend,
    QualifiedToolExecutable, QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
    ResourcesConfig, RootProblem, RoutingWorkflow, RunnerSelection, RunsOn, RustConfiguration,
    RustPolicyConfig, RustPolicyProfile, RustStackConfig, SCALE_SET_NAME, SCALE_SET_PROFILE_ID,
    ScaleSetSelector, ShardTimingEvidence, StacksConfig, TestShardingConfig, TofuStackConfig,
    Utf8RepoRelDir, VELNOR_LABEL, VERIFICATION_TASK_JOB_PREFIX, ValidatorKind, VelnorConfig,
    VelnorSupportWorkflow, VerificationRunner, VerificationTask, VerificationTaskKind,
    WorkflowConfig, WorkflowPolicy, check_execution_receipt_upper_bound, is_valid_feature_name,
    is_valid_mise_task_name, is_valid_rust_target, is_valid_verification_task_id,
    validate_qualified_tools, validate_shard_changes_need_evidence,
};
pub use named_check::{NamedCheckIdentityExtension, validate_named_check_extension};
