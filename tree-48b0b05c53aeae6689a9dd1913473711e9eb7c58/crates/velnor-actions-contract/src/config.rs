//! `.velnor/config.toml` schema types (TOML shape, serde only).
//!
//! Unknown fields are rejected; validation reports file, key path, problem.
use crate::errors::ContractError;
use crate::ids::manifest_key_for_cargo_manifest;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
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
}
/// Workflow section of `.velnor/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    /// Display name for the generated workflow.
    pub name: String,
    /// Workflow policy.
    pub policy: WorkflowPolicy,
    /// Default branch; required when `origin/HEAD` cannot be resolved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,
    /// Generator validation mode.
    pub generator_validation: GeneratorValidation,
    /// Maximum parallel matrix jobs.
    pub max_parallel_jobs: u32,
    /// Pinned older runner-label override; omit for latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,
}
/// Workflow policy selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkflowPolicy {
    /// Default consumer policy; assumes no Velnor-specific files.
    ConsumerV1,
    /// Velnor-repository policy; canonical `tailrocks/velnor-new` only.
    VelnorRepositoryV1,
}
/// Generator validation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeneratorValidation {
    /// Validate with the locked bootstrap binary.
    Bootstrap,
    /// Validate with the built candidate binary (Velnor only).
    Candidate,
}
/// Runner-label selection provenance recorded in the plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerSelection {
    /// Latest pinned label; no override present.
    LatestDefault,
    /// Explicit `workflow.runner_label` override.
    ConfigOverride,
}
/// Process budgets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourcesConfig {
    /// Compiler process budget.
    pub compiler_process_budget: u32,
    /// Test process budget.
    pub test_process_budget: u32,
}
/// Test sharding policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TestShardingConfig {
    /// Default shard count.
    pub default_shards: u32,
    /// Per-manifest shard overrides keyed by repo-relative manifest path.
    #[serde(default)]
    pub by_manifest: BTreeMap<String, u32>,
}
/// Stack selection and per-stack options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StacksConfig {
    /// Sorted, duplicate-free exact registered stack IDs to ignore.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Rust stack options.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rust: Option<RustStackConfig>,
}
/// Rust stack options.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustStackConfig {
    /// Rust task configuration variants.
    pub configurations: Vec<RustConfiguration>,
}
/// One Rust task configuration variant.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustConfiguration {
    /// Configuration name.
    pub name: String,
    /// Cargo features for this variant.
    #[serde(default)]
    pub features: Vec<String>,
    /// Target triple or `host`.
    pub target: String,
}
/// Discovery exclusions (repo-relative POSIX globs).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryConfig {
    /// Exclusion globs applied before detectors.
    #[serde(default)]
    pub exclude: Vec<String>,
}
/// Policy job enabled only under `velnor-repository-v1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyJob {
    /// Repository-structure lint job.
    Alint,
    /// Dependency/security policy job.
    Policy,
}
/// Support-job set derived from policy plus validation mode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VelnorSupportWorkflow {
    /// Extra policy jobs to emit.
    pub policy_jobs: Vec<PolicyJob>,
    /// Whether to emit candidate validation.
    pub candidate_validation: bool,
}
impl WorkflowPolicy {
    /// Derive the support-job set for this policy and validation mode.
    #[must_use]
    pub fn support_workflow(&self, validation: GeneratorValidation) -> VelnorSupportWorkflow {
        let policy_jobs = match self {
            Self::ConsumerV1 => Vec::new(),
            Self::VelnorRepositoryV1 => vec![PolicyJob::Alint, PolicyJob::Policy],
        };
        VelnorSupportWorkflow {
            policy_jobs,
            candidate_validation: validation == GeneratorValidation::Candidate,
        }
    }
}
impl RustStackConfig {
    /// Documented default: one `default` configuration.
    #[must_use]
    pub fn default_config() -> Self {
        Self {
            configurations: vec![RustConfiguration {
                name: "default".to_owned(),
                features: vec!["default".to_owned()],
                target: "host".to_owned(),
            }],
        }
    }
}
impl VelnorConfig {
    /// Schema version this contract accepts.
    pub const SCHEMA: u32 = 1;
    /// Stack IDs registered in V1.
    pub const REGISTERED_STACKS: &'static [&'static str] = &["rust"];
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
        Ok(())
    }
}
impl WorkflowConfig {
    /// Validate the workflow section.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.name.trim().is_empty() {
            return Err(ContractError::config(file, "workflow.name", "empty_name"));
        }
        if self.max_parallel_jobs < 1 {
            return Err(ContractError::config(
                file,
                "workflow.max_parallel_jobs",
                "must_be_at_least_one",
            ));
        }
        if let Some(branch) = &self.default_branch
            && (branch.trim().is_empty() || branch.contains(' ') || branch.contains(".."))
        {
            return Err(ContractError::config(
                file,
                "workflow.default_branch",
                "malformed_branch",
            ));
        }
        if let Some(label) = &self.runner_label
            && (label.trim().is_empty() || label.ends_with("-latest") || label == "ubuntu-latest")
        {
            return Err(ContractError::config(
                file,
                "workflow.runner_label",
                "unpinned_or_alias_label",
            ));
        }
        Ok(())
    }
}
impl ResourcesConfig {
    /// Validate process budgets.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.compiler_process_budget < 1 {
            return Err(ContractError::config(
                file,
                "resources.compiler_process_budget",
                "must_be_at_least_one",
            ));
        }
        if self.test_process_budget < 1 {
            return Err(ContractError::config(
                file,
                "resources.test_process_budget",
                "must_be_at_least_one",
            ));
        }
        Ok(())
    }
}
impl TestShardingConfig {
    /// Validate shard counts and manifest keys.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.default_shards < 1 {
            return Err(ContractError::config(
                file,
                "test_sharding.default_shards",
                "must_be_at_least_one",
            ));
        }
        for (manifest, shards) in &self.by_manifest {
            manifest_key_for_cargo_manifest(manifest).map_err(|_| {
                ContractError::config(
                    file,
                    format!("test_sharding.by_manifest.{manifest}"),
                    "non_relative_manifest",
                )
            })?;
            if *shards < 1 {
                return Err(ContractError::config(
                    file,
                    format!("test_sharding.by_manifest.{manifest}"),
                    "must_be_at_least_one",
                ));
            }
        }
        Ok(())
    }
}
impl StacksConfig {
    /// Validate ignore list and Rust configurations.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let mut sorted = self.ignore.clone();
        sorted.sort();
        if sorted != self.ignore {
            return Err(ContractError::config(
                file,
                "stacks.ignore",
                "must_be_sorted",
            ));
        }
        let unique: BTreeSet<&str> = self.ignore.iter().map(String::as_str).collect();
        if unique.len() != self.ignore.len() {
            return Err(ContractError::config(
                file,
                "stacks.ignore",
                "duplicate_stack_id",
            ));
        }
        for id in &self.ignore {
            if !VelnorConfig::REGISTERED_STACKS.contains(&id.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.ignore",
                    format!("unknown_stack_id:{id}"),
                ));
            }
        }
        if let Some(rust) = &self.rust {
            rust.validate(file)?;
        }
        Ok(())
    }
}
impl RustStackConfig {
    /// Validate Rust configurations (unique nonempty names).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if self.configurations.is_empty() {
            return Err(ContractError::config(
                file,
                "stacks.rust.configurations",
                "empty_configurations",
            ));
        }
        let mut names = BTreeSet::new();
        for config in &self.configurations {
            if config.name.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.name",
                    "empty_name",
                ));
            }
            if !names.insert(config.name.as_str()) {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations",
                    format!("duplicate_configuration:{}", config.name),
                ));
            }
            if config.target.trim().is_empty() {
                return Err(ContractError::config(
                    file,
                    "stacks.rust.configurations.target",
                    "empty_target",
                ));
            }
        }
        Ok(())
    }
}
impl DiscoveryConfig {
    /// Validate exclusion globs (relative, no traversal, well-formed).
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        for pattern in &self.exclude {
            if pattern.is_empty()
                || pattern.starts_with('/')
                || pattern.contains('\\')
                || pattern.split('/').any(|seg| seg == "..")
                || !pattern.bytes().all(is_glob_byte)
            {
                return Err(ContractError::config(
                    file,
                    "discovery.exclude",
                    format!("malformed_pattern:{pattern}"),
                ));
            }
        }
        Ok(())
    }
}
/// Bytes allowed in discovery globs (paths plus `*?[]{}!` classes).
fn is_glob_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'/' | b'.'
                | b'-'
                | b'_'
                | b'*'
                | b'?'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'!'
                | b'+'
                | b'@'
        )
}
