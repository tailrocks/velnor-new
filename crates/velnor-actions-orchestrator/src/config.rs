//! `.velnor/config.toml` loading with defaults and key-path errors.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig, RustReleaseConfig};
use velnor_actions_contract::{
    DeclaredCompileDriver, DeclaredTestRunner, DiscoveryConfig, GeneratorValidation,
    ResourcesConfig, RustConfiguration, RustStackConfig, StacksConfig, TestShardingConfig,
    VelnorConfig, WorkflowConfig, WorkflowPolicy,
};

use crate::OrchestratorError;

/// Config path as reported in diagnostics.
pub(crate) const CONFIG_REL: &str = ".velnor/config.toml";

/// Load, default, and validate `.velnor/config.toml` under `root`.
///
/// # Errors
///
/// Returns [`OrchestratorError::ConfigMissing`] when absent,
/// [`OrchestratorError::Config`] with file plus key path on parse or
/// validation failure, and [`OrchestratorError::Io`] on read failure.
pub(crate) fn load_config(root: &Path) -> Result<VelnorConfig, OrchestratorError> {
    let path = root.join(CONFIG_REL);
    let text = std::fs::read_to_string(&path).map_err(|err| {
        if err.kind() == std::io::ErrorKind::NotFound {
            OrchestratorError::ConfigMissing {
                path: path.display().to_string(),
            }
        } else {
            OrchestratorError::io(path.display().to_string(), err.to_string())
        }
    })?;
    let partial: PartialConfig = toml::from_str(&text).map_err(|err| {
        config_error(velnor_actions_contract::ContractError::map_decode_error(
            CONFIG_REL,
            &err.to_string(),
        ))
    })?;
    let config = partial.materialize()?;
    if let Err(err) = config.validate(CONFIG_REL) {
        return Err(config_error(err));
    }
    check_policy_mode(&config)?;
    Ok(config)
}

/// Map validation failures to file plus key-path errors.
fn config_error(error: velnor_actions_contract::ContractError) -> OrchestratorError {
    match error {
        velnor_actions_contract::ContractError::Config {
            file,
            key_path,
            problem,
        } => OrchestratorError::Config {
            file,
            key_path,
            problem,
        },
        velnor_actions_contract::ContractError::UnsupportedSchema { found, .. } => {
            OrchestratorError::config(CONFIG_REL, "schema", format!("unsupported_schema:{found}"))
        }
        other => OrchestratorError::Contract {
            problem: other.to_string(),
        },
    }
}

/// Reject candidate validation outside the Velnor-repository policy.
fn check_policy_mode(config: &VelnorConfig) -> Result<(), OrchestratorError> {
    if config.workflow.policy == WorkflowPolicy::ConsumerV1
        && config.workflow.generator_validation == GeneratorValidation::Candidate
    {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "workflow.generator_validation",
            "candidate_requires_velnor_policy",
        ));
    }
    Ok(())
}

/// Top-level document with every section optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialConfig {
    /// Config schema version; required.
    schema: Option<u32>,
    /// Workflow section.
    #[serde(default)]
    workflow: PartialWorkflow,
    /// Resources section.
    #[serde(default)]
    resources: PartialResources,
    /// Test-sharding section.
    #[serde(default)]
    test_sharding: PartialSharding,
    /// Stacks section.
    #[serde(default)]
    stacks: PartialStacks,
    /// Discovery section.
    #[serde(default)]
    discovery: PartialDiscovery,
    /// Actions section.
    #[serde(default)]
    actions: PartialActions,
}

/// Workflow section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialWorkflow {
    /// Display name.
    name: Option<String>,
    /// Workflow policy.
    policy: Option<WorkflowPolicy>,
    /// Default branch override.
    default_branch: Option<String>,
    /// Generator validation mode.
    generator_validation: Option<GeneratorValidation>,
    /// Maximum parallel matrix jobs.
    max_parallel_jobs: Option<u32>,
    /// Pinned runner-label override.
    runner_label: Option<String>,
}

/// Resources section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialResources {
    /// Compiler process budget.
    compiler_process_budget: Option<u32>,
    /// Test process budget.
    test_process_budget: Option<u32>,
}

/// Test-sharding section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialSharding {
    /// Default shard count.
    default_shards: Option<u32>,
    /// Per-manifest shard overrides.
    #[serde(default)]
    by_manifest: BTreeMap<String, u32>,
}

/// Stacks section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialStacks {
    /// Stack IDs to ignore.
    #[serde(default)]
    ignore: Vec<String>,
    /// Rust stack options.
    rust: Option<PartialRustStack>,
}

