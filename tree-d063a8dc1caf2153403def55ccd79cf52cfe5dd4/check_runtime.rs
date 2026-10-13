//! Qualified execution of one named check, through the existing plan/report gate.
use std::env;
use std::path::Path;

use velnor_actions_contract_workflow::{NAMED_CHECK_JOB_ID_ENV, NAMED_CHECK_LANE_VARIANT_ENV};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_core::internal;
use velnor_actions_orchestrator_runtime_execute::execute::{CHECK_ID_ENV, execute_check_to};

/// Internal execution operation for a statically authorized native Mise task.
pub const EXECUTE_CHECK_OP: &str = "execute-check-v1";

/// Execute the declared check and publish its ordinary task/matrix reports.
/// # Errors
/// Missing source binding, capabilities, task failures, and invalid evidence fail closed.
pub fn execute_check() -> Result<usize, OrchestratorError> {
    let root = env::var_os("GITHUB_WORKSPACE")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("missing_check_workspace"))?;
    let temp = env::var_os("RUNNER_TEMP")
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let id = required_env(CHECK_ID_ENV)?;
    let task_id = required_env(velnor_actions_orchestrator_core::report_keys::TASK_ID_ENV)?;
    let job_id = required_env(NAMED_CHECK_JOB_ID_ENV)?;
    let lane = required_env(NAMED_CHECK_LANE_VARIANT_ENV)?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    execute_check_to(
        Path::new(&root),
        Path::new(&temp),
        &run_key,
        &id,
        &task_id,
        &job_id,
        &lane,
        velnor_actions_orchestrator_retrieve::retrieve_reports::MAX_RETRIEVE_PLAN_BYTES,
    )
}
fn required_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| internal("missing_check_identity"))
}

#[cfg(test)]
mod tests;
