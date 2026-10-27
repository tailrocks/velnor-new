// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OrchestratorError {
    #[error("root_discovery: {problem}")]
    RootDiscovery { problem: String },
    #[error("not_work_tree: {problem}")]
    NotWorkTree { problem: String },
    #[error("config_missing: {path}")]
    ConfigMissing { path: String },
    #[error("{file}: {key_path}: {problem}")]
    Config {
        file: String,
        key_path: String,
        problem: String,
    },
    #[error("default_branch: {problem}")]
    DefaultBranch { problem: String },
    #[error("identity_rejected: {problem}")]
    IdentityRejected { problem: String },
    #[error("discovery: {problem}")]
    Discovery { problem: String },
    #[error("detection: {problem}")]
    Detection { problem: String },
    #[error("inventory: {problem}")]
    Inventory { problem: String },
    #[error("preparation_incomplete: {manifest}: {problem}")]
    PreparationIncomplete { manifest: String, problem: String },
    #[error("profile: {problem}")]
    Profile { problem: String },
    #[error("contract: {problem}")]
    Contract { problem: String },
    #[error("render: {problem}")]
    Render { problem: String },
    #[error("jactionlint: {problem}")]
    WorkflowLint { problem: String },
    #[error("preview_refused: {path}: {reason}")]
    PreviewRefused { path: String, reason: String },
    #[error("overwrite_refused: {path}")]
    OverwriteRefused { path: String },
    #[error("unsafe_path: {path}: {reason}")]
    UnsafePath { path: String, reason: String },
    #[error("internal: {problem}")]
    Internal { problem: String },
    #[error("validation: {tool}: {problem}")]
    Validation { tool: String, problem: String },
    #[error("io: {path}: {problem}")]
    Io { path: String, problem: String },
    #[error("cancelled: {operation}: {detail}")]
    Cancelled { operation: String, detail: String },
    #[error("unsupported: {capability}: {detail}")]
    Unsupported { capability: String, detail: String },
}

impl OrchestratorError {
    #[must_use]
    pub fn io(path: impl Into<String>, problem: impl Into<String>) -> Self {
        Self::Io {
            path: path.into(),
            problem: problem.into(),
        }
    }

    #[must_use]
    pub fn config(
        file: impl Into<String>,
        key_path: impl Into<String>,
        problem: impl Into<String>,
    ) -> Self {
        Self::Config {
            file: file.into(),
            key_path: key_path.into(),
            problem: problem.into(),
        }
    }

    #[must_use]
    pub fn cancelled(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Cancelled {
            operation: operation.into(),
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn unsupported(capability: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            capability: capability.into(),
            detail: detail.into(),
        }
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    #[must_use]
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported { .. })
    }
}

impl From<velnor_actions_contract::ContractError> for OrchestratorError {
    fn from(error: velnor_actions_contract::ContractError) -> Self {
        match error {
            velnor_actions_contract::ContractError::Config {
                file,
                key_path,
                problem,
            } => Self::Config {
                file,
                key_path,
                problem,
            },
            velnor_actions_contract::ContractError::UnsupportedSchema {
                field,
                found,
                expected,
            } => Self::Unsupported {
                capability: field.to_owned(),
                detail: format!("version {found}, expected {expected}"),
            },
            other => Self::Contract {
                problem: other.to_string(),
            },
        }
    }
}

impl From<velnor_actions_workflow_renderer::RenderError> for OrchestratorError {
    fn from(error: velnor_actions_workflow_renderer::RenderError) -> Self {
        Self::Render {
            problem: error.to_string(),
        }
    }
}

impl From<velnor_actions_actionlint::ActionlintError> for OrchestratorError {
    fn from(error: velnor_actions_actionlint::ActionlintError) -> Self {
        Self::WorkflowLint {
            problem: error.to_string(),
        }
    }
}

impl From<velnor_actions_workflow_renderer::plan_sources::PlanSourcesError> for OrchestratorError {
    fn from(error: velnor_actions_workflow_renderer::plan_sources::PlanSourcesError) -> Self {
        Self::Contract {
            problem: format!("{error:?}"),
        }
    }
}
