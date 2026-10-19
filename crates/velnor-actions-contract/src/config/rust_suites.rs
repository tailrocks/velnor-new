//! Named Nextest suites (`[[stacks.rust.suites]]`) and `[workflow.dispatch]` config.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Default partition count for a declared Rust suite.
pub const DEFAULT_SUITE_PARTITIONS: u16 = 1;
/// Maximum partition count for a declared Rust suite.
pub const MAX_SUITE_PARTITIONS: u16 = 64;
/// Default per-job timeout in minutes for a declared Rust suite.
pub const DEFAULT_SUITE_TIMEOUT_MINUTES: u16 =
    crate::workflow::timeout::JobTimeout::DEFAULT.minutes();
/// Maximum per-job timeout in minutes for a declared Rust suite.
pub const MAX_SUITE_TIMEOUT_MINUTES: u16 = crate::workflow::timeout::JobTimeout::MAX_MINUTES;
/// Maximum `NEXTEST_TEST_THREADS` value for a declared Rust suite.
pub const MAX_SUITE_TEST_THREADS: u8 = 64;
/// Default artifact retention period in days for a declared Rust suite.
pub const DEFAULT_SUITE_RETENTION_DAYS: u16 = 14;
/// Maximum artifact retention period in days for a declared Rust suite.
pub const MAX_SUITE_RETENTION_DAYS: u16 = 90;
/// Default workflow slug for non-CI suites (`suites` -> `.github/workflows/suites.yml`).
pub const DEFAULT_SUITE_WORKFLOW_SLUG: &str = "suites";

/// Reserved job/suite identifiers that a custom suite cannot claim.
pub const RESERVED_SUITE_IDS: &[&str] =
    &["plan", "required", "actionlint", "publish-baseline", "rust"];

/// Reserved generator-owned workflow slugs that `suite.workflow` cannot claim.
pub const RESERVED_SUITE_WORKFLOW_SLUGS: &[&str] = &[
    "ci",
    "freshness",
    "release",
    "binary-release",
    "tofu-apply",
    "review-completion",
    "preview-tap-reconcile",
    "qualification",
    "product-release",
    "monitoring",
];

/// Core generator-owned workflow filenames that `preserve_workflows` cannot claim.
pub const GENERATOR_OWNED_WORKFLOW_FILES: &[&str] = &[
    "ci.yml",
    "freshness.yml",
    "release.yml",
    "binary-preview.yml",
    "binary-release.yml",
    "tofu-apply.yml",
    "review-completion.yml",
    "preview-tap-reconcile.yml",
    "qualification.yml",
    "product-release.yml",
    "monitoring.yml",
    "suites.yml",
];

const fn default_suite_partitions() -> u16 {
    DEFAULT_SUITE_PARTITIONS
}

const fn default_suite_timeout_minutes() -> u16 {
    DEFAULT_SUITE_TIMEOUT_MINUTES
}

const fn default_suite_retention_days() -> u16 {
    DEFAULT_SUITE_RETENTION_DAYS
}

fn default_suite_triggers() -> Vec<RustSuiteTrigger> {
    vec![RustSuiteTrigger::PullRequest, RustSuiteTrigger::Push]
}

/// Nextest `--run-ignored` execution mode for a suite.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextestRunIgnored {
    /// Run non-ignored tests only (`default`).
    #[default]
    Default,
    /// Run ignored tests only (`only`, alias `ignored-only`).
    #[serde(alias = "ignored-only")]
    Only,
    /// Run both non-ignored and ignored tests (`all`).
    All,
}

impl NextestRunIgnored {
    /// Canonical CLI flag value passed to `--run-ignored`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Only => "only",
            Self::All => "all",
        }
    }
}

/// Event trigger enabling a declared Rust suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSuiteTrigger {
    /// Run on `pull_request` events.
    PullRequest,
    /// Run on `push` events to the default branch.
    Push,
    /// Run on `schedule` cron events.
    Schedule,
    /// Run on manual `workflow_dispatch` events.
    WorkflowDispatch,
}

impl RustSuiteTrigger {
    /// Canonical config and GitHub event spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PullRequest => "pull_request",
            Self::Push => "push",
            Self::Schedule => "schedule",
            Self::WorkflowDispatch => "workflow_dispatch",
        }
    }
}

/// How test binaries are prepared for a declared Rust suite.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSuiteBuildMode {
    /// Each shard compiles and executes inline (`cargo nextest run`).
    #[default]
    Inline,
    /// One build job produces a `cargo nextest archive` shared across shards.
    Archive,
}

impl RustSuiteBuildMode {
    /// True when the build mode is the default `inline` mode.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Inline)
    }

    /// Canonical config spelling of this build mode.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Archive => "archive",
        }
    }
}

/// Condition under which a suite uploads a declared artifact.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSuiteArtifactWhen {
    /// Upload only when a prior step failed (`failure()`).
    #[default]
    Failure,
    /// Upload unconditionally (`always()`).
    Always,
    /// Upload only when all prior steps succeeded (`success()`).
    Success,
}

impl RustSuiteArtifactWhen {
    /// True when the upload condition is the default `failure` mode.
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Failure)
    }

    /// Canonical config spelling of this condition.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failure => "failure",
            Self::Always => "always",
            Self::Success => "success",
        }
    }

    /// GitHub Actions `if:` expression corresponding to this condition.
    #[must_use]
    pub const fn github_condition(self) -> &'static str {
        match self {
            Self::Failure => "failure()",
            Self::Always => "always()",
            Self::Success => "success()",
        }
    }
}

