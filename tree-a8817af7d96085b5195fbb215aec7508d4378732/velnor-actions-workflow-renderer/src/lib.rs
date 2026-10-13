//! Pure typed-IR to GitHub Actions YAML rendering.
//!
//! Consumes contract workflow IR plus validated command strings only. No
//! subprocesses, no shell construction beyond quoting fixed argv, and no
//! stack-specific logic: tool pins and command vectors arrive validated.

mod cache_steps;
mod commands;
mod document;
pub mod guard;
pub mod marker;
pub mod render;
pub mod steps;
mod support;
pub mod yaml;

pub use commands::{join_argv_for_run, quote_run_arg, validate_command_argv, validate_env};
pub use guard::{SafeTreePath, check_no_symlink, join_within_root, validate_tree_path};
pub use marker::{
    MARKER_PREFIX, MARKER_SUFFIX, check_first_line, marker_for_version, validate_version,
    with_marker,
};
pub use render::{
    ACTIONLINT_PATH, ALINT_JOB_ID, ALINT_USES, CANDIDATE_JOB_ID, CONCURRENCY_CANCEL,
    CONCURRENCY_GROUP, CandidateSpec, EXPECTED_PR_TYPES, FINAL_CONDITION, FINAL_DISPLAY_NAME,
    FINAL_JOB_ID, PLAN_JOB_ID, POLICY_JOB_ID, PolicyCommand, RenderContext, RenderedFile,
    RenderedTree, TASK_JOB_ID, WORKFLOW_PATH, render_tree, render_workflow_ir,
};
pub use steps::{
    ASSET_SHA_ENV, ASSET_URL_ENV, FORBIDDEN_TOKENS, INTERNAL_OP_ENV, MERGE_OPERATION,
    PLAN_OPERATION, REQUEST_DIR_PREFIX, REQUEST_FILE_ENV, STAGED_BINARY_PREFIX,
    acquire_velnor_step, action_step, checkout_step, internal_step, merge_step, plan_step,
    scan_for_private_subcommands, shell_step, validate_uses,
};
pub use yaml::{Yaml, quote_scalar, render_yaml};

use std::fmt::{Display, Formatter};

/// Renderer implementation version (typed Gate-2 renderer).
pub const RENDERER_VERSION: u32 = 2;

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
