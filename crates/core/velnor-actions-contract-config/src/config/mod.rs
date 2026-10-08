//! `.velnor/config.toml` schema types (TOML shape, serde only).
//!
//! Unknown fields are rejected; validation reports file, key path, problem.

mod actions;
mod artifact_build;
mod check_receipt_budget;
mod discovery;
mod docs_lane;
mod execution;
mod host_container;
mod mise;
mod qualified_tools;
mod release;
mod resources;
mod runs_on;
mod rust_policy;
mod stacks;
mod tofu;
mod verification;
mod workflow;

pub use actions::{ActionPinOverride, ActionsConfig, OVERRIDABLE_ACTIONS};
pub use artifact_build::{ArtifactBuildOutput, ArtifactBuildTask};
pub use check_receipt_budget::{
    MAX_CHECK_CONTAINER_APP_INFO_CAPTURE_BYTES, MAX_CHECK_CONTAINER_APP_VERIFY_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_DAEMON_CAPTURE_BYTES, MAX_CHECK_CONTAINER_IDENTITY_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_PATH_BYTES, MAX_CHECK_CONTAINER_PROBE_CAPTURE_BYTES,
    MAX_CHECK_CONTAINER_RUNTIME_ENTRIES, MAX_CHECK_CONTAINER_RUNTIME_ENTRY_PATH_BYTES,
    MAX_CHECK_EXECUTION_RECEIPT_BYTES, MAX_CHECK_QUALIFIED_PROBE_CAPTURE_BYTES,
    MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES, MAX_CHECK_SOURCE_BYTES,
    MAX_CHECK_SYSTEM_VERSION_BYTES, check_execution_receipt_upper_bound,
};
pub use discovery::DiscoveryConfig;
pub use docs_lane::DocsLaneConfig;
pub use execution::{
    ExecutionConfig, ExecutionMode, ExecutionOverride, ExecutionParity, ExecutionProfile,
    ExecutionRole, HOSTED_PROFILE_ID, ProfileKind, RoutingWorkflow, SCALE_SET_PROFILE_ID,
};
pub use host_container::{
    ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli, HostDockerDaemon,
    HostOrbStackSdk,
};
pub use mise::{
    CheckEvidence, CheckExecutor, CheckPlatform, CheckRunner, CheckSystemTool, CheckSystemToolKind,
    MiseCheck, is_valid_mise_task_name,
};
pub use qualified_tools::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolArtifact, QualifiedToolBackend,
    QualifiedToolExecutable, QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe,
    validate_qualified_tools,
};
pub use release::{BootstrapRelease, ReleaseAuthentication, RustReleaseConfig};
pub use resources::{
    ResourcesConfig, ShardTimingEvidence, TestShardingConfig, validate_shard_changes_need_evidence,
};
pub use runs_on::{
    LINUX_AMD64, RunsOn, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL, is_hosted_catalog,
    is_legacy_hosted_label,
};
pub use rust_policy::{RustPolicyConfig, RustPolicyProfile};
pub use stacks::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustStackConfig, StacksConfig,
    is_valid_feature_name, is_valid_rust_target,
};
pub use tofu::{RootProblem, TofuStackConfig, Utf8RepoRelDir};
pub use verification::{
    VERIFICATION_TASK_JOB_PREFIX, VerificationRunner, VerificationTask, VerificationTaskKind,
    is_valid_verification_task_id,
};
pub use workflow::{
    GeneratorValidation, RunnerSelection, ValidatorKind, VelnorSupportWorkflow, VerifyConfig,
    WorkflowConfig, WorkflowPolicy,
};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

/// Mandatory job admission for external ephemeral check runners.
/// GitHub evaluates job conditions before allocating a runner. Unknown
/// event classes and fork PRs cannot enter this execution environment.
pub const EPHEMERAL_CHECK_ADMISSION_CONDITION: &str = "github.event_name == 'push' || github.event_name == 'merge_group' || (github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository)";

