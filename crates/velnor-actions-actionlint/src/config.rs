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

/// Default runner label mirrored for the freshness gate.
///
/// Pinned by `scripts/check-freshness.sh` against the inventory
/// runner default. Rendering never uses this value: the bridge label
/// always comes from [`ActionlintConfigInput::runner_label`], which
/// the orchestrator sets from the effective configured label.
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
    /// Effective configured runner label for the bridge block.
    ///
    /// `None` fails rendering while the pinned release needs the
    /// bridge: the label must come from configuration, never from a
    /// hardcoded default.
    pub runner_label: Option<String>,
    /// Extra scale-set labels appended after the hosted bridge label.
    pub extra_runner_labels: Vec<String>,
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
            runner_label: None,
            extra_runner_labels: Vec::new(),
        }
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
    /// The orchestrator passes the exact declared repo names here.
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

    /// Set the effective configured runner label for the bridge block.
    #[must_use]
    pub fn with_runner_label(mut self, label: impl Into<String>) -> Self {
        self.runner_label = Some(label.into());
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
    let labels = bridge_labels(input)?;
    let yaml = render_bytes(&input.generator_version, &variables, &labels);
    Ok(ActionlintConfigOutput {
        yaml,
        approved_ignores: approved,
        runner_bridge_emitted: !labels.is_empty(),
    })
}

/// Hosted bridge label, then any extra scale-set labels in given order.
fn bridge_labels(input: &ActionlintConfigInput) -> Result<Vec<String>, ActionlintError> {
    let mut labels = Vec::new();
    if let Some(label) = bridge_label(input)? {
        labels.push(label);
    }
    for extra in &input.extra_runner_labels {
        check_extra_label(extra)?;
        if !labels.iter().any(|have| have == extra) {
            labels.push(extra.clone());
        }
    }
    Ok(labels)
}

/// Reject labels that could break the generated YAML block.
fn check_extra_label(label: &str) -> Result<(), ActionlintError> {
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

/// Configured bridge label, if the pinned release needs the bridge.
///
/// The label must be an exact catalog member (the same catalog the
/// config validator enforces); a missing or unlisted label fails
/// closed instead of emitting a hardcoded distro.
fn bridge_label(input: &ActionlintConfigInput) -> Result<Option<String>, ActionlintError> {
    if !input.capabilities.requires_runner_label_bridge() {
        return Ok(None);
    }
    let Some(label) = input.runner_label.clone() else {
        return Err(ActionlintError::InvalidRunnerLabel {
            label: String::new(),
        });
    };
    if !velnor_actions_contract::config::RUNNER_LABEL_CATALOG.contains(&label.as_str()) {
        return Err(ActionlintError::InvalidRunnerLabel { label });
    }
    Ok(Some(label))
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
fn render_bytes(version: &str, variables: &[String], labels: &[String]) -> String {
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
    if !labels.is_empty() {
        yaml.push_str("\nself-hosted-runner:\n  labels:\n");
        for label in labels {
            yaml.push_str("    - ");
            yaml.push_str(label);
            yaml.push('\n');
        }
    }
    yaml
}
