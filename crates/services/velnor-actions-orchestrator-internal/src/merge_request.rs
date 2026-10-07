//! Merge-request assembly over the extracted merge-request crate.
//!
//! Assembly lives in `velnor-actions-orchestrator-merge-request` and
//! resolves the run key only through [`RequestPort`]; this module
//! implements the port over the hub's `internal_request` module and
//! keeps the original entrypoints byte-identical for the CLI and
//! integration tests.

use std::path::{Path, PathBuf};

use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_merge_request_ports::RequestPort;

/// Hub run-key resolution behind the merge-request port.
struct HubRequest;

impl RequestPort for HubRequest {
    fn resolve_run_key(&self, explicit: Option<&str>) -> Result<String, OrchestratorError> {
        velnor_actions_orchestrator_core::run_key::resolve_run_key(explicit)
    }
}

/// Assemble one canonical merge request from a run directory.
///
/// Reads `plan.json`, `matrix.json`, exactly the plan-expected
/// `reports/<artifact-id>/matrix-report.json` files plus their
/// `tasks/<task-report-id>.json` files (sorted by ID for determinism;
/// anything else under `reports/` is ignored, never globbed), plus
/// optional `baseline.json`.
/// Validator inventory and conclusions come from `VELNOR_NEEDS_JSON`. Every
/// missing or unparsable input is recorded in `assembly_errors`, never
/// dropped, so the merge judges the gap explicitly.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for encoding failures.
pub fn assemble_merge_request(run_key: &str, run_dir: &Path) -> Result<String, OrchestratorError> {
    velnor_actions_orchestrator_merge_request::assemble_merge_request(run_key, run_dir)
}

/// Materialize the merge request from the environment and run directory.
///
/// Resolves the run key from `GITHUB_RUN_ID`/`GITHUB_RUN_ATTEMPT` and the
/// run directory from `RUNNER_TEMP`, then delegates to the extracted
/// crate's exclusive file writer.
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env or unwritable
/// paths; [`OrchestratorError::Io`] for unreadable artifact JSON.
pub(crate) fn write_merge_request(request_path: &Path) -> Result<PathBuf, OrchestratorError> {
    velnor_actions_orchestrator_merge_request::write_merge_request(&HubRequest, request_path)
}