/// Top-level `.velnor/config.toml` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VelnorConfig {
    /// Config schema version. `1` is hosted-only. `2` is routing.
    pub schema: u32,
    /// Workflow policy and naming.
    pub workflow: WorkflowConfig,
    /// Process budgets.
    pub resources: ResourcesConfig,
    /// Test sharding policy.
    pub test_sharding: TestShardingConfig,
    /// Stack selection.
    pub stacks: StacksConfig,
    /// Discovery exclusions.
    pub discovery: DiscoveryConfig,
    /// Action-pin overrides; absent means bundled latest pins.
    #[serde(default)]
    pub actions: ActionsConfig,
    /// Schema 2 routing. Absent on schema 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionConfig>,
    /// Explicit repository-owned checks independent of language stacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<MiseCheck>,
    /// Explicit check-scoped tool qualification registry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualified_tools: Vec<QualifiedTool>,
    /// Docs-lane inputs; absent means no docs lane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<DocsLaneConfig>,
}

impl VelnorConfig {
    /// Schema 1: hosted-only generation. Schema 2 is routing.
    pub const SCHEMA: u32 = 1;
    /// Routing schema.
    pub const SCHEMA_V2: u32 = 2;
    /// Stack IDs registered in V1.
    pub const REGISTERED_STACKS: &'static [&'static str] = &["mise", "rust", "tofu"];
    /// Validate every field; failures name file, key path, and problem.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        self.check_schema(file)?;
        self.workflow.validate(file)?;
        self.resources.validate(file)?;
        self.test_sharding.validate(file)?;
        self.stacks.validate(file)?;
        self.discovery.validate(file)?;
        self.actions.validate(file)?;
        validate_qualified_tools(&self.qualified_tools, file)?;
        if let Some(docs) = &self.docs {
            docs.validate(file)?;
        }
        let mut check_ids = std::collections::BTreeSet::new();
        for (index, check) in self.checks.iter().enumerate() {
            check.validate(file, &format!("checks[{index}]"))?;
            for tool_id in &check.tools {
                if !self.qualified_tools.iter().any(|tool| &tool.id == tool_id) {
                    return Err(ContractError::config(
                        file,
                        format!("checks[{index}].tools"),
                        format!("unknown_qualified_tool:{tool_id}"),
                    ));
                }
            }
            check_receipt_budget::validate_check_budget(
                check,
                &self.qualified_tools,
                file,
                &format!("checks[{index}]"),
            )?;
            if !check_ids.insert(check.id.as_str()) {
                return Err(ContractError::config(file, "checks", "duplicate_check_id"));
            }
        }
        self.check_shard_budgets(file)?;
        Ok(())
    }

    /// Schema 1 rejects routing. Schema 2 requires `[execution]`.
    fn check_schema(&self, file: &str) -> Result<(), ContractError> {
        match self.schema {
            1 => {
                if self.execution.is_some() {
                    return Err(ContractError::config(
                        file,
                        "execution",
                        "schema1_rejects_execution",
                    ));
                }
                Ok(())
            }
            2 => {
                let execution = self
                    .execution
                    .as_ref()
                    .ok_or_else(|| ContractError::config(file, "execution", "missing_execution"))?;
                execution.validate(file)
            }
            _ => Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1 or 2",
            }),
        }
    }

    /// Reject shard counts beyond the test-process budget (par §8).
    /// # Errors
    pub fn check_shard_budgets(&self, file: &str) -> Result<(), ContractError> {
        let budget = self.resources.test_process_budget;
        if self.test_sharding.default_shards > budget {
            return Err(ContractError::config(
                file,
                "test_sharding.default_shards",
                "exceeds_test_process_budget",
            ));
        }
        for (manifest, shards) in &self.test_sharding.by_manifest {
            if *shards > budget {
                return Err(ContractError::config(
                    file,
                    format!("test_sharding.by_manifest.{manifest}"),
                    "exceeds_test_process_budget",
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
