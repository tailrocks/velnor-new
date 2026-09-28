//! Deterministic `.github/actionlint.yaml` generator from typed input.
//!
//! Output is fixed template bytes: version header, sorted
//! `config-variables`, and the runner-label bridge while the pinned
//! release needs it. Never invokes `actionlint -init-config`; never
//! fetches a template. Zero ignored diagnostics by default.

use crate::{ActionlintCapabilities, ActionlintError};
use std::collections::BTreeSet;

pub use crate::zizmor::{
    ZizmorConfigInput, ZizmorConfigOutput, ZizmorWorkflowText, render_zizmor_yaml,
};

/// Runner label emitted only as an actionlint compatibility bridge.
pub const RUNNER_LABEL_BRIDGE: &str = "ubuntu-26.04";

/// Directory prefix every declared workflow path must use.
const WORKFLOWS_PREFIX: &str = ".github/workflows/";

/// Policy selecting whether narrow ignore approvals are accepted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum IgnorePolicy {
    /// Consumer repositories: every ignore entry is rejected.
    #[default]
    Consumer,
    /// Velnor protected policy: narrow approved ignores accepted.
    VelnorProtected,
}

/// One narrow diagnostic-ignore approval.
///
/// Narrow means: one named rule, one exact declared workflow file, and a
/// recorded justification. Broad paths are rejected, never rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoreEntry {
    /// Actionlint rule name (e.g. `shellcheck`).
    pub rule: String,
    /// Exact declared workflow path the approval covers.
    pub path: String,
    /// Why this diagnostic is approved for this file.
    pub justification: String,
}

/// Typed input for actionlint config generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionlintConfigInput {
    /// Exact generator version for the header comment.
    pub generator_version: String,
    /// Declared repository configuration variable names.
    pub config_variables: Vec<String>,
    /// Generated workflow paths (validation context for ignores).
    pub workflow_paths: Vec<String>,
    /// Requested ignore approvals (empty by default).
    pub ignores: Vec<IgnoreEntry>,
    /// Ignore acceptance policy.
    pub policy: IgnorePolicy,
    /// Capability flags of the pinned actionlint release.
    pub capabilities: ActionlintCapabilities,
}

impl ActionlintConfigInput {
    /// Minimal input: version only, consumer policy, pinned capabilities.
    #[must_use]
    pub fn new(generator_version: impl Into<String>) -> Self {
        Self {
            generator_version: generator_version.into(),
            config_variables: Vec::new(),
            workflow_paths: Vec::new(),
            ignores: Vec::new(),
            policy: IgnorePolicy::Consumer,
            capabilities: ActionlintCapabilities::for_pinned(),
        }
    }

    /// Append a repository configuration variable name.
    #[must_use]
    pub fn with_config_variable(mut self, name: impl Into<String>) -> Self {
        self.config_variables.push(name.into());
        self
    }

    /// Append a generated workflow path.
    #[must_use]
    pub fn with_workflow_path(mut self, path: impl Into<String>) -> Self {
        self.workflow_paths.push(path.into());
        self
    }

    /// Append an ignore approval request.
    #[must_use]
    pub fn with_ignore(mut self, entry: IgnoreEntry) -> Self {
        self.ignores.push(entry);
        self
    }

    /// Select the ignore acceptance policy.
    #[must_use]
    pub const fn with_policy(mut self, policy: IgnorePolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Override the capability flags.
    #[must_use]
    pub const fn with_capabilities(mut self, capabilities: ActionlintCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }
}

/// Deterministic render output: config bytes plus validated approvals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionlintConfigOutput {
    /// Exact `.github/actionlint.yaml` bytes.
    pub yaml: String,
    /// Validated narrow ignores, sorted by `(path, rule)`.
    pub approved_ignores: Vec<IgnoreEntry>,
    /// Whether the bytes carry the runner-label bridge block.
    pub runner_bridge_emitted: bool,
}

