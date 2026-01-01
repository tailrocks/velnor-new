//! Validated workflow-step vocabulary: construction, validation, scrubbing.
//!
//! Fixed step templates over validated command strings, action refs, and
//! environment maps: names, env keys, and payload shapes are fixed here
//! and argv arrives validated. Credential scrubbing is on by construction.

pub mod action_ref;
pub mod artifact_paths;
pub mod commands;
mod error;
pub mod expressions;
pub mod setup;
pub mod steps;
mod steps_artifact;
mod steps_internal;
pub mod toolchain_env;

pub use action_ref::{ALINT_BINARY_VERSION, ALINT_USES};
pub use artifact_paths::{
    CANDIDATE_OUTPUT_DIR_EXPR, CANDIDATE_STAGE_DIR_EXPR, PRESEED_OUTPUT_DIR_EXPR,
    PRESEED_STAGE_DIR_EXPR,
};
pub use commands::{
    check_no_bare_cargo, has_bare_env_expansion, join_argv_for_run, quote_env_path_for_run,
    quote_run_arg, quote_run_line_env_paths, validate_command_argv, validate_env,
};
pub use setup::{MISE_ACTION_NAME, MiseSetup, SETUP_MISE_NAME, mise_setup_step};
pub use steps::{
    ACQUIRE_NAME, ARTIFACT_EXPORT_OPERATION, ASSET_SHA_ENV, ASSET_URL_ENV,
    BASELINE_PUBLISH_UPLOAD_NAME, CRATE_REPORT_UPLOAD_NAME, DENY_STEP_NAME, FORBIDDEN_TOKENS,
    INTERNAL_OP_ENV, MACHETE_STEP_NAME, MATRIX_REPORT_UPLOAD_NAME, MERGE_OPERATION, PLAN_OPERATION,
    PUBLISH_OPERATION, PUBLISH_STEP_ID, RELEASE_COMMIT_ENV, REQUEST_DIR_PREFIX, REQUEST_FILE_ENV,
    RUN_KEY_EXPR, STAGED_BINARY_PREFIX, WRITE_PRESEED_MANIFEST_OPERATION, WRITE_REQUEST_OPERATION,
    acquire_velnor_step, action_step, action_step_with_env, ambient_shell_step,
    artifact_build_upload_step, artifact_export_step, baseline_publish_upload_step, checkout_step,
    crate_job_report_upload_step, internal_step, lane_cargo_target_env, matrix_report_upload_step,
    merge_step, plan_step, publish_step, scan_for_private_subcommands, shell_step, validate_uses,
    write_request_step,
};

pub use error::RenderError;
