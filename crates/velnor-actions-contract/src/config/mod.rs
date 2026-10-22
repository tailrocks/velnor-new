//! `.velnor/config.toml` schema types (TOML shape, serde only).
//!
//! Unknown fields are rejected; validation reports file, key path, problem.

mod actions;
mod agents;
mod ansible;
mod binary_release;
mod build_task;
mod bun;
mod check_receipt_budget;
mod dbt;
mod dco;
mod discovery;
mod distribution;
mod dockerfile;
mod execution;
mod github;
pub mod gradle;
mod host_container;
mod image_grammar;
mod image_jobs;
mod image_task;
mod images;
mod mise;
pub mod playwright_lock;
mod preview_policy;
mod private_sources;
/// Generated run-time resolver for project-declared versions (hard decision 5).
mod project_tool_grammar;
/// Run-time references to project-declared tool versions (hard decision 5).
pub mod project_tool_refs;
pub mod project_tool_resolver;
mod publish;
mod publish_maven;
mod publish_npm;
mod qualified_tools;
mod release;
mod release_validation;
mod resources;
mod runs_on;
mod rust_inputs;
mod rust_suites;
mod rust_suites_validate;
mod rust_toolchain_source;
mod stacks;
mod task_source;
mod tofu;
mod tofu_apply;
mod tools;
mod upgrades;
mod verification;
mod workflow;
mod workflow_flags;
mod workflow_lint;
mod workflow_task;
mod workflow_timeouts;

pub(crate) use binary_release::validate_static_release_inventory;

