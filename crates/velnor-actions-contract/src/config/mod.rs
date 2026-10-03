//! `.velnor/config.toml` schema types (TOML shape, serde only).
//!
//! Unknown fields are rejected; validation reports file, key path, problem.

mod actions;
mod discovery;
mod host_container;
mod mise;
mod qualified_tools;
mod release;
mod resources;
mod stacks;
mod tofu;
mod workflow;

pub use actions::{ActionPinOverride, ActionsConfig, OVERRIDABLE_ACTIONS};
pub use discovery::DiscoveryConfig;
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
pub use stacks::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustStackConfig, StacksConfig,
    is_valid_feature_name, is_valid_rust_target,
};
pub use tofu::{RootProblem, TofuStackConfig, Utf8RepoRelDir};
pub use workflow::{
    GeneratorValidation, LATEST_RUNNER_LABEL, RUNNER_LABEL_CATALOG, RunnerSelection,
    VelnorSupportWorkflow, WorkflowConfig, WorkflowPolicy,
};

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// Mandatory job admission for external ephemeral check runners.
/// GitHub evaluates job conditions before allocating a runner. Unknown
/// event classes and fork PRs cannot enter this execution environment.
pub const EPHEMERAL_CHECK_ADMISSION_CONDITION: &str = "github.event_name == 'push' || github.event_name == 'merge_group' || (github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository)";

/// Top-level `.velnor/config.toml` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VelnorConfig {
    /// Config schema version; must be 1.
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
    /// Explicit repository-owned checks independent of language stacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<MiseCheck>,
    /// Explicit check-scoped tool qualification registry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualified_tools: Vec<QualifiedTool>,
}

impl VelnorConfig {
    /// Schema version this contract accepts.
    pub const SCHEMA: u32 = 1;
    /// Stack IDs registered in V1.
    pub const REGISTERED_STACKS: &'static [&'static str] = &["mise", "rust", "tofu"];
    /// Validate every field; failures name file, key path, and problem.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.schema != Self::SCHEMA {
            return Err(ContractError::UnsupportedSchema {
                field: "schema",
                found: self.schema.to_string(),
                expected: "1",
            });
        }
        self.workflow.validate(file)?;
        self.resources.validate(file)?;
        self.test_sharding.validate(file)?;
        self.stacks.validate(file)?;
        self.discovery.validate(file)?;
        self.actions.validate(file)?;
        validate_qualified_tools(&self.qualified_tools, file)?;
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
            if !check_ids.insert(check.id.as_str()) {
                return Err(ContractError::config(file, "checks", "duplicate_check_id"));
            }
        }
        self.check_shard_budgets(file)?;
        Ok(())
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
#[path = "check_tool_reference_tests.rs"]
mod check_tool_reference_tests;
