// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::{Display, Formatter, Result as FmtResult};
use velnor_actions_contract::ContractError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ActionlintError {
    InvalidGeneratorVersion {
        problem: String,
    },

    InvalidConfigVariable {
        name: String,
    },

    UnknownAction {
        uses: String,
    },

    InvalidPin {
        uses: String,

        problem: String,
    },

    OverrideRejected {
        action: String,

        problem: String,
    },

    UnknownActionInput {
        action: String,

        input: String,
    },

    MissingActionInput {
        action: String,

        input: String,
    },

    InvalidActionInput {
        action: String,

        input: String,

        problem: String,
    },

    UnsupportedSyntax {
        syntax: &'static str,
    },

    InvalidRunnerLabel {
        label: String,
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
        }
    }
}

impl std::error::Error for ActionlintError {}

impl ActionlintError {
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
                ".github/jactionlint.yaml",
                "jactionlint",
                error.config_problem(),
            ),
        }
    }
}
