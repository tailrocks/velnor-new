//! Mise task-cache decisions, restore evidence, and reuse grants.

pub mod cache;
pub mod cache_sources;
pub mod cache_transport;
pub mod cache_trust;
pub mod gate6;
pub mod restore;
pub mod restore_evidence;
pub mod reuse;

pub use cache::{
    CACHE_DIR_ENV, CachedTaskDescriptor, TASK_ARTIFACTS_DIR_NAME, TASK_ARTIFACTS_VERSION,
    TASK_CACHE_DIR_ENV, TASK_CACHE_MODE_ENV, TaskCacheMode, artifact_path, cache_mode_from_env,
    read_artifact_bytes, resolve_task_artifact_dir, task_artifact_dir_from_env,
    verify_artifact_digest,
};
pub use gate6::{Gate6Fixture, qualified_task_run_argv, render_gated_task_toml};
pub use restore::{
    MissReason, RestoreCheck, RestoreEvidence, ReuseFallback, SaveInputs, ToolAvailability,
    fallback_for_error, save_decision, save_useful, verify_restored_task_result, writers_overlap,
};
pub use restore_evidence::{RestoreObservation, classify_restore, output_bytes_complete};
pub use reuse::{
    ReuseGrant, ReusePlan, ReuseQualification, ReuseSignal, TaskArtifactTransport, TaskCacheKey,
    TaskReuseRequest, plan_reuse,
};
