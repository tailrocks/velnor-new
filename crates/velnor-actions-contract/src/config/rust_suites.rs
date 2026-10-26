// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const DEFAULT_SUITE_PARTITIONS: u16 = 1;

pub const MAX_SUITE_PARTITIONS: u16 = 64;

pub const DEFAULT_SUITE_TIMEOUT_MINUTES: u16 =
    crate::workflow::timeout::JobTimeout::DEFAULT.minutes();

pub const MAX_SUITE_TIMEOUT_MINUTES: u16 = crate::workflow::timeout::JobTimeout::MAX_MINUTES;

pub const MAX_SUITE_TEST_THREADS: u8 = 64;

pub const DEFAULT_SUITE_RETENTION_DAYS: u16 = 14;

pub const MAX_SUITE_RETENTION_DAYS: u16 = 90;

pub const DEFAULT_SUITE_WORKFLOW_SLUG: &str = "suites";

pub const RESERVED_SUITE_IDS: &[&str] =
    &["plan", "required", "required-gate", "jactionlint", "rust"];

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

pub const GENERATOR_OWNED_WORKFLOW_FILES: &[&str] = &[
    "ci.yml",
    "freshness.yml",
    "release.yml",
    "publish.yml",
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextestRunIgnored {
    #[default]
    Default,

    #[serde(alias = "ignored-only")]
    Only,

    All,
}

impl NextestRunIgnored {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Only => "only",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSuiteTrigger {
    PullRequest,

    Push,

    Schedule,

    WorkflowDispatch,
}

impl RustSuiteTrigger {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSuiteBuildMode {
    #[default]
    Inline,

    Archive,
}

impl RustSuiteBuildMode {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Inline)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Archive => "archive",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RustSuiteArtifactWhen {
    #[default]
    Failure,

    Always,

    Success,
}

impl RustSuiteArtifactWhen {
    #[must_use]
    pub const fn is_default(&self) -> bool {
        matches!(self, Self::Failure)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Failure => "failure",
            Self::Always => "always",
            Self::Success => "success",
        }
    }

    #[must_use]
    pub const fn github_condition(self) -> &'static str {
        match self {
            Self::Failure => "failure()",
            Self::Always => "always()",
            Self::Success => "success()",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustSuiteArtifact {
    pub name: String,

    pub paths: Vec<String>,

    #[serde(default, skip_serializing_if = "RustSuiteArtifactWhen::is_default")]
    pub when: RustSuiteArtifactWhen,

    #[serde(default = "default_suite_retention_days")]
    pub retention_days: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustSuiteConfig {
    pub id: String,

    pub profile: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_ignored: Option<NextestRunIgnored>,

    #[serde(default = "default_suite_partitions")]
    pub partitions: u16,

    #[serde(default, skip_serializing_if = "RustSuiteBuildMode::is_default")]
    pub build: RustSuiteBuildMode,

    #[serde(default = "default_suite_timeout_minutes")]
    pub timeout_minutes: u16,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_threads: Option<u8>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_label: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<String>,

    #[serde(default = "default_suite_triggers")]
    pub triggers: Vec<RustSuiteTrigger>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub schedule: Vec<String>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel_in_progress: Option<bool>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub allow_failure: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter_input: Option<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub dispatch_inputs: BTreeMap<String, Vec<String>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<RustSuiteArtifact>,
}

impl RustSuiteConfig {
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

    #[must_use]
    pub fn has_pr_or_push_trigger(&self) -> bool {
        self.triggers.contains(&RustSuiteTrigger::PullRequest)
            || self.triggers.contains(&RustSuiteTrigger::Push)
    }

    #[must_use]
    pub fn has_schedule_trigger(&self) -> bool {
        self.triggers.contains(&RustSuiteTrigger::Schedule)
    }

    #[must_use]
    pub fn has_workflow_dispatch_trigger(&self) -> bool {
        self.triggers.contains(&RustSuiteTrigger::WorkflowDispatch)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDispatchInputType {
    #[default]
    String,

    Choice,
}

impl WorkflowDispatchInputType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Choice => "choice",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDispatchInputConfig {
    #[serde(rename = "type")]
    pub kind: WorkflowDispatchInputType,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub required: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowDispatchConfig {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, WorkflowDispatchInputConfig>,
}

impl WorkflowDispatchConfig {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }
}
