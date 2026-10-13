//! `.velnor/config.toml` loading with defaults and key-path errors.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract_config::config::{
    ActionPinOverride, ActionsConfig, MiseCheck, QualifiedTool,
};
use velnor_actions_contract_config::{
    DiscoveryConfig, DocsLaneConfig, GeneratorValidation, ResourcesConfig, TestShardingConfig,
    VelnorConfig, VerificationTask, WorkflowConfig, WorkflowPolicy,
};

use crate::OrchestratorError;
use crate::config_stacks::PartialStacks;

/// Config path as reported in diagnostics.
pub const CONFIG_REL: &str = ".velnor/config.toml";

/// Load, default, and validate `.velnor/config.toml` under `root`.
///
/// # Errors
///
/// Returns [`OrchestratorError::ConfigMissing`] when absent,
/// [`OrchestratorError::Config`] with file plus key path on parse or
/// validation failure, and [`OrchestratorError::Io`] on read failure.
/// Symlinks and root escapes fail closed as unsafe paths (X6).
pub fn load_config(root: &Path) -> Result<VelnorConfig, OrchestratorError> {
    let path = root.join(CONFIG_REL);
    let text = match crate::safe_read::read_repo_file(
        root,
        CONFIG_REL,
        crate::safe_read::MAX_REPO_FILE_BYTES,
    )? {
        crate::safe_read::RepoRead::Absent => {
            return Err(OrchestratorError::ConfigMissing {
                path: path.display().to_string(),
            });
        }
        crate::safe_read::RepoRead::Text(text) => text,
    };
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
pub fn config_error(error: velnor_actions_contract::ContractError) -> OrchestratorError {
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
    /// Schema 2 routing section.
    #[serde(default)]
    execution: Option<velnor_actions_contract_config::ExecutionConfig>,
    /// Explicit repository-owned Mise checks.
    #[serde(default)]
    checks: Vec<MiseCheck>,
    /// Explicit qualified repository tool closure for named checks.
    #[serde(default)]
    qualified_tools: Vec<QualifiedTool>,
    /// Docs-lane inputs; absent means no docs lane.
    #[serde(default)]
    docs: Option<PartialDocsLane>,
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
    /// Explicit isolated verification tasks.
    #[serde(default)]
    tasks: Vec<VerificationTask>,
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

/// Docs-lane section with every value optional.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialDocsLane {
    /// Repo-relative app directory.
    app_dir: Option<String>,
    /// App-relative MDX collection directory.
    content_dir: Option<String>,
    /// Site base path serving the collection.
    base_path: Option<String>,
    /// App-relative build output directory.
    output_dir: Option<String>,
    /// Absolute smoke routes.
    smoke_routes: Option<Vec<String>>,
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
            stacks: self.stacks.materialize()?,
            discovery: self.discovery.materialize(),
            actions: self.actions.materialize(),
            execution: self.execution,
            checks: self.checks,
            qualified_tools: self.qualified_tools,
            docs: self.docs.map(PartialDocsLane::materialize),
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
            tasks: self.tasks,
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

impl PartialDocsLane {
    /// Fill docs-lane defaults; omitted routes smoke `/` plus the base.
    fn materialize(self) -> DocsLaneConfig {
        let base_path = self.base_path.unwrap_or_else(|| "/docs".to_owned());
        DocsLaneConfig {
            app_dir: self.app_dir.unwrap_or_else(|| "docs".to_owned()),
            content_dir: self
                .content_dir
                .unwrap_or_else(|| "content/docs".to_owned()),
            output_dir: self
                .output_dir
                .unwrap_or_else(|| ".output/public".to_owned()),
            smoke_routes: self
                .smoke_routes
                .unwrap_or_else(|| vec!["/".to_owned(), base_path.clone()]),
            base_path,
        }
    }
}
