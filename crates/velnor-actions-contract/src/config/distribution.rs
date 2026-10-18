//! Typed downstream distribution policy.

use super::{CheckRunner, MiseTaskSource, mise::is_valid_mise_task_name};
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Optional distribution destinations outside `[stacks.rust]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionConfig {
    /// Optional Homebrew tap update policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homebrew_tap: Option<HomebrewTapConfig>,
}

impl DistributionConfig {
    /// Whether no distribution destination is declared.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.homebrew_tap.is_none()
    }

    /// Validate enabled distribution destinations.
    ///
    /// # Errors
    ///
    /// Returns the first invalid destination contract.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if let Some(tap) = &self.homebrew_tap {
            tap.validate(file)?;
        }
        Ok(())
    }
}

/// PR promotion policy for an immutable Rust binary preview release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomebrewTapConfig {
    /// Whether to generate tap preparation and PR promotion.
    #[serde(default)]
    pub enabled: bool,
    /// Only supported source channel; not a user-defined release locator.
    pub source: HomebrewTapSource,
    /// Owner/repository slug, never a URL or arbitrary API destination.
    pub repository: String,
    /// Single formula path that updater and writer may change.
    pub formula: String,
    /// Source-qualified updater executed without write credentials.
    pub updater: TapUpdaterBinding,
    /// Protected environment for the isolated App writer and merge boundary.
    pub environment: TapUpdateEnvironment,
    /// Repository/environment variable containing the GitHub App ID.
    pub app_id_variable: String,
    /// Repository/environment secret containing the GitHub App private key.
    pub app_private_key_secret: String,
    /// Closed merge policy; arbitrary merge modes and conditions are forbidden.
    pub merge: TapMergePolicy,
    /// Trusted App check that must complete against the exact PR head SHA.
    pub review_completion: ExactHeadReviewCheck,
}

/// Native release channel consumed by the tap adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HomebrewTapSource {
    /// `[stacks.rust.binary_release.preview]`.
    #[serde(rename = "rust.binary_release.preview")]
    RustBinaryReleasePreview,
}

/// Qualified task boundary for generating a formula patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TapUpdaterBinding {
    /// Repository-owned Mise task, never arbitrary argv.
    pub task: String,
    /// Exact Mise configuration and task working directory.
    pub source: MiseTaskSource,
    /// Qualified task placement from the existing named-check contract.
    pub runner: CheckRunner,
    /// Complete tracked task closure: Mise config/includes, task scripts, and
    /// task inputs. The runtime resolver must compare its closure with this
    /// list before invoking the updater; an incomplete declaration fails.
    pub inputs: Vec<String>,
    /// Tool IDs resolved only from the captured tap Mise config and lock.
    /// Runtime qualification must verify the exact pinned declarations there;
    /// these IDs never resolve through Velnor's own tool catalog.
    pub tools: Vec<String>,
    /// Bounded task runtime.
    #[serde(default = "default_updater_timeout")]
    pub timeout_minutes: u32,
}

fn default_updater_timeout() -> u32 {
    u32::from(crate::workflow::timeout::JobTimeout::DEFAULT.minutes())
}

/// The only supported write environment name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TapUpdateEnvironment {
    /// Environment containing the target-scoped App key, referenced only by writer.
    #[serde(rename = "tap-update")]
    TapUpdate,
}

/// The only supported automatic merge operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TapMergePolicy {
    /// Squash only after checks and feedback completion for the exact head.
    #[serde(rename = "squash-after-gates")]
    SquashAfterGates,
}

/// Exact-head completion signal from a configured trusted GitHub App.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExactHeadReviewCheck {
    /// Exact required check-run name emitted by the trusted completion App.
    pub check_name: String,
    /// Variable naming the trusted completion App; runtime compares numeric App ID.
    pub app_id_variable: String,
    /// Every expected reviewer App whose findings need explicit disposition.
    /// The completion check must bind its result to the exact PR head SHA and
    /// report each configured reviewer as complete with no unaddressed findings.
    pub expected_reviewers: Vec<ReviewAppIdentity>,
}

/// Trusted reviewer App identity, referred to by its numeric App ID variable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewAppIdentity {
    /// Variable naming the expected reviewer's numeric GitHub App ID.
    pub app_id_variable: String,
}

impl HomebrewTapConfig {
    /// Validate the closed tap contract and its credential references.
    ///
    /// # Errors
    ///
    /// Returns a config error for unqualified tasks, unsafe paths, malformed
    /// credential references, or unsupported runtime bounds.
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        let key = "distribution.homebrew_tap";
        if !crate::vcs::is_safe_github_repository(&self.repository) {
            return Err(config_error(file, key, "repository", "bad_repository_slug"));
        }
        if !valid_formula_path(&self.formula) {
            return Err(config_error(file, key, "formula", "bad_formula_path"));
        }
        self.updater.validate(file, key)?;
        if self.updater.runner.executor != super::CheckExecutor::Hosted {
            return Err(config_error(
                file,
                key,
                "updater.runner.executor",
                "tap_updater_requires_hosted_runner",
            ));
        }
        if !self.updater.inputs.iter().any(|path| path == &self.formula) {
            return Err(config_error(
                file,
                key,
                "updater.inputs",
                "formula_base_not_an_input",
            ));
        }
        if !valid_app_id_variable(&self.app_id_variable) {
            return Err(config_error(
                file,
                key,
                "app_id_variable",
                "bad_app_id_variable",
            ));
        }
        if !valid_secret_name(&self.app_private_key_secret) {
            return Err(config_error(
                file,
                key,
                "app_private_key_secret",
                "bad_app_private_key_secret",
            ));
        }
        self.review_completion.validate(file, key)?;
        if self.app_id_variable == self.review_completion.app_id_variable
            || self
                .review_completion
                .expected_reviewers
                .iter()
                .any(|reviewer| reviewer.app_id_variable == self.app_id_variable)
        {
            return Err(config_error(
                file,
                key,
                "review_completion",
                "writer_app_must_not_complete_reviews",
            ));
        }
        Ok(())
    }
}

impl TapUpdaterBinding {
    fn validate(&self, file: &str, parent: &str) -> Result<(), ContractError> {
        let bad = |field, problem| config_error(file, parent, field, problem);
        if !is_valid_mise_task_name(&self.task) || self.task.len() > 128 {
            return Err(bad("updater.task", "bad_mise_task"));
        }
        if !self.source.validate() {
            return Err(bad("updater.source", "bad_mise_task_source"));
        }
        self.runner
            .validate(file, &format!("{parent}.updater.runner"))?;
        if self.inputs.is_empty() || !unique_paths(&self.inputs) {
            return Err(bad("updater.inputs", "invalid_task_inputs"));
        }
        if !self
            .inputs
            .iter()
            .any(|path| path == &self.source.mise_config)
        {
            return Err(bad("updater.inputs", "task_source_not_an_input"));
        }
        if self.tools.is_empty() || !unique_names(&self.tools) {
            return Err(bad("updater.tools", "invalid_task_tools"));
        }
        if !crate::workflow::timeout::JobTimeout::is_valid_minutes(self.timeout_minutes) {
            return Err(bad("updater.timeout_minutes", "bad_task_timeout"));
        }
        Ok(())
    }
}

impl ExactHeadReviewCheck {
    fn validate(&self, file: &str, parent: &str) -> Result<(), ContractError> {
        if self.check_name.is_empty()
            || self.check_name.len() > 200
            || self.check_name.trim() != self.check_name
            || self.check_name.contains("${{")
            || self.check_name.chars().any(char::is_control)
        {
            return Err(config_error(
                file,
                parent,
                "review_completion.check_name",
                "bad_review_check_name",
            ));
        }
        if !valid_app_id_variable(&self.app_id_variable) {
            return Err(config_error(
                file,
                parent,
                "review_completion.app_id_variable",
                "bad_review_app_id_variable",
            ));
        }
        if self.expected_reviewers.is_empty() || self.expected_reviewers.len() > 32 {
            return Err(config_error(
                file,
                parent,
                "review_completion.expected_reviewers",
                "invalid_expected_reviewers",
            ));
        }
        let mut expected = BTreeSet::new();
        for reviewer in &self.expected_reviewers {
            if !valid_app_id_variable(&reviewer.app_id_variable)
                || !expected.insert(reviewer.app_id_variable.as_str())
                || reviewer.app_id_variable == self.app_id_variable
            {
                return Err(config_error(
                    file,
                    parent,
                    "review_completion.expected_reviewers",
                    "invalid_expected_reviewer_identity",
                ));
            }
        }
        Ok(())
    }
}

fn config_error(file: &str, parent: &str, field: &str, problem: &str) -> ContractError {
    ContractError::config(file, format!("{parent}.{field}"), problem)
}

fn valid_formula_path(value: &str) -> bool {
    value.starts_with("Formula/")
        && value
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension == "rb")
        && valid_repo_path(value)
        && value.len() <= 512
}

fn valid_repo_path(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('/')
        && value.len() <= 1024
        && value.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        })
}

fn unique_paths(values: &[String]) -> bool {
    let unique: BTreeSet<&str> = values.iter().map(String::as_str).collect();
    unique.len() == values.len() && values.iter().all(|value| valid_repo_path(value))
}

fn unique_names(values: &[String]) -> bool {
    let unique: BTreeSet<&str> = values.iter().map(String::as_str).collect();
    unique.len() == values.len()
        && values.iter().all(|value| {
            !value.is_empty()
                && value.len() <= 128
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'@' | b'+')
                })
        })
}

fn valid_app_id_variable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && !value.contains("TOKEN")
        && !value.contains("SECRET")
        && !value.contains("KEY")
}

fn valid_secret_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_uppercase()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        && value.contains("PRIVATE_KEY")
}

#[cfg(test)]
#[path = "distribution_tests.rs"]
mod tests;

#[cfg(test)]
pub(super) fn valid_tap() -> HomebrewTapConfig {
    tests::valid()
}
