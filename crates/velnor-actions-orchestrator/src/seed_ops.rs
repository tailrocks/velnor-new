//! Data-only Cache seed operations run by the staged helper.
//!
//! - `resolve-seed-task-v1`: one lane record as an `env`-ready NUL frame;
//! - `seed-skeleton-v1`: stub workspace member sources in place;
//! - `seed-trim-v1`: trim lane profiles to dependency units, report, and
//!   publish the ceiling verdicts.
//!
//! The generated shell executes every process; these operations only read
//! and validate data and write files under the workspace and runner temp.

use std::path::PathBuf;

use crate::OrchestratorError;
use crate::internal::internal;

#[path = "seed_skeleton.rs"]
mod skeleton;
#[path = "seed_task.rs"]
mod task;
#[path = "seed_trim.rs"]
mod trim;

pub use skeleton::seed_skeleton;
pub use task::resolve_seed_task;
pub use trim::seed_trim;

/// Private op tag: resolve one seed lane record.
pub const RESOLVE_SEED_TASK_OP: &str = "resolve-seed-task-v1";
/// Private op tag: stub workspace member sources.
pub const SEED_SKELETON_OP: &str = "seed-skeleton-v1";
/// Private op tag: trim the dependency seed.
pub const SEED_TRIM_OP: &str = "seed-trim-v1";

/// Seed scratch directory below runner temp.
pub(crate) const SEED_DIR: &str = "velnor/seed";

/// One required, absolute path from the runner environment.
pub(crate) fn absolute_env(key: &str) -> Result<PathBuf, OrchestratorError> {
    let path = std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal(&format!("missing_{}", key.to_ascii_lowercase())))?;
    if !path.is_absolute() {
        return Err(internal("runner_paths_must_be_absolute"));
    }
    Ok(path)
}

/// `VELNOR_SEED_ROOT` (`.` is the repository root), validated.
pub(crate) fn seed_root_env() -> Result<String, OrchestratorError> {
    let raw = std::env::var("VELNOR_SEED_ROOT").map_err(|_| internal("missing_seed_root"))?;
    let root = if raw == "." { String::new() } else { raw };
    velnor_actions_workflow_renderer::steps::validate_seed_root(&root)
        .map_err(|_| internal("bad_seed_root"))?;
    Ok(root)
}
