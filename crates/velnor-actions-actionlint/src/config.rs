//! Deterministic `.github/jactionlint.yaml` generator from typed input.
//!
//! Output is fixed template bytes in a fixed key order: the Velnor marker,
//! `profile`, `generated-files: report`, sorted `config-variables`, and the
//! `self-hosted-runner` labels the pinned linter cannot know. Never invokes
//! `jactionlint --init`; never fetches a template. The file carries no
//! `rules`, `ignores`, `paths`, `baseline` or `online` key: no diagnostic is
//! ever turned off from generated configuration.

use crate::{ActionlintError, is_known_hosted_label};
use std::collections::BTreeSet;
use velnor_actions_contract::WorkflowLintProfile;

pub use crate::zizmor::{
    ZizmorConfigInput, ZizmorConfigOutput, ZizmorWorkflowText, render_zizmor_yaml,
};

/// Directory prefix every declared workflow path must use.
const WORKFLOWS_PREFIX: &str = ".github/workflows/";

/// Typed input for jactionlint config generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JactionlintConfigInput {
    /// Exact generator version for the header comment.
    pub generator_version: String,
    /// `[workflow.lint] profile`.
    pub profile: WorkflowLintProfile,
    /// Declared repository configuration variable names.
    pub config_variables: Vec<String>,
    /// Configured scale-set labels and the hosted labels generated jobs use.
    ///
    /// Rendering keeps only the labels the pinned linter does not know:
    /// declaring a GitHub-hosted label as self-hosted would make it treat
    /// those jobs as self-hosted.
    pub runner_labels: Vec<String>,
}

impl JactionlintConfigInput {
    /// Minimal input: version only, `correctness` profile.
    #[must_use]
    pub fn new(generator_version: impl Into<String>) -> Self {
        Self {
            generator_version: generator_version.into(),
            profile: WorkflowLintProfile::default(),
            config_variables: Vec::new(),
            runner_labels: Vec::new(),
        }
    }

    /// Select the jactionlint profile.
    #[must_use]
    pub const fn with_profile(mut self, profile: WorkflowLintProfile) -> Self {
        self.profile = profile;
        self
    }

    /// Append a repository configuration variable name.
    #[must_use]
    pub fn with_config_variable(mut self, name: impl Into<String>) -> Self {
        self.config_variables.push(name.into());
        self
    }

    /// Append multiple repository configuration variable names.
    ///
    /// Order and duplicates do not matter: rendering sorts and dedupes.
    #[must_use]
    pub fn with_config_variables(
        mut self,
        names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        for name in names {
            self.config_variables.push(name.into());
        }
        self
    }

    /// Append runner labels; order is kept, duplicates are dropped.
    #[must_use]
    pub fn with_runner_labels(
        mut self,
        labels: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        for label in labels {
            self.runner_labels.push(label.into());
        }
        self
    }
}

/// Render deterministic jactionlint config bytes from typed input.
///
/// # Errors
///
/// Returns [`ActionlintError`] on an unsafe generator version, an invalid
/// variable name, or a runner label that could break the YAML block.
pub fn render_jactionlint_yaml(input: &JactionlintConfigInput) -> Result<String, ActionlintError> {
    check_generator_version(&input.generator_version)?;
    let variables = sorted_variables(&input.config_variables)?;
    let labels = self_hosted_labels(&input.runner_labels)?;
    Ok(render_bytes(
        &input.generator_version,
        input.profile,
        &variables,
        &labels,
    ))
}

/// Labels in given order, deduplicated, without labels the linter knows.
fn self_hosted_labels(labels: &[String]) -> Result<Vec<String>, ActionlintError> {
    let mut kept: Vec<String> = Vec::new();
    for label in labels {
        check_label(label)?;
        if !is_known_hosted_label(label) && !kept.iter().any(|have| have == label) {
            kept.push(label.clone());
        }
    }
    Ok(kept)
}

/// Reject labels that could break the generated YAML block.
fn check_label(label: &str) -> Result<(), ActionlintError> {
    let ok = !label.is_empty()
        && label.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        });
    if ok {
        Ok(())
    } else {
        Err(ActionlintError::InvalidRunnerLabel {
            label: label.to_owned(),
        })
    }
}

/// Reject empty versions and header-injection characters.
pub(crate) fn check_generator_version(version: &str) -> Result<(), ActionlintError> {
    if version.is_empty() {
        return Err(ActionlintError::InvalidGeneratorVersion {
            problem: "empty_version".to_owned(),
        });
    }
    if !version.is_ascii() || version.chars().any(char::is_whitespace) {
        return Err(ActionlintError::InvalidGeneratorVersion {
            problem: format!("unsafe_version:{version}"),
        });
    }
    Ok(())
}

/// Validate, sort, and dedupe configuration variable names.
fn sorted_variables(names: &[String]) -> Result<Vec<String>, ActionlintError> {
    let mut sorted = BTreeSet::new();
    for name in names {
        if !is_valid_variable(name) {
            return Err(ActionlintError::InvalidConfigVariable { name: name.clone() });
        }
        sorted.insert(name.clone());
    }
    Ok(sorted.into_iter().collect())
}

/// GitHub variable names: ASCII word chars, no leading digit, no `GITHUB_`.
fn is_valid_variable(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() && first != '_' {
        return false;
    }
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !name.starts_with("GITHUB_")
}

/// Workflow paths are narrow: one file under `.github/workflows/`.
#[expect(
    clippy::case_sensitive_file_extension_comparisons,
    reason = "generated workflow files use lowercase extensions only"
)]
pub(crate) fn is_valid_workflow_path(path: &str) -> bool {
    let Some(rest) = path.strip_prefix(WORKFLOWS_PREFIX) else {
        return false;
    };
    if rest.is_empty() || rest.contains("..") || rest.starts_with('/') {
        return false;
    }
    if !rest.ends_with(".yml") && !rest.ends_with(".yaml") {
        return false;
    }
    !rest
        .chars()
        .any(|c| c.is_whitespace() || matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '!' | '\\'))
}

/// Render the fixed template bytes.
fn render_bytes(
    version: &str,
    profile: WorkflowLintProfile,
    variables: &[String],
    labels: &[String],
) -> String {
    let mut yaml = format!(
        "# Generated by Velnor Actions {version}; edit .velnor/config.toml and regenerate.\n\n"
    );
    yaml.push_str("profile: ");
    yaml.push_str(profile.as_str());
    yaml.push('\n');
    // The Velnor marker matches jactionlint's generated-file heuristic; its
    // default `skip-policy` would silence the policy rules on every file
    // Velnor owns, so generated files are linted like any other file.
    yaml.push_str("generated-files: report\n");
    if variables.is_empty() {
        yaml.push_str("config-variables: []\n");
    } else {
        yaml.push_str("config-variables:\n");
        for name in variables {
            yaml.push_str("  - ");
            yaml.push_str(name);
            yaml.push('\n');
        }
    }
    if !labels.is_empty() {
        yaml.push_str("self-hosted-runner:\n  labels:\n");
        for label in labels {
            yaml.push_str("    - ");
            yaml.push_str(label);
            yaml.push('\n');
        }
    }
    yaml
}
