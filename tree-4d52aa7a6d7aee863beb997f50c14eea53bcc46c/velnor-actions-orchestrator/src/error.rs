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
    /// Filesystem IO failed.
    #[error("io: {path}: {problem}")]
    Io {
        /// Path being accessed.
        path: String,
        /// Operating-system error detail.
        problem: String,
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
