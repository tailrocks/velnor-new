//! Run-key and staged-report behavior consumed by merge-request assembly, behind a port trait.
//!
//! Assembly calls exactly these two hub functions. The hub implements
//! [`RequestPort`] by delegating to its `internal_request` and
//! `retrieve_reports` modules, so merge-request assembly depends only
//! on this contract and the direct `internal_request`/`merge_request`
//! cycle is broken.

use std::path::Path;

use velnor_actions_orchestrator_core::OrchestratorError;

/// Run-key resolution plus staged matrix/task report reads.
pub trait RequestPort {
    /// Explicit run key, else `r<run-id>-a<attempt>` from the GitHub environment.
    fn resolve_run_key(&self, explicit: Option<&str>) -> Result<String, OrchestratorError>;

    /// Read exactly the plan-expected staged reports plus task files.
    ///
    /// Each `matrix.include` entry names its job's artifact and its own
    /// matrix key; absent or unreadable files are recorded in `errors`,
    /// never skipped silently. Both lists sort by ID.
    fn read_staged_reports(
        &self,
        run_key: &str,
        plan: &serde_json::Value,
        dir: &Path,
        errors: &mut Vec<String>,
    ) -> (Vec<serde_json::Value>, Vec<serde_json::Value>);
}
