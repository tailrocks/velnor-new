//! `.velnor/config.toml` schema types (TOML shape, serde only).
//!
//! Unknown fields are rejected; validation reports file, key path, problem.

mod actions;
mod delivery;
mod delivery_apt;
mod delivery_desktop;
mod discovery;
mod native_desktop;
mod native_desktop_checks;
mod oci;
mod release;
mod release_owners;
mod resources;
mod stacks;
mod swift_inputs;
mod tofu;
mod workflow;
mod workflow_verification;
mod workload_gradle;
mod workload_package;
mod workload_package_update;
mod workloads;

pub use actions::{ActionPinOverride, ActionsConfig, OVERRIDABLE_ACTIONS};
pub use delivery::DeliveryConfig;
pub use delivery_apt::AptDeliveryConfig;
pub use delivery_desktop::DesktopDeliveryConfig;
pub use discovery::DiscoveryConfig;
pub use native_desktop::{
    AppleAppProfile, NativeDesktopProfile, NativeDesktopTarget, RustFfiProfile,
};
pub use native_desktop_checks::{NativeDesktopChecks, SwiftTestFramework};
pub use oci::{OciImage, OciPlatform, OciReleaseConfig, RegistryAuthentication};
pub use release::{BootstrapRelease, ReleaseAuthentication, RustReleaseConfig};
pub use resources::{
    ResourcesConfig, ShardTimingEvidence, TestShardingConfig, validate_shard_changes_need_evidence,
};
pub use stacks::{
    DeclaredCompileDriver, DeclaredTestRunner, RustConfiguration, RustFeatureMode, RustStackConfig,
    StacksConfig, is_valid_custom_task_name, is_valid_feature_name, is_valid_rust_target,
};
pub use swift_inputs::{SwiftChecks, SwiftFfiArtifacts, SwiftInputs};
pub use tofu::{RootProblem, TofuStackConfig, Utf8RepoRelDir};
pub use workflow::{
    GeneratorValidation, LATEST_RUNNER_LABEL, RUNNER_LABEL_CATALOG, RunnerSelection,
    VelnorSupportWorkflow, WorkflowConfig, WorkflowPolicy,
};
pub use workflow_verification::VerificationConfig;

pub use workload_gradle::{GradleWorkloadConfig, PostgresBinding, PostgresFixture};
pub use workload_package::PackageScript;
pub use workload_package_update::{
    PackageUpdateArchive, PackageUpdateArtifact, PackageUpdateFixture, PackageUpdateOutput,
};
pub use workloads::{WorkloadConfig, WorkloadKind, is_valid_workload_name, is_valid_workload_path};

use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

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
    /// Optional native delivery workflows; disabled when omitted.
    #[serde(default)]
    pub delivery: DeliveryConfig,
}

impl VelnorConfig {
    /// Schema version this contract accepts.
    pub const SCHEMA: u32 = 1;
    /// Stack IDs registered in V1.
    pub const REGISTERED_STACKS: &'static [&'static str] = &["rust", "tofu", "workload"];
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
        if self.delivery.is_configured() && self.workflow.policy != WorkflowPolicy::ConsumerV1 {
            return Err(ContractError::config(
                file,
                "delivery",
                "delivery_requires_consumer_policy",
            ));
        }
        self.delivery.validate(file)?;
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

mod required_obligations;
pub use required_obligations::{
    RequiredNativeObligation, RequiredNativeObligations, RequiredNativePhase,
};
