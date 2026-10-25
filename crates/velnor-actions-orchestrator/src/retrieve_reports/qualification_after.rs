//! Stage Required's authenticated post-fetch qualification-cache census.

use std::path::{Path, PathBuf};

use crate::OrchestratorError;
use crate::internal::internal;

pub(super) fn stage_after(
    run_id: u64,
    run_key: &str,
    run_dir: &Path,
) -> Result<(), OrchestratorError> {
    let runner_temp = run_dir
        .ancestors()
        .nth(2)
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let Ok(plan) = crate::task_report::load_plan(run_key, runner_temp) else {
        return Ok(());
    };
    let Some(context) = plan.qualification.as_ref() else {
        return Ok(());
    };
    if context.run_id != run_id {
        return Err(internal("qualification_cache_after_run_mismatch"));
    }
    let workspace = std::env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| internal("missing_github_workspace"))?;
    let snapshot =
        crate::qualification_cache_backend::capture_qualification_cache_after(&workspace, context)?;
    crate::qualification_cache_backend::stage_qualification_cache_after(run_dir, &snapshot)
}
