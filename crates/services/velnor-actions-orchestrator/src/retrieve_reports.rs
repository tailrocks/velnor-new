//! Event-time `fetch-reports-v1` entrypoint over the extracted retrieve crate.
//!
//! Artifact download plus baseline fetch live in
//! `velnor-actions-orchestrator-retrieve` as a dependency-free leaf;
//! this module resolves the run key and directory from the runner
//! environment and keeps the original `retrieve_reports` entrypoint
//! byte-identical for the CLI and integration tests.

use std::path::Path;

use velnor_actions_orchestrator_core::{OrchestratorError, internal};

/// Retrieve operation tag (single-sourced from the renderer protocol).
pub use velnor_actions_workflow_steps::steps::FETCH_OPERATION as FETCH_OP;

/// Retrieve every plan-expected matrix artifact for this run.
///
/// Resolves the run ID from `GITHUB_RUN_ID` and the run directory from
/// `RUNNER_TEMP/velnor/<run-key>`, then delegates to the extracted
/// crate. Returns the count of artifacts downloaded.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing or non-numeric
/// run IDs and missing runner temp.
pub fn retrieve_reports() -> Result<usize, OrchestratorError> {
    let run_id = std::env::var("GITHUB_RUN_ID")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_run_id"))?
        .parse::<u64>()
        .map_err(|_| internal("bad_run_id"))?;
    let run_key = velnor_actions_orchestrator_core::run_key::resolve_run_key(None)?;
    let temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let run_dir = Path::new(&temp).join("velnor").join(&run_key);
    Ok(
        velnor_actions_orchestrator_retrieve::retrieve_reports::retrieve_reports_to(
            run_id, &run_dir,
        ),
    )
}
