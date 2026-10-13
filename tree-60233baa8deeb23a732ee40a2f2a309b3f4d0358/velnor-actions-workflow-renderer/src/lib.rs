//! Typed-IR to YAML rendering for CI, release, and freshness workflows.
//!
//! Validated IR plus fixed argv in, marked YAML out: no subprocesses, no
//! stack or tool branching, quoting-only shell shaping.

mod candidate;
pub mod closure;
mod closure_paths;
mod composite;
mod document;
mod document_lanes;
mod document_steps;
mod final_steps;
pub mod freshness;
mod lane_share;
mod lane_share_sections;
pub mod lane_target;
mod matrix;
mod matrix_output_mode;
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
mod support;
pub mod tree;
mod verification_jobs;
mod workflow_policy;

pub use candidate::{
    VERIFY_MANIFEST_NAME, candidate_artifact_name, candidate_manifest_verify_script,
    candidate_manifest_verify_step, check_release_build,
};
pub use closure::{
    CHECK_GENERATED_NAME, DOWNLOAD_PLAN_NAME, FRESHNESS_OUTDIR, HelperProvenance,
    PLAN_ARTIFACT_NAME, PLAN_ARTIFACT_PATH, PUBLISH_PLAN_NAME, SEED_REMEDIATION,
    download_plan_step, freshness_step, provision_acquire_step, publish_plan_step,
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
    AGENTS_MD_PATH, CANDIDATE_JOB_ID, CLAUDE_MD_PATH, CLAUDE_MD_TARGET, CONCURRENCY_CANCEL,
    CONCURRENCY_GROUP, COVERED_TASKS_OUTPUT, CandidateSpec, EXPECTED_PR_TYPES, FINAL_CONDITION,
    FINAL_DISPLAY_NAME, FINAL_JOB_ID, MATRIX_MAX_PARALLEL_ENV, MATRIX_NEEDS_JOB_ENV,
    MATRIX_OUTPUT_ENV, MatrixSource, PLAN_ID_OUTPUT, PLAN_JOB_ID, PLAN_STEP_ID, PUBLISH_JOB_ID,
    RUN_KEY_OUTPUT, RenderContext, TASK_JOB_ID, ValidatorCommand, WORKFLOW_PATH, action_pins,
    finalize_jobs, render_workflow_ir, render_workflow_ir_strict,
};
pub use schema2::{
    GeneratorReleasePins, MbxQualificationPins, Schema2WorkflowRequest, render_schema2_workflows,
};
pub use tree::{render_tree, render_tree_with_extra};
pub use verification_jobs::{
    INSTALL_VERIFICATION_TOOLS_NAME, RUN_VERIFICATION_TASK_NAME, VerificationTaskPolicy,
    build_verification_task_job,
};

/// Renderer implementation version (typed Gate-2 renderer).
pub const RENDERER_VERSION: u32 = 2;
