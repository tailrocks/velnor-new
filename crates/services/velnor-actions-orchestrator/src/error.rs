//! Typed orchestration failures with machine-readable problems.

use thiserror::Error;

/// Every failure the coordinator can report.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OrchestratorError {
    /// Repository root discovery failed.
    #[error("root_discovery: {problem}")]
    RootDiscovery {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// The resolved root is not inside a Git working tree.
    #[error("not_work_tree: {problem}")]
    NotWorkTree {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// `.velnor/config.toml` is missing.
    #[error("config_missing: {path}")]
    ConfigMissing {
        /// Missing path as supplied.
        path: String,
    },
    /// Configuration parsing or validation failed for one key path.
    #[error("{file}: {key_path}: {problem}")]
    Config {
        /// Config file path as reported to the user.
        file: String,
        /// Dotted key path within the file.
        key_path: String,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// No default branch: config omits it and `origin/HEAD` is unresolvable.
    #[error("default_branch: {problem}")]
    DefaultBranch {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Velnor-repository policy without the canonical repository identity.
    #[error("identity_rejected: {problem}")]
    IdentityRejected {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// File-index construction failed.
    #[error("discovery: {problem}")]
    Discovery {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Stack detection failed.
    #[error("detection: {problem}")]
    Detection {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Cargo inventory parsing failed.
    #[error("inventory: {problem}")]
    Inventory {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Tool execution for discovery failed without a Cargo diagnostic.
    #[error("preparation_incomplete: {manifest}: {problem}")]
    PreparationIncomplete {
        /// Manifest whose discovery did not complete.
        manifest: String,
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Execution-profile detection failed.
    #[error("profile: {problem}")]
    Profile {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Contract derivation or validation failed.
    #[error("contract: {problem}")]
    Contract {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Workflow rendering failed.
    #[error("render: {problem}")]
    Render {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Actionlint config rendering failed.
    #[error("actionlint: {problem}")]
    Actionlint {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Preview destination refused.
    #[error("preview_refused: {path}: {reason}")]
    PreviewRefused {
        /// Refused path as supplied.
        path: String,
        /// Machine-readable reason.
        reason: String,
    },
    /// Refused to overwrite an existing file.
    #[error("overwrite_refused: {path}")]
    OverwriteRefused {
        /// Existing path as supplied.
        path: String,
    },
    /// Output path is unsafe (symlink, traversal, or escape).
    #[error("unsafe_path: {path}: {reason}")]
    UnsafePath {
        /// Rejected path as supplied.
        path: String,
        /// Machine-readable reason.
        reason: String,
    },
    /// Internal plan/merge request failed.
    #[error("internal: {problem}")]
    Internal {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Staged-tree validation failed before any write.
    #[error("validation: {tool}: {problem}")]
    Validation {
        /// Validator that failed (`actionlint`, `shellcheck`, or `zizmor`).
        tool: String,
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Filesystem IO failed.
    #[error("io: {path}: {problem}")]
    Io {
        /// Path being accessed.
        path: String,
        /// Operating-system error detail.
        problem: String,
    },
    /// An operation was cancelled; never report as success or cache miss.
    #[error("cancelled: {operation}: {detail}")]
    Cancelled {
        /// Cancelled operation name.
        operation: String,
        /// Machine-readable detail.
        detail: String,
    },
    /// A capability or schema is unsupported on this input or platform.
    #[error("unsupported: {capability}: {detail}")]
    Unsupported {
        /// Unsupported capability or schema name.
        capability: String,
        /// Machine-readable detail.
        detail: String,
    },
}

impl OrchestratorError {
    /// Build an IO error for one path.
    #[must_use]
    pub fn io(path: impl Into<String>, problem: impl Into<String>) -> Self {
        Self::Io {
            path: path.into(),
            problem: problem.into(),
        }
    }

    /// Build a config error for one key path.
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

    /// Build a cancellation error for one operation.
    #[must_use]
    pub fn cancelled(operation: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Cancelled {
            operation: operation.into(),
            detail: detail.into(),
        }
    }

    /// Build an unsupported-capability error.
    #[must_use]
    pub fn unsupported(capability: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::Unsupported {
            capability: capability.into(),
            detail: detail.into(),
        }
    }

    /// Whether this error is a cancellation (not a success or cache miss).
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }

    /// Whether this error is an unsupported capability or schema.
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
        Self::Actionlint {
            problem: error.to_string(),
        }
    }
}
#[cfg(test)]
mod tests;