/// Render deterministic actionlint config bytes from typed input.
///
/// # Errors
///
/// Returns [`ActionlintError`] on an unsafe generator version, invalid
/// variable or workflow path, forbidden consumer ignore, broad ignore,
/// or malformed protected-policy ignore entry.
pub fn render_actionlint_yaml(
    input: &ActionlintConfigInput,
) -> Result<ActionlintConfigOutput, ActionlintError> {
    check_generator_version(&input.generator_version)?;
    let variables = sorted_variables(&input.config_variables)?;
    let workflows = sorted_workflows(&input.workflow_paths)?;
    let approved = approved_ignores(input, &workflows)?;
    let yaml = render_bytes(&input.generator_version, &variables, input.capabilities);
    Ok(ActionlintConfigOutput {
        yaml,
        approved_ignores: approved,
        runner_bridge_emitted: input.capabilities.requires_runner_label_bridge(),
    })
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

/// Validate, sort, and dedupe workflow paths.
fn sorted_workflows(paths: &[String]) -> Result<BTreeSet<String>, ActionlintError> {
    let mut sorted = BTreeSet::new();
    for path in paths {
        if !is_valid_workflow_path(path) {
            return Err(ActionlintError::InvalidWorkflowPath { path: path.clone() });
        }
        sorted.insert(path.clone());
    }
    Ok(sorted)
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

/// Validate ignore requests against policy and declared workflows.
fn approved_ignores(
    input: &ActionlintConfigInput,
    workflows: &BTreeSet<String>,
) -> Result<Vec<IgnoreEntry>, ActionlintError> {
    if input.policy == IgnorePolicy::Consumer {
        if let Some(entry) = input.ignores.first() {
            return Err(ActionlintError::ConsumerIgnoreForbidden {
                rule: entry.rule.clone(),
            });
        }
        return Ok(Vec::new());
    }
    let mut approved = BTreeSet::new();
    for entry in &input.ignores {
        check_ignore_entry(entry, workflows)?;
        approved.insert((entry.path.clone(), entry.rule.clone()));
    }
    Ok(approved
        .into_iter()
        .filter_map(|(path, rule)| {
            input
                .ignores
                .iter()
                .find(|entry| entry.path == path && entry.rule == rule)
                .cloned()
        })
        .collect())
}

/// Check one protected-policy ignore entry for narrowness and justification.
fn check_ignore_entry(
    entry: &IgnoreEntry,
    workflows: &BTreeSet<String>,
) -> Result<(), ActionlintError> {
    if entry.rule.is_empty()
        || !entry
            .rule
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err(ActionlintError::InvalidIgnore {
            problem: format!("invalid_rule:{}", entry.rule),
        });
    }
    if is_broad_path(&entry.path) {
        return Err(ActionlintError::BroadIgnore {
            path: entry.path.clone(),
        });
    }
    if !workflows.contains(&entry.path) {
        return Err(ActionlintError::InvalidIgnore {
            problem: format!("unknown_workflow_path:{}", entry.path),
        });
    }
    if entry.justification.trim().is_empty() {
        return Err(ActionlintError::InvalidIgnore {
            problem: format!("missing_justification:{}:{}", entry.rule, entry.path),
        });
    }
    Ok(())
}

/// Broad means glob, directory, escape, or anything outside workflows.
fn is_broad_path(path: &str) -> bool {
    !is_valid_workflow_path(path)
}

/// Render the fixed template bytes with sorted variables.
fn render_bytes(
    version: &str,
    variables: &[String],
    capabilities: ActionlintCapabilities,
) -> String {
    let mut yaml = format!(
        "# Generated by Velnor Actions {version}; edit .velnor/config.toml and regenerate.\n\n"
    );
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
    if capabilities.requires_runner_label_bridge() {
        yaml.push_str("\nself-hosted-runner:\n  labels:\n    - ");
        yaml.push_str(RUNNER_LABEL_BRIDGE);
        yaml.push('\n');
    }
    yaml
}
