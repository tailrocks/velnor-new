//! Event-time typed task-report production over the report-write core.
//!
//! The `write-task-report-v1` core lives in the `report_write` module as
//! a dependency-free leaf; this module resolves the run key and keeps
//! the original `write_task_report` entrypoint byte-identical for the CLI
//! and integration tests.

use velnor_actions_orchestrator_core::OrchestratorError;

use super::report_write::write_task_report_with_key;

/// Write the executed obligation's reports plus downstream skip reports.
///
/// Resolves the run key from the GitHub environment and the plan from the
/// downloaded plan artifact; returns the count of tasks reported (one plus
/// downstream skips on failure).
///
/// # Errors
///
/// Returns [`OrchestratorError::Internal`] for missing env, unreadable or
/// invalid plans, run-key mismatches, unknown or multi-task entries, and
/// unwritable report paths; [`OrchestratorError::Io`] for IO failures.
pub fn write_task_report() -> Result<usize, OrchestratorError> {
    let run_key = velnor_actions_orchestrator_core::run_key::resolve_run_key(None)?;
    write_task_report_with_key(&run_key)
}

#[cfg(test)]
mod tests;