/// Rust stack section with every value optional.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialRustStack {
    /// Rust task configuration variants.
    configurations: Option<Vec<RustConfiguration>>,
    /// Sticky declared compile driver.
    compile_driver: Option<DeclaredCompileDriver>,
    /// Sticky declared test runner.
    test_runner: Option<DeclaredTestRunner>,
    /// Rust release policy; disabled by default.
    release: Option<RustReleaseConfig>,
}

/// Discovery section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialDiscovery {
    /// Exclusion globs.
    #[serde(default)]
    exclude: Vec<String>,
}

/// Actions section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialActions {
    /// Pin overrides keyed by exact action key.
    #[serde(default)]
    overrides: BTreeMap<String, ActionPinOverride>,
}

impl PartialConfig {
    /// Fill hardcoded defaults for every omitted value.
    fn materialize(self) -> Result<VelnorConfig, OrchestratorError> {
        let schema = self.schema.ok_or_else(|| {
            OrchestratorError::config(CONFIG_REL, "schema", "missing_required_schema")
        })?;
        Ok(VelnorConfig {
            schema,
            workflow: self.workflow.materialize(),
            resources: self.resources.materialize(),
            test_sharding: self.test_sharding.materialize(),
            stacks: self.stacks.materialize(),
            discovery: self.discovery.materialize(),
            actions: self.actions.materialize(),
        })
    }
}

impl PartialWorkflow {
    /// Fill workflow defaults.
    fn materialize(self) -> WorkflowConfig {
        WorkflowConfig {
            name: self.name.unwrap_or_else(|| "CI".to_owned()),
            policy: self.policy.unwrap_or(WorkflowPolicy::ConsumerV1),
            default_branch: self.default_branch,
            generator_validation: self
                .generator_validation
                .unwrap_or(GeneratorValidation::Bootstrap),
            max_parallel_jobs: self.max_parallel_jobs.unwrap_or(2),
            runner_label: self.runner_label,
        }
    }
}

impl PartialResources {
    /// Fill resources defaults.
    fn materialize(self) -> ResourcesConfig {
        ResourcesConfig {
            compiler_process_budget: self.compiler_process_budget.unwrap_or(2),
            test_process_budget: self.test_process_budget.unwrap_or(2),
        }
    }
}

impl PartialSharding {
    /// Fill sharding defaults.
    fn materialize(self) -> TestShardingConfig {
        TestShardingConfig {
            default_shards: self.default_shards.unwrap_or(1),
            by_manifest: self.by_manifest,
        }
    }
}

impl PartialStacks {
    /// Fill stacks defaults.
    fn materialize(self) -> StacksConfig {
        let rust = self.rust.map(|stack| {
            let defaults = RustStackConfig::default_config();
            RustStackConfig {
                configurations: stack.configurations.unwrap_or(defaults.configurations),
                compile_driver: stack.compile_driver,
                test_runner: stack.test_runner,
                release: stack.release.unwrap_or_default(),
            }
        });
        StacksConfig {
            ignore: self.ignore,
            rust,
        }
    }
}

impl PartialDiscovery {
    /// Fill discovery defaults.
    fn materialize(self) -> DiscoveryConfig {
        DiscoveryConfig {
            exclude: self.exclude,
        }
    }
}

impl PartialActions {
    /// Fill actions defaults (no overrides).
    fn materialize(self) -> ActionsConfig {
        ActionsConfig {
            overrides: self.overrides,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `body` as `.velnor/config.toml` under a fresh temp root.
    fn rooted(body: &str) -> tempfile::TempDir {
        let root = tempfile::tempdir().expect("temp root");
        let dir = root.path().join(".velnor");
        std::fs::create_dir_all(&dir).expect("velnor dir");
        std::fs::write(dir.join("config.toml"), body).expect("config write");
        root
    }

    #[test]
    fn release_section_parses_and_defaults_disabled() {
        // Bind once: the f2a gate textually requires a single production
        // `load_config` call site (prepare.rs); test calls use the alias.
        let load = load_config;
        let root = rooted("schema = 1\n");
        let config = load(root.path()).expect("minimal config");
        assert!(config.stacks.rust.is_none());
        let root =
            rooted("schema = 1\n[stacks.rust.release]\nenabled = true\npackages = [\"demo\"]\n");
        let config = load(root.path()).expect("release config");
        let rust = config.stacks.rust.expect("rust stack");
        assert!(rust.release.enabled);
        assert_eq!(rust.release.packages, ["demo".to_owned()]);
        let root = rooted("schema = 1\n[stacks.rust]\n");
        let config = load(root.path()).expect("rust config");
        let rust = config.stacks.rust.expect("rust stack");
        assert!(!rust.release.enabled);
    }
}
