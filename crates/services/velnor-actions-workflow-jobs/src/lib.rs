//! Job builders: policy merge, closures, fan-in, and render parameters.
//!
//! Pure and total: job construction contains no `std::fs`, `std::net`, or
//! process calls. Callers supply all discovered inputs.

#![forbid(unsafe_code)]

pub mod action_pins;
pub mod candidate;
pub mod closure;
pub mod closure_paths;
pub mod context;
pub mod final_steps;
pub mod finalize;
pub mod freshness;
pub mod msrv;
pub mod overlap;
pub mod plan_format;
pub mod preseed;
pub mod preseed_closure;
pub mod support;
pub mod verification_jobs;
pub mod workflow_policy;

pub use action_pins::action_pins;
pub use candidate::{
    VERIFY_MANIFEST_NAME, candidate_artifact_name, candidate_manifest_verify_script,
    candidate_manifest_verify_step, check_release_build,
};
pub use closure::{
    CHECK_GENERATED_NAME, DOWNLOAD_PLAN_NAME, FRESHNESS_OUTDIR, HelperProvenance,
    PLAN_ARTIFACT_NAME, PLAN_ARTIFACT_PATH, PUBLISH_PLAN_NAME, SEED_REMEDIATION,
    download_plan_step, freshness_step, provision_acquire_step, publish_plan_step,
};
pub use context::{
    CANDIDATE_JOB_ID, CONCURRENCY_CANCEL, CONCURRENCY_GROUP, CandidateSpec, EXPECTED_PR_TYPES,
    FINAL_CONDITION, FINAL_DISPLAY_NAME, FINAL_JOB_ID, PLAN_JOB_ID, PUBLISH_JOB_ID, RenderContext,
    TASK_JOB_ID, ValidatorCommand,
};
pub use finalize::{finalize_jobs, merged_jobs};
pub use preseed::{
    PRESEED_ARTIFACT_NAME, PRESEED_BUILD_NAME, PRESEED_BUILD_OUTPUT, PRESEED_DOWNLOAD_NAME,
    PRESEED_DOWNLOADED_BINARY, PRESEED_MANIFEST_BINARY_ENV, PRESEED_MANIFEST_FILE,
    PRESEED_MANIFEST_NAME, PRESEED_MANIFEST_OUT_ENV, PRESEED_MANIFEST_TARGET_ENV,
    PRESEED_MANIFEST_TOOLCHAIN_ENV, PRESEED_STAGE_DIR, PRESEED_STAGE_NAME, PRESEED_UPLOAD_NAME,
    PRESEED_VERIFY_MANIFEST_NAME, PRESEED_VERIFY_NAME, PreseedStageSource, preseed_build_step,
    preseed_download_step, preseed_manifest_step, preseed_manifest_verify_script,
    preseed_manifest_verify_step, preseed_stage_step, preseed_upload_step, preseed_verify_step,
};
pub use verification_jobs::{
    INSTALL_VERIFICATION_TOOLS_NAME, RUN_VERIFICATION_TASK_NAME, VerificationTaskPolicy,
    build_verification_task_job,
};
