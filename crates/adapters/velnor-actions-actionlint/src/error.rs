//! Typed errors for actionlint configuration and action-schema validation.

use std::fmt::{Display, Formatter, Result as FmtResult};
use velnor_actions_contract::ContractError;

/// Errors for actionlint config rendering, capability gates, pinned action
/// refs, overrides, action inputs, and lint-tool pins.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActionlintError {
    /// Generator version for the config header is missing or unsafe.
    InvalidGeneratorVersion {
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// A `config-variables` entry is not a valid variable name.
    InvalidConfigVariable {
        /// Rejected variable name.
        name: String,
    },
    /// A declared workflow path is not a narrow `.github/workflows` file.
    InvalidWorkflowPath {
        /// Rejected path.
        path: String,
    },
    /// Consumer policy forbids every ignore entry.
    ConsumerIgnoreForbidden {
        /// Rule of the rejected entry.
        rule: String,
    },
    /// An ignore path is broad (glob, directory, or outside workflows).
    BroadIgnore {
        /// Rejected path.
        path: String,
    },
    /// A protected-policy ignore entry is malformed.
    InvalidIgnore {
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// A `uses:` value names an action outside the 9-entry allowlist.
    UnknownAction {
        /// Rejected `uses:` value.
        uses: String,
    },
    /// A `uses:` value has a malformed pin (non-SHA ref, bad comment).
    InvalidPin {
        /// Rejected `uses:` value.
        uses: String,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// A per-project pin override failed allowlist/catalog validation.
    OverrideRejected {
        /// Override key (`owner/repo[/path]`).
        action: String,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// An action input is not in the action's allowlisted schema.
    UnknownActionInput {
        /// Action key (`owner/repo[/path]`).
        action: String,
        /// Rejected input name.
        input: String,
    },
    /// A required action input is missing.
    MissingActionInput {
        /// Action key (`owner/repo[/path]`).
        action: String,
        /// Missing input name.
        input: String,
    },
    /// An action input value is empty or multi-line.
    InvalidActionInput {
        /// Action key (`owner/repo[/path]`).
        action: String,
        /// Input name.
        input: String,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// Requested workflow syntax is not qualified for the pinned actionlint.
    UnsupportedSyntax {
        /// Requested syntax identifier.
        syntax: &'static str,
    },
    /// The bridge runner label is missing or outside the label catalog.
    InvalidRunnerLabel {
        /// Rejected label (empty when unset).
        label: String,
    },
    /// A lint-tool version is not the required exact pin.
    InvalidToolVersion {
        /// Tool identifier.
        tool: &'static str,
        /// Rejected version string.
        version: String,
    },
}

impl Display for ActionlintError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidGeneratorVersion { problem } => {
                write!(f, "invalid_generator_version: {problem}")
            }
            Self::InvalidConfigVariable { name } => {
                write!(f, "invalid_config_variable: {name}")
            }
            Self::InvalidWorkflowPath { path } => {
                write!(f, "invalid_workflow_path: {path}")
            }
            Self::ConsumerIgnoreForbidden { rule } => {
                write!(f, "consumer_ignore_forbidden: {rule}")
            }
            Self::BroadIgnore { path } => write!(f, "broad_ignore: {path}"),
            Self::InvalidIgnore { problem } => write!(f, "invalid_ignore: {problem}"),
            Self::UnknownAction { uses } => write!(f, "unknown_action: {uses}"),
            Self::InvalidPin { uses, problem } => {
                write!(f, "invalid_pin: {uses}: {problem}")
            }
            Self::OverrideRejected { action, problem } => {
                write!(f, "override_rejected: {action}: {problem}")
            }
            Self::UnknownActionInput { action, input } => {
                write!(f, "unknown_action_input: {action}: {input}")
            }
            Self::MissingActionInput { action, input } => {
                write!(f, "missing_action_input: {action}: {input}")
            }
            Self::InvalidActionInput {
                action,
                input,
                problem,
            } => write!(f, "invalid_action_input: {action}: {input}: {problem}"),
            Self::UnsupportedSyntax { syntax } => write!(f, "unsupported_syntax: {syntax}"),
            Self::InvalidRunnerLabel { label } => write!(f, "invalid_runner_label: {label}"),
            Self::InvalidToolVersion { tool, version } => {
                write!(f, "invalid_tool_version: {tool}: {version}")
            }
        }
    }
}

impl std::error::Error for ActionlintError {}

impl ActionlintError {
    /// Config-file problem code for the contract error mapping.
    fn config_problem(&self) -> String {
        self.to_string()
    }
}

impl From<ActionlintError> for ContractError {
    fn from(error: ActionlintError) -> Self {
        match &error {
            ActionlintError::OverrideRejected { action, .. } => Self::config(
                ".velnor/config.toml",
                format!("actions.overrides.{action}"),
                error.config_problem(),
            ),
            ActionlintError::UnknownAction { .. }
            | ActionlintError::InvalidPin { .. }
            | ActionlintError::UnknownActionInput { .. }
            | ActionlintError::MissingActionInput { .. }
            | ActionlintError::InvalidActionInput { .. } => {
                Self::identity("uses", error.config_problem())
            }
            _ => Self::config(
                ".github/actionlint.yaml",
                "actionlint",
                error.config_problem(),
            ),
        }
    }
}
