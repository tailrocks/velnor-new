// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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

pub(crate) const CONFIG_REL: &str = ".velnor/config.toml";

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

pub(crate) fn load_workflow_policy_until(
    root: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<WorkflowPolicy, OrchestratorError> {
    Ok(load_config_until(root, deadline)?.workflow.policy)
}

pub(crate) fn load_config_until(
    root: &Path,
    deadline: velnor_actions_mise::CheckDeadline,
) -> Result<VelnorConfig, OrchestratorError> {
    let path = root.join(CONFIG_REL);
    let text = match crate::safe_read::read_repo_file_until(
        root,
        CONFIG_REL,
        crate::safe_read::MAX_REPO_FILE_BYTES,
        deadline,
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
    if config.workflow.generator_validation == GeneratorValidation::Candidate {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            "workflow.generator_validation",
            "candidate_requires_report_aware_helper",
        ));
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialConfig {
    schema: Option<u32>,
    #[serde(default)]
    workflow: PartialWorkflow,
    #[serde(default)]
    resources: PartialResources,
    #[serde(default)]
    test_sharding: PartialSharding,
    #[serde(default)]
    stacks: PartialStacks,
    #[serde(default)]
    discovery: PartialDiscovery,
    #[serde(default)]
    actions: PartialActions,
    #[serde(default)]
    agents: velnor_actions_contract::AgentsConfig,
    #[serde(default)]
    github: velnor_actions_contract::GithubConfig,
    #[serde(default)]
    dco: Option<velnor_actions_contract::DcoConfig>,
    #[serde(default)]
    execution: Option<velnor_actions_contract::ExecutionConfig>,
    #[serde(default)]
    checks: Vec<MiseCheck>,
    #[serde(default)]
    native_checks: Vec<velnor_actions_contract::config::NativeHomebrewCheck>,
    #[serde(default)]
    qualified_tools: Vec<QualifiedTool>,
    #[serde(default)]
    distribution: velnor_actions_contract::config::DistributionConfig,
    #[serde(default)]
    private_sources: Option<velnor_actions_contract::config::PrivateSourcesConfig>,
    #[serde(default)]
    upgrades: Option<velnor_actions_contract::config::UpgradesConfig>,
    #[serde(default)]
    images: Vec<velnor_actions_contract::config::ImageConfig>,
    #[serde(default)]
    publish: velnor_actions_contract::config::PublishConfig,
    #[serde(default)]
    tools: velnor_actions_contract::ToolsConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialResources {
    compiler_process_budget: Option<u32>,
    test_process_budget: Option<u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialSharding {
    default_shards: Option<u32>,
    #[serde(default)]
    by_manifest: BTreeMap<String, u32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialDiscovery {
    #[serde(default)]
    exclude: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialActions {
    #[serde(default)]
    overrides: BTreeMap<String, ActionPinOverride>,
}

impl PartialConfig {
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
            native_checks: self.native_checks,
            qualified_tools: self.qualified_tools,
            distribution: self.distribution,
            private_sources: self.private_sources,
            upgrades: self.upgrades,
            images: self.images,
            publish: self.publish,
            tools: self.tools,
        })
    }
}

impl PartialResources {
    fn materialize(self) -> ResourcesConfig {
        ResourcesConfig {
            compiler_process_budget: self.compiler_process_budget.unwrap_or(2),
            test_process_budget: self.test_process_budget.unwrap_or(2),
        }
    }
}

impl PartialSharding {
    fn materialize(self) -> TestShardingConfig {
        TestShardingConfig {
            default_shards: self.default_shards.unwrap_or(1),
            by_manifest: self.by_manifest,
        }
    }
}

impl PartialDiscovery {
    fn materialize(self) -> DiscoveryConfig {
        DiscoveryConfig {
            exclude: self.exclude,
        }
    }
}

impl PartialActions {
    fn materialize(self) -> ActionsConfig {
        ActionsConfig {
            overrides: self.overrides,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{load_config_until, load_workflow_policy_until};
    use std::path::PathBuf;
    use velnor_actions_contract::WorkflowPolicy;

    fn fixture_root_with_repo_config(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "velnor-policy-{test}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock must advance")
                .as_nanos()
        ));
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.velnor/config.toml");
        let config_text = std::fs::read_to_string(&source).expect("repo config must exist");
        std::fs::create_dir_all(dir.join(".velnor")).expect("fixture must build");
        std::fs::write(dir.join(".velnor/config.toml"), config_text)
            .expect("fixture must write");
        dir
    }

    fn expired_deadline() -> velnor_actions_mise::CheckDeadline {
        let start = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(10))
            .expect("past instant must build");
        velnor_actions_mise::CheckDeadline::from_start(start, std::time::Duration::from_secs(1))
            .expect("past deadline must build")
    }

    #[test]
    fn policy_load_reads_config_with_live_deadline() {
        let dir = fixture_root_with_repo_config("live");
        let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(
            60,
        ))
        .expect("test deadline must build");
        let policy =
            load_workflow_policy_until(&dir, deadline).expect("live policy load must succeed");
        assert_eq!(policy, WorkflowPolicy::VelnorRepositoryV1);
        let config = load_config_until(&dir, deadline).expect("live config load must succeed");
        assert_eq!(config.workflow.policy, policy);
        std::fs::remove_dir_all(&dir).expect("temp dir must clean");
    }

    #[test]
    fn policy_load_refuses_expired_deadline() {
        let dir = fixture_root_with_repo_config("expired");
        let error = load_workflow_policy_until(&dir, expired_deadline())
            .expect_err("expired deadline must refuse the read");
        assert!(
            format!("{error:?}").contains("deadline_exhausted"),
            "unexpected error: {error:?}"
        );
        std::fs::remove_dir_all(&dir).expect("temp dir must clean");
    }
}
