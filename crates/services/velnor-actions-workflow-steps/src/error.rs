//! Pure rendering failures; no IO is performed.

use std::fmt::{Display, Formatter};

/// Pure rendering failures; no IO is performed.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderError {
    /// Contract IR validation failed.
    Contract(velnor_actions_contract::ContractError),
    /// Workflow IR violates a renderer-enforced invariant.
    InvalidWorkflow(String),
    /// Policy gate rejected the render request.
    PolicyRejected {
        /// Policy name as rendered.
        policy: String,
        /// Machine-readable problem code plus detail.
        problem: String,
    },
    /// Generator version is malformed.
    BadVersion(String),
    /// First-line marker is missing or inexact.
    BadMarker {
        /// Expected marker line.
        expected: String,
        /// Marker line found.
        found: String,
    },
    /// Action reference is not a pinned allowlist entry.
    BadActionRef(String),
    /// Command argv violates fixed-vector policy.
    BadCommand(String),
    /// Rendered text would leak a private subcommand token.
    PrivateSubcommand(String),
    /// Output path is unsafe (traversal, absolute, or symlink).
    UnsafePath(String),
}

impl Display for RenderError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Contract(err) => write!(f, "contract: {err}"),
            Self::InvalidWorkflow(problem) => write!(f, "invalid workflow: {problem}"),
            Self::PolicyRejected { policy, problem } => {
                write!(f, "policy {policy} rejected: {problem}")
            }
            Self::BadVersion(version) => write!(f, "bad version: {version}"),
            Self::BadMarker { expected, found } => {
                write!(f, "bad marker: expected {expected:?}, found {found:?}")
            }
            Self::BadActionRef(problem) => write!(f, "bad action ref: {problem}"),
            Self::BadCommand(problem) => write!(f, "bad command: {problem}"),
            Self::PrivateSubcommand(token) => {
                write!(f, "private subcommand token: {token}")
            }
            Self::UnsafePath(problem) => write!(f, "unsafe path: {problem}"),
        }
    }
}

impl std::error::Error for RenderError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Contract(err) => Some(err),
            _ => None,
        }
    }
}