pub use actions::{ActionPinOverride, ActionsConfig, OVERRIDABLE_ACTIONS};
pub use agents::{AgentsConfig, ClaudeMdMode};
pub use ansible::{
    ANSIBLE_JOB_PREFIX, AnsibleCheck, AnsibleCollection, AnsibleStackConfig, LintProfile,
};
pub use binary_release::{
    RustBinaryPreviewConfig, RustBinaryPreviewVersioning, RustBinaryReleaseArchive,
    RustBinaryReleaseCompanion, RustBinaryReleaseConfig, RustBinaryReleaseEvidence,
    RustBinaryReleaseEvidenceProfile, RustBinaryReleaseProducer, RustBinaryReleaseProduct,
    RustBinaryStableConfig,
};
pub use build_task::{
    BuildTask, BuildTaskRunner, BuildTaskXcodeCapability, is_valid_build_tool_key,
};
pub use bun::{
    BUN_JOB_PREFIX, BunBrowser, BunInputRule, BunMemberConfig, BunStackConfig, BunSuiteConfig,
    BunSuitePlaywright, BunSuiteReport, DEFAULT_BUN_SCRIPTS, DEFAULT_REPORT_RETENTION_DAYS,
    DEFAULT_RESULTS_RETENTION_DAYS, MAX_BUN_RETENTION_DAYS, MAX_BUN_SUITE_LEGS,
    MAX_BUN_TIMEOUT_MINUTES, RESERVED_BUN_SUITE_IDS, is_valid_member_name, is_valid_project_name,
    is_valid_script_name, is_valid_suite_id,
};
pub use check_receipt_budget::*;
pub use dbt::{DBT_JOB_PREFIX, DbtCheck, DbtService, DbtStackConfig};
pub use dco::{DCO_CONFIG_PATH, DcoConfig, DcoRemediationCommits, DcoRequire};
pub use discovery::DiscoveryConfig;
pub use distribution::{
    DistributionConfig, ExactHeadReviewCheck, HomebrewTapConfig, HomebrewTapDestination,
    HomebrewTapDestinations, HomebrewTapProtectionObserver, ReviewAppIdentity, TapMergePolicy,
    TapUpdateEnvironment, TapUpdaterBinding,
};
pub use dockerfile::{DockerfileFacts, DockerfileProblem, analyze_dockerfile, is_digest_pinned};
pub use execution::{
    ExecutionConfig, ExecutionMode, ExecutionOverride, ExecutionParity, ExecutionProfile,
    ExecutionRole, HOSTED_PROFILE_ID, ProfileKind, RoutingWorkflow, SCALE_SET_PROFILE_ID,
};
pub use github::{GithubConfig, UnmanagedPolicy};
pub use host_container::{
    ContainerPlatform, DaemonIdentityPolicy, HostContainerProfile, HostDockerCli, HostDockerDaemon,
    HostOrbStackSdk,
};
pub use image_grammar::{is_valid_image_id, is_valid_image_repository};
pub use image_task::{NativeImageCachePolicy, NativeImagePlatform, NativeImageTask};
pub use images::{
    DEFAULT_IMAGE_TIMEOUT_MINUTES, IMAGE_JOB_PREFIX, IMAGE_REGISTRY_HOST, ImageConfig,
    ImageDockerfile, ImagePlatform, ImagePrebuilt, validate_images,
};
pub use mise::{
    CheckEvidence, CheckExecutor, CheckPlatform, CheckRunner, CheckSystemTool, CheckSystemToolKind,
    MiseCheck, is_valid_check_system_build, is_valid_check_system_version, is_valid_mise_task_name,
};
pub use private_sources::{
    DEPS_LOCK_PATH, PrivateAssetSource, PrivateGitTreeSource, PrivateSourcesConfig,
    is_valid_asset_name, is_valid_extract_to, is_valid_github_owner, is_valid_github_repo_name,
    is_valid_source_id, is_valid_source_tag, split_repository,
};
pub use publish::{
    PublishBundle, PublishConfig, RELEASE_TAG_GLOB, ReleaseTag, VersionSource, is_artifact_id,
    is_strict_semver,
};
pub use publish_maven::{
    MAVEN_BUNDLE_OUTPUT_DIR, MAVEN_BUNDLE_TASK, MAVEN_VERSION_FILE, PublishMaven,
};
pub use publish_npm::{NPM_PUBLISH_REGISTRY_URL, NpmRegistry, PublishNpm};
pub use qualified_tools::{
    QualifiedCargoInstallation, QualifiedTool, QualifiedToolArtifact, QualifiedToolArtifactRecipe,
    QualifiedToolBackend, QualifiedToolExecutable, QualifiedToolExecutableRecipe,
    QualifiedToolOptions, QualifiedToolPlatform, QualifiedToolProbe, QualifiedToolRecipe,
    QualifiedToolRecipePlatform, validate_qualified_tool_recipe, validate_qualified_tool_recipes,
    validate_qualified_tools,
};
pub use release::{BootstrapRelease, ReleaseAuthentication, ReleaseRegistry, RustReleaseConfig};
pub use resources::{
    ResourcesConfig, ShardTimingEvidence, TestShardingConfig, validate_shard_changes_need_evidence,
};
pub use runs_on::{
    LINUX_AMD64, RunsOn, SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL, is_hosted_catalog,
    is_legacy_hosted_label,
};
pub use rust_inputs::RustInputRule;
pub use rust_suites::{
    DEFAULT_SUITE_PARTITIONS, DEFAULT_SUITE_RETENTION_DAYS, DEFAULT_SUITE_TIMEOUT_MINUTES,
    DEFAULT_SUITE_WORKFLOW_SLUG, GENERATOR_OWNED_WORKFLOW_FILES, MAX_SUITE_PARTITIONS,
    MAX_SUITE_RETENTION_DAYS, MAX_SUITE_TEST_THREADS, MAX_SUITE_TIMEOUT_MINUTES, NextestRunIgnored,
    RESERVED_SUITE_IDS, RESERVED_SUITE_WORKFLOW_SLUGS, RustSuiteArtifact, RustSuiteArtifactWhen,
    RustSuiteBuildMode, RustSuiteConfig, RustSuiteTrigger, WorkflowDispatchConfig,
    WorkflowDispatchInputConfig, WorkflowDispatchInputType,
};
pub use rust_suites_validate::{
    validate_preserve_workflows, validate_rust_suites, validate_suites_with_dispatch,
};
pub use rust_toolchain_source::RustToolchainSource;
pub use stacks::{
    DeclaredCompileDriver, DeclaredTestRunner, MAX_CRATE_BUILD_JOBS, RustConfiguration,
    RustCrateConfig, RustStackConfig, StacksConfig, is_valid_feature_name, is_valid_rust_target,
};
pub use task_source::MiseTaskSource;
pub use tofu::{RootProblem, TofuStackConfig, Utf8RepoRelDir};
pub use tofu_apply::{GitHubTokenSecret, S3BackendConfig, TofuApplyConfig};
pub use tools::{OVERRIDABLE_TOOLS, ToolsConfig, is_exact_semver};
pub use upgrades::{
    DEFAULT_UPGRADES_APP_ID_VARIABLE, DEFAULT_UPGRADES_APP_KEY_SECRET, DEFAULT_UPGRADES_CRON,
    DEFAULT_UPGRADES_STUCK_DAYS, MAX_UPGRADES_STUCK_DAYS, UPGRADES_WORKFLOW_PATH, UpgradesConfig,
};
pub use verification::{VerificationRunner, VerificationTask};
pub use workflow::{
    GeneratorValidation, LATEST_RUNNER_LABEL, PullRequestCachePolicy, RUNNER_LABEL_CATALOG,
    RunnerSelection, VelnorSupportWorkflow, VerifyConfig, WorkflowConfig, WorkflowPolicy,
    default_crate_timeout_minutes, default_plan_timeout_minutes,
};
pub use workflow_lint::{WorkflowLintConfig, WorkflowLintProfile};
pub use workflow_task::{WORKFLOW_TASK_JOB_PREFIX, WorkflowTask, is_valid_workflow_task_id};

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
    /// Tool-version overrides (`[tools]`); absent means bundled catalog pins.
    #[serde(default, skip_serializing_if = "ToolsConfig::is_empty")]
    pub tools: ToolsConfig,
    /// Agent instruction file policy (`[agents]`).
    #[serde(default, skip_serializing_if = "AgentsConfig::is_default")]
    pub agents: AgentsConfig,
    /// Generated dco2 configuration (`[dco]`); absent emits no `.github/dco.yml`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dco: Option<DcoConfig>,
    /// Policy for `.github` entries the generator does not emit (`[github]`).
    #[serde(default, skip_serializing_if = "GithubConfig::is_default")]
    pub github: GithubConfig,
    /// Schema 2 routing. Absent on schema 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionConfig>,
    /// Explicit repository-owned checks independent of language stacks.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<MiseCheck>,
    /// Explicit check-scoped tool qualification registry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub qualified_tools: Vec<QualifiedTool>,
    /// Optional downstream package promotion policy.
    #[serde(default, skip_serializing_if = "DistributionConfig::is_empty")]
    pub distribution: DistributionConfig,
    /// Private cross-repository dependency credentials (`[private_sources]`, HD-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_sources: Option<PrivateSourcesConfig>,
    /// Daily upgrade pull request and freshness gate (`[upgrades]`); off by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrades: Option<UpgradesConfig>,
    /// Container images built on every run and published from the default branch.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub images: Vec<ImageConfig>,
    /// Tag-driven publication (`[[publish.bundles]]`).
    #[serde(default, skip_serializing_if = "PublishConfig::is_empty")]
    pub publish: PublishConfig,
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
        self.github.validate(file)?;
        self.tools.validate(file)?;
        if let Some(dco) = &self.dco {
            dco.validate(file)?;
        }
        self.check_toolchain_consistency(file)?;
        let suites = self.stacks.rust.as_ref().map_or(&[][..], |r| &r.suites);
        validate_suites_with_dispatch(suites, &self.workflow.dispatch, file)?;
        for suite in suites {
            let slug = suite
                .workflow
                .as_deref()
                .unwrap_or(DEFAULT_SUITE_WORKFLOW_SLUG);
            let path = format!(".github/workflows/{slug}.yml");
            if self.workflow.preserve_workflows.contains(&path) {
                return Err(ContractError::config(
                    file,
                    "workflow.preserve_workflows",
                    format!("preserve_workflow_generator_owned:{path}"),
                ));
            }
        }
        self.check_binary_release_authority(file)?;
        if let Some(apply) = &self.workflow.tofu_apply {
            let roots = self.stacks.tofu.as_ref().ok_or_else(|| {
                ContractError::config(
                    file,
                    "workflow.tofu_apply.root",
                    "requires_stacks_tofu_roots",
                )
            })?;
            if !roots.roots.contains(&apply.root) {
                return Err(ContractError::config(
                    file,
                    "workflow.tofu_apply.root",
                    "must_match_declared_tofu_root",
                ));
            }
        }
        self.distribution.validate(file)?;
        if let Some(private_sources) = &self.private_sources {
            private_sources.validate(file)?;
        }
        if let Some(upgrades) = &self.upgrades {
            upgrades.validate(file)?;
        }
        self.publish.validate(file)?;
        self.check_distribution_source(file)?;
        self.check_binary_release_tools(file)?;
        self.check_crate_build_jobs(file)?;
        self.discovery.validate(file)?;
        self.actions.validate(file)?;
        validate_qualified_tools(&self.qualified_tools, file)?;
        self.check_checks(file)?;
        validate_images(&self.images, file)?;
        self.check_shard_budgets(file)?;
        Ok(())
    }

    fn check_toolchain_consistency(&self, file: &str) -> Result<(), ContractError> {
        if let (Some(stack_tc), Some(tools_tc)) = (
            self.stacks
                .rust
                .as_ref()
                .and_then(|r| r.toolchain.as_deref()),
            self.tools.get("rust"),
        ) && stack_tc != tools_tc
        {
            return Err(ContractError::config(
                file,
                "stacks.rust.toolchain",
                format!(
                    "conflicting_rust_toolchain:stacks.rust.toolchain={stack_tc},tools.rust={tools_tc}"
                ),
            ));
        }
        Ok(())
    }

    fn check_checks(&self, file: &str) -> Result<(), ContractError> {
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
}

#[cfg(test)]
#[path = "check_tool_reference_tests.rs"]
mod check_tool_reference_tests;

#[cfg(test)]
mod unit_tests;

mod review_completion;
pub(crate) use distribution::valid_review_check_name;
pub use review_completion::{ReviewCompletionConfig, ReviewCompletionProvider};

/// Tap updater execution remains independently bounded to five minutes.
pub(crate) const NATIVE_TAP_UPDATER_MAX_TIMEOUT_MINUTES: u16 = 5;
