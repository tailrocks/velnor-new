//! Typed-IR to YAML rendering for CI, release, and freshness workflows.
//!
//! Validated IR plus fixed argv in, marked YAML out: no subprocesses, no
//! stack or tool branching, quoting-only shell shaping.

mod action_ref;
pub mod agents_md;
mod artifact_paths;
pub mod cache_elect;
pub mod cache_p08;
mod cache_p08_detect;
mod cache_steps;
mod candidate;
pub mod closure;
mod closure_paths;
mod commands;
mod composite;
mod document;
mod document_lanes;
mod document_steps;
mod error;
mod expressions;
mod final_steps;
pub mod freshness;
pub mod guard;
mod lane_share;
mod lane_share_sections;
pub mod lane_target;
pub mod marker;
mod matrix;
mod matrix_output_mode;
mod mbx_gc_policy;
pub mod msrv;
pub mod overlap;
pub mod plan_format;
pub mod preseed;
mod preseed_closure;
pub mod release_checkout_gates;
pub mod release_config;
pub mod release_gates;
pub mod release_jobs;
pub mod release_permissions;
pub mod release_spec;
pub mod release_tree;
pub mod render;
mod runs_on;
pub mod schema2;
pub mod setup;
pub mod steps;
mod steps_artifact;
mod steps_internal;
mod steps_plain;
mod support;
pub mod tofu_cache;
mod tool_seed;
mod tool_seed_admission;
#[cfg(test)]
mod tool_seed_test_support;
pub mod toolchain_env;
pub mod tree;
mod verification_jobs;
mod workflow_policy;
mod workflow_size;
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
    PRESEED_DOWNLOADED_BINARY, PRESEED_MANIFEST_BINARY_ENV, PRESEED_MANIFEST_FILE,
    PRESEED_MANIFEST_NAME, PRESEED_MANIFEST_OUT_ENV, PRESEED_MANIFEST_TARGET_ENV,
    PRESEED_MANIFEST_TOOLCHAIN_ENV, PRESEED_STAGE_DIR, PRESEED_STAGE_NAME, PRESEED_UPLOAD_NAME,
    PRESEED_VERIFY_MANIFEST_NAME, PRESEED_VERIFY_NAME, PreseedStageSource, preseed_build_step,
    preseed_download_step, preseed_manifest_step, preseed_manifest_verify_script,
    preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};
pub use render::{
    ACTIONLINT_PATH, AGENTS_MD_PATH, ALINT_BINARY_VERSION, ALINT_USES, CANDIDATE_JOB_ID,
    CLAUDE_MD_PATH, CLAUDE_MD_TARGET, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, COVERED_TASKS_OUTPUT,
    CandidateSpec, EXPECTED_PR_TYPES, FINAL_CONDITION, FINAL_DISPLAY_NAME, FINAL_JOB_ID,
    MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV, MATRIX_OUTPUT_ENV, MatrixSource, MiseSetup,
    PLAN_ID_OUTPUT, PLAN_JOB_ID, PLAN_STEP_ID, PUBLISH_JOB_ID, RUN_KEY_OUTPUT, RenderContext,
    RenderedFile, RenderedSymlink, RenderedTree, TASK_JOB_ID, ValidatorCommand, WORKFLOW_PATH,
    action_pins, finalize_jobs, render_workflow_ir, render_workflow_ir_strict,
};
pub use schema2::{
    GeneratorReleasePins, MbxQualificationPins, Schema2WorkflowRequest, render_schema2_workflows,
};
pub use setup::{MISE_ACTION_NAME, SETUP_MISE_NAME, mise_setup_step};
pub use steps::{
    ACQUIRE_NAME, ASSET_SHA_ENV, ASSET_URL_ENV, BASELINE_PUBLISH_UPLOAD_NAME,
    CRATE_REPORT_UPLOAD_NAME, CompileDriver, DENY_STEP_NAME, FORBIDDEN_TOKENS, INTERNAL_OP_ENV,
    MACHETE_STEP_NAME, MATRIX_REPORT_UPLOAD_NAME, MBX_PREFLIGHT_NAME, MBX_VERSION_CHECK_NAME,
    MERGE_OPERATION, PLAN_OPERATION, PUBLISH_OPERATION, PUBLISH_STEP_ID, RELEASE_COMMIT_ENV,
    REQUEST_DIR_PREFIX, REQUEST_FILE_ENV, RUN_KEY_EXPR, STAGED_BINARY_PREFIX,
    WRITE_PRESEED_MANIFEST_OPERATION, WRITE_REQUEST_OPERATION, acquire_velnor_step, action_step,
    action_step_with_env, ambient_shell_step, baseline_publish_upload_step, check_cache_step_order,
    check_mbx_gating, checkout_step, crate_job_report_upload_step, internal_step,
    lane_cargo_target_env, matrix_report_upload_step, mbx_steps_for_driver, merge_step, plan_step,
    publish_step, scan_for_private_subcommands, shell_step, validate_uses, write_request_step,
};
pub use tree::{render_tree, render_tree_with_extra};
pub use verification_jobs::{
    INSTALL_VERIFICATION_TOOLS_NAME, RUN_VERIFICATION_TASK_NAME, VerificationTaskPolicy,
    build_verification_task_job,
};
pub use workflow_size::MAX_WORKFLOW_BYTES;
pub use yaml::{Yaml, quote_scalar, render_yaml};

pub use error::RenderError;

/// Renderer implementation version (typed Gate-2 renderer).
pub const RENDERER_VERSION: u32 = 2;