/// Repository artifact upload declaration for a Rust suite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustSuiteArtifact {
    /// Artifact name or template (may include `{shard}` or `{partition}`).
    pub name: String,
    /// Repo-relative paths to upload.
    pub paths: Vec<String>,
    /// Upload condition (`failure` by default, `always`, or `success`).
    #[serde(default, skip_serializing_if = "RustSuiteArtifactWhen::is_default")]
    pub when: RustSuiteArtifactWhen,
    /// Artifact retention period in days (`1..=90`, default `14`).
    #[serde(default = "default_suite_retention_days")]
    pub retention_days: u16,
}

/// One named Nextest suite declared under `[[stacks.rust.suites]]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustSuiteConfig {
    /// Unique lowercase suite identifier (`[a-z0-9-]+`).
    pub id: String,
    /// Nextest profile name in `.config/nextest.toml`.
    pub profile: String,
    /// Optional `--run-ignored` mode override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ignored: Option<NextestRunIgnored>,
    /// Number of hash partitions (`1..=64`, default `1`).
    #[serde(default = "default_suite_partitions")]
    pub partitions: u16,
    /// Binary preparation mode (`inline` or `archive`).
    #[serde(default, skip_serializing_if = "RustSuiteBuildMode::is_default")]
    pub build: RustSuiteBuildMode,
    /// Per-job timeout in minutes (`1..=5`, default `5`).
    #[serde(default = "default_suite_timeout_minutes")]
    pub timeout_minutes: u16,
    /// Optional `NEXTEST_TEST_THREADS` override (`1..=64`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_threads: Option<u8>,
    /// Optional per-suite runner label override (when `None`, inherits `workflow.runner_label`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,
    /// Optional workflow file slug (`[a-z0-9][a-z0-9_-]*`, without `.yml`) for non-CI suites.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,
    /// Event triggers that run this suite (defaults to `["pull_request", "push"]`).
    #[serde(default = "default_suite_triggers")]
    pub triggers: Vec<RustSuiteTrigger>,
    /// Five-field cron expressions when `triggers` includes `schedule`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedule: Vec<String>,
    /// Whether this suite gates the `Required` job on PR/push runs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// Optional explicit concurrency `cancel-in-progress` override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_in_progress: Option<bool>,
    /// Whether test failures are permitted without failing the job (`continue-on-error`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_failure: bool,
    /// Optional static nextest filterset expression passed via `-E`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,
    /// Optional `[workflow.dispatch.inputs]` name bound at runtime to `-E`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_input: Option<String>,
    /// Optional `[workflow.dispatch.inputs]` value constraints for `workflow_dispatch` runs.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dispatch_inputs: BTreeMap<String, Vec<String>>,
    /// Optional Cargo package name (`--package <name>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// Optional normalized repo-relative Cargo manifest path (`--manifest-path <path>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
    /// Additional static environment variables for suite execution.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Artifacts uploaded after suite execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<RustSuiteArtifact>,
}

impl RustSuiteConfig {
    /// Construct a suite with `id` and `profile` and documented defaults.
    #[must_use]
    pub fn new(id: impl Into<String>, profile: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            profile: profile.into(),
            run_ignored: None,
            partitions: DEFAULT_SUITE_PARTITIONS,
            build: RustSuiteBuildMode::default(),
            timeout_minutes: DEFAULT_SUITE_TIMEOUT_MINUTES,
            test_threads: None,
            runner_label: None,
            workflow: None,
            triggers: default_suite_triggers(),
            schedule: Vec::new(),
            required: false,
            cancel_in_progress: None,
            allow_failure: false,
            filter: None,
            filter_input: None,
            dispatch_inputs: BTreeMap::new(),
            package: None,
            manifest_path: None,
            env: BTreeMap::new(),
            artifacts: Vec::new(),
        }
    }

    /// True when the suite triggers on `pull_request` or `push`.
    #[must_use]
    pub fn has_pr_or_push_trigger(&self) -> bool {
        self.triggers.contains(&RustSuiteTrigger::PullRequest)
            || self.triggers.contains(&RustSuiteTrigger::Push)
    }

    /// True when the suite triggers on `schedule`.
    #[must_use]
    pub fn has_schedule_trigger(&self) -> bool {
        self.triggers.contains(&RustSuiteTrigger::Schedule)
    }

    /// True when the suite triggers on `workflow_dispatch`.
    #[must_use]
    pub fn has_workflow_dispatch_trigger(&self) -> bool {
        self.triggers.contains(&RustSuiteTrigger::WorkflowDispatch)
    }
}

/// Supported input widget types for `[workflow.dispatch.inputs]`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDispatchInputType {
    /// Free-form single-line string input.
    #[default]
    String,
    /// Closed choice input over `options`.
    Choice,
}

impl WorkflowDispatchInputType {
    /// Canonical config and GitHub Actions YAML spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Choice => "choice",
        }
    }
}

/// One `[workflow.dispatch.inputs.<name>]` definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDispatchInputConfig {
    /// Input widget type (`choice` or `string`).
    #[serde(rename = "type")]
    pub kind: WorkflowDispatchInputType,
    /// Optional human-readable description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the input is required on manual dispatch.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,
    /// Optional default value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Allowed option values when `type = "choice"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

/// `[workflow.dispatch]` configuration table.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDispatchConfig {
    /// Named `workflow_dispatch` inputs keyed by input identifier.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, WorkflowDispatchInputConfig>,
}

impl WorkflowDispatchConfig {
    /// True when no dispatch inputs are declared.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }
}

#[cfg(test)]
#[path = "rust_suites_timeout_tests.rs"]
mod timeout_tests;
