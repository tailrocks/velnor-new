//! `.velnor/config.toml` loading with defaults and key-path errors.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use velnor_actions_contract::config::{ActionPinOverride, ActionsConfig, MiseCheck, QualifiedTool};
use velnor_actions_contract::{
    DiscoveryConfig, GeneratorValidation, ResourcesConfig, TestShardingConfig, VelnorConfig,
    WorkflowPolicy,
};

use crate::OrchestratorError;
use crate::config_stacks::PartialStacks;
#[path = "config_workflow.rs"]
mod workflow;
use workflow::PartialWorkflow;

#[path = "bun_registry_install.rs"]
pub(crate) mod bun_registry_install;
#[path = "deps_layout.rs"]
pub(crate) mod deps_layout;
#[path = "deps_lock.rs"]
pub(crate) mod deps_lock;
#[path = "gradle_jobs.rs"]
pub(crate) mod gradle_jobs;
#[path = "gradle_run.rs"]
pub(crate) mod gradle_run;
#[path = "playwright_install.rs"]
pub(crate) mod playwright_install;
#[path = "playwright_lock.rs"]
pub(crate) mod playwright_lock;
#[path = "playwright_resolve.rs"]
pub(crate) mod playwright_resolve;
#[path = "playwright_system_deps.rs"]
pub(crate) mod playwright_system_deps;
#[path = "private_fetch.rs"]
pub(crate) mod private_fetch;
#[path = "private_fetch_http.rs"]
pub(crate) mod private_fetch_http;
#[path = "private_fetch_root.rs"]
pub(crate) mod private_fetch_root;
#[path = "private_fetch_session.rs"]
pub(crate) mod private_fetch_session;
#[path = "private_fetch_verified.rs"]
pub(crate) mod private_fetch_verified;
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
    load_config_from_text(&text)
}

/// Parse and validate the exact serialized configuration bytes.
pub(crate) fn load_config_from_text(text: &str) -> Result<VelnorConfig, OrchestratorError> {
    if u64::try_from(text.len()).map_or(true, |length| {
        length > crate::safe_read::MAX_REPO_FILE_BYTES
    }) {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "document",
            "config_too_large",
        ));
    }
    let partial: PartialConfig = toml::from_str(text).map_err(|err| {
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
    /// Policy for unmanaged `.github` entries.
    #[serde(default)]
    github: velnor_actions_contract::GithubConfig,
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
    /// Reviewed job roles for explicit authority inventory generation.
    #[serde(default)]
    job_authority: Vec<velnor_actions_contract::config::JobAuthorityBinding>,
    /// Private cross-repository dependency credentials (HD-7).
    #[serde(default)]
    private_sources: Option<velnor_actions_contract::config::PrivateSourcesConfig>,
    /// Daily upgrade pull request and freshness gate; off by default.
    #[serde(default)]
    upgrades: Option<velnor_actions_contract::config::UpgradesConfig>,
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
            github: self.github,
            dco: self.dco,
            execution: self.execution,
            checks: self.checks,
            qualified_tools: self.qualified_tools,
            distribution: self.distribution,
            job_authority: self.job_authority,
            private_sources: self.private_sources,
            upgrades: self.upgrades,
            images: self.images,
            publish: self.publish,
            tools: self.tools,
        })
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
