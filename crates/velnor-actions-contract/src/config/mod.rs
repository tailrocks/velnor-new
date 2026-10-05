//! `.velnor/config.toml` schema types (TOML shape, serde only).
//!
//! Unknown fields are rejected; validation reports file, key path, problem.

mod actions;
mod discovery;
mod execution;
mod release;
mod resources;
mod runs_on;
mod stacks;
mod tofu;
mod workflow;

pub use actions::{ActionPinOverride, ActionsConfig, OVERRIDABLE_ACTIONS};
pub use discovery::DiscoveryConfig;
pub use execution::{
    ExecutionConfig, ExecutionMode, ExecutionOverride, ExecutionParity, ExecutionProfile,
    ExecutionRole, HOSTED_PROFILE_ID, ProfileKind, RoutingWorkflow, SCALE_SET_PROFILE_ID,
};
pub use release::{BootstrapRelease, ReleaseAuthentication, RustReleaseConfig};
pub use resources::{
    ResourcesConfig, ShardTimingEvidence, TestShardingConfig, validate_shard_changes_need_evidence,
};
pub use runs_on::{
    LINUX_AMD64, RunsOn, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL, is_hosted_catalog,
    is_legacy_hosted_label,
};
pub use stacks::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustStackConfig, StacksConfig,
    is_valid_custom_task_name, is_valid_feature_name, is_valid_rust_target,
};
pub use tofu::{RootProblem, TofuStackConfig, Utf8RepoRelDir};
pub use workflow::{
    GeneratorValidation, LATEST_RUNNER_LABEL, RUNNER_LABEL_CATALOG, RunnerSelection,
    VelnorSupportWorkflow, WorkflowConfig, WorkflowPolicy,
};

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

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
}

impl VelnorConfig {
    /// Schema 1: hosted-only generation. Schema 2 is routing.
    pub const SCHEMA: u32 = 1;
    /// Routing schema.
    pub const SCHEMA_V2: u32 = 2;
    /// Stack IDs registered in V1.
    pub const REGISTERED_STACKS: &'static [&'static str] = &["rust", "tofu"];
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
