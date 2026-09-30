//! Pure typed-IR to GitHub Actions YAML rendering.
//!
//! Consumes contract workflow IR plus validated command strings only. No
//! subprocesses, no shell construction beyond quoting fixed argv, and no
//! stack-specific logic: tool pins and command vectors arrive validated.

mod artifact_paths;
mod cache_steps;
mod candidate;
pub mod closure;
mod commands;
mod document;
mod final_steps;
pub mod guard;
pub mod marker;
mod matrix;
pub mod msrv;
pub mod overlap;
pub mod plan_format;
pub mod preseed;
pub mod render;
pub mod setup;
pub mod steps;
mod support;
pub mod task_steps;
pub mod toolchain_env;
pub mod yaml;

pub use artifact_paths::{
    CANDIDATE_OUTPUT_DIR_EXPR, CANDIDATE_STAGE_DIR_EXPR, PRESEED_OUTPUT_DIR_EXPR,
    PRESEED_STAGE_DIR_EXPR,
};
pub use candidate::{
    VERIFY_MANIFEST_NAME, candidate_artifact_name, candidate_manifest_verify_script,
    candidate_manifest_verify_step, check_release_build,
};
pub use closure::{
    CHECK_GENERATED_NAME, DOWNLOAD_PLAN_NAME, FRESHNESS_OUTDIR, HelperProvenance,
    PLAN_ARTIFACT_NAME, PLAN_ARTIFACT_PATH, PUBLISH_PLAN_NAME, SEED_REMEDIATION,
    download_plan_step, freshness_step, provision_acquire_step, publish_plan_step,
};
pub use commands::{
    check_no_bare_cargo, has_bare_env_expansion, join_argv_for_run, quote_env_path_for_run,
    quote_run_arg, quote_run_line_env_paths, validate_command_argv, validate_env,
};
pub use guard::{SafeTreePath, check_no_symlink, join_within_root, validate_tree_path};
pub use marker::{
    MARKER_PREFIX, MARKER_SUFFIX, check_first_line, marker_for_version, validate_version,
    with_marker,
};
pub use preseed::{
    PRESEED_ARTIFACT_NAME, PRESEED_BUILD_NAME, PRESEED_BUILD_OUTPUT, PRESEED_DOWNLOAD_NAME,
    PRESEED_DOWNLOADED_BINARY, PRESEED_MANIFEST_FILE, PRESEED_MANIFEST_NAME, PRESEED_OUTPUT_DIR,
    PRESEED_STAGE_DIR, PRESEED_STAGE_NAME, PRESEED_UPLOAD_NAME, PRESEED_VERIFY_NAME,
    PreseedStageSource, preseed_build_step, preseed_download_step, preseed_manifest_script,
    preseed_manifest_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};
pub use render::{
    ACTIONLINT_PATH, ALINT_USES, CANDIDATE_JOB_ID, CONCURRENCY_CANCEL, CONCURRENCY_GROUP,
    CandidateSpec, EXPECTED_PR_TYPES, FINAL_CONDITION, FINAL_DISPLAY_NAME, FINAL_JOB_ID,
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV, MatrixSource, MiseSetup,
    PLAN_ID_OUTPUT, PLAN_JOB_ID, PLAN_STEP_ID, RUN_KEY_OUTPUT, RenderContext, RenderedFile,
    RenderedTree, TASK_JOB_ID, ValidatorCommand, WORKFLOW_PATH, render_tree, render_workflow_ir,
    render_workflow_ir_strict,
};
pub use setup::{MISE_ACTION_NAME, SETUP_MISE_NAME, mise_setup_step};
pub use steps::{
    ACQUIRE_NAME, ASSET_SHA_ENV, ASSET_URL_ENV, CompileDriver, DENY_STEP_NAME, FORBIDDEN_TOKENS,
    INTERNAL_OP_ENV, MACHETE_STEP_NAME, MATRIX_REPORT_UPLOAD_NAME, MERGE_OPERATION, PLAN_OPERATION,
    REQUEST_DIR_PREFIX, REQUEST_FILE_ENV, RUN_KEY_EXPR, STAGED_BINARY_PREFIX,
    WRITE_REQUEST_OPERATION, acquire_velnor_step, action_step, check_cache_step_order,
    check_mbx_gating, checkout_step, internal_step, lane_cargo_target_env,
    matrix_report_upload_step, mbx_step_for_driver, merge_step, plan_step,
    scan_for_private_subcommands, shell_step, validate_uses, write_request_step,
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
