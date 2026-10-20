//! `.velnor/config.toml` loading with defaults and key-path errors.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig, MiseCheck, QualifiedTool};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, PullRequestCachePolicy, ResourcesConfig,
    TestShardingConfig, VelnorConfig, VerifyConfig, WorkflowConfig, WorkflowPolicy, WorkflowTask,
};

use crate::OrchestratorError;
use crate::config_stacks::PartialStacks;

#[path = "bun_registry_install.rs"]
pub(crate) mod bun_registry_install;
#[path = "deps_lock.rs"]
pub(crate) mod deps_lock;
#[path = "gradle_run.rs"]
pub(crate) mod gradle_run;
#[path = "playwright_install.rs"]
pub(crate) mod playwright_install;
#[path = "playwright_lock.rs"]
pub(crate) mod playwright_lock;
#[path = "playwright_resolve.rs"]
pub(crate) mod playwright_resolve;
#[path = "private_fetch.rs"]
pub(crate) mod private_fetch;
#[path = "private_fetch_root.rs"]
pub(crate) mod private_fetch_root;
#[path = "private_fetch_session.rs"]
pub(crate) mod private_fetch_session;
#[path = "private_revoke.rs"]
pub(crate) mod private_revoke;
#[path = "private_sources.rs"]
pub(crate) mod private_sources;
#[path = "private_wiring.rs"]
pub(crate) mod private_wiring;

/// Config path as reported in diagnostics.
pub(crate) const CONFIG_REL: &str = ".velnor/config.toml";

/// Load, default, and validate `.velnor/config.toml` under `root`.
///
/// # Errors
///
/// Returns [`OrchestratorError::ConfigMissing`] when absent,
/// [`OrchestratorError::Config`] with file plus key path on parse or
/// validation failure, and [`OrchestratorError::Io`] on read failure.
/// Symlinks and root escapes fail closed as unsafe paths (X6).
pub(crate) fn load_config(root: &Path) -> Result<VelnorConfig, OrchestratorError> {
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
pub(crate) fn config_error(error: velnor_actions_contract::ContractError) -> OrchestratorError {
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
pub(crate) fn check_policy_mode(config: &VelnorConfig) -> Result<(), OrchestratorError> {
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
    /// Agent instruction file policy.
    #[serde(default)]
    agents: velnor_actions_contract::AgentsConfig,
    /// Generated dco2 configuration.
    #[serde(default)]
    dco: Option<velnor_actions_contract::DcoConfig>,
    /// Schema 2 routing section.
    #[serde(default)]
    execution: Option<velnor_actions_contract::ExecutionConfig>,
    /// Explicit repository-owned Mise checks.
    #[serde(default)]
    checks: Vec<MiseCheck>,
    /// Explicit qualified repository tool closure for named checks.
    #[serde(default)]
    qualified_tools: Vec<QualifiedTool>,
    /// Optional downstream package promotion.
    #[serde(default)]
    distribution: velnor_actions_contract::config::DistributionConfig,
    /// Private cross-repository dependency credentials (HD-7).
    #[serde(default)]
    private_sources: Option<velnor_actions_contract::config::PrivateSourcesConfig>,
    /// Container images built on every run and published from the default branch.
    #[serde(default)]
    images: Vec<velnor_actions_contract::config::ImageConfig>,
    /// Tag-driven publication.
    #[serde(default)]
    publish: velnor_actions_contract::config::PublishConfig,
    /// Optional exact `X.Y.Z` version overrides for generator-managed tools.
    #[serde(default)]
    tools: velnor_actions_contract::ToolsConfig,
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
    /// Crate-job timeout in minutes.
    crate_timeout_minutes: Option<u16>,
    /// Workflow-level Mise silencing override.
    mise_quiet: Option<bool>,
    /// Git LFS objects in generated checkouts.
    checkout_lfs: Option<bool>,
    /// Pull-request cache-write policy.
    #[serde(default)]
    pull_request_cache_policy: PullRequestCachePolicy,
    /// Pinned runner-label override.
    runner_label: Option<String>,
    /// One tagged task inventory for all explicit workflow work.
    #[serde(default)]
    tasks: Vec<WorkflowTask>,
    /// Optional protected post-merge `OpenTofu` apply declaration.
    tofu_apply: Option<velnor_actions_contract::TofuApplyConfig>,
    /// Optional closed review completion emitter.
    review_completion: Option<velnor_actions_contract::config::ReviewCompletionConfig>,
    /// Consumer-selected verification jobs.
    #[serde(default)]
    verify: VerifyConfig,
    /// Optional `workflow_dispatch` inputs for suite workflows.
    #[serde(default)]
    dispatch: velnor_actions_contract::WorkflowDispatchConfig,
    /// Repository-owned workflow files preserved across generation.
    #[serde(default)]
    preserve_workflows: Vec<String>,
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
            agents: self.agents,
            dco: self.dco,
            execution: self.execution,
            checks: self.checks,
            qualified_tools: self.qualified_tools,
            distribution: self.distribution,
            private_sources: self.private_sources,
            images: self.images,
            publish: self.publish,
            tools: self.tools,
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
            crate_timeout_minutes: self
                .crate_timeout_minutes
                .unwrap_or_else(velnor_actions_contract::config::default_crate_timeout_minutes),
            mise_quiet: self.mise_quiet.unwrap_or(true),
            checkout_lfs: self.checkout_lfs.unwrap_or(false),
            pull_request_cache_policy: self.pull_request_cache_policy,
            runner_label: self.runner_label,
            tasks: self.tasks,
            tofu_apply: self.tofu_apply,
            review_completion: self.review_completion,
            verify: self.verify,
            dispatch: self.dispatch,
            preserve_workflows: self.preserve_workflows,
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

#[cfg(test)]
#[path = "config_tests.rs"]
mod config_tests;

#[cfg(test)]
#[path = "config_preview_tests.rs"]
mod config_preview_tests;
