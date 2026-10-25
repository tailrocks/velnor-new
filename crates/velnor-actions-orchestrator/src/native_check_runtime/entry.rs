use std::{env, path::Path};

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::config::NativeHomebrewOperation;

use super::{OPERATION_ENV, execute_native_homebrew_check_to};

pub fn execute_native_homebrew_check_from_env() -> Result<usize, OrchestratorError> {
    let root = env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_native_check_workspace"))?;
    let temp = env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_runner_temp"))?;
    let check_id = required_env("VELNOR_CHECK_ID")?;
    let operation = parse_operation(&required_env(OPERATION_ENV)?)?;
    let task_id = required_env(crate::task_report::TASK_ID_ENV)?;
    let run_key = crate::internal_request::resolve_run_key(None)?;
    execute_native_homebrew_check_to(
        Path::new(&root),
        Path::new(&temp),
        &run_key,
        &check_id,
        operation,
        &task_id,
    )
}

fn required_env(key: &str) -> Result<String, OrchestratorError> {
    env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| internal("missing_native_check_identity"))
}

fn parse_operation(value: &str) -> Result<NativeHomebrewOperation, OrchestratorError> {
    match value {
        "tap_tree" => Ok(NativeHomebrewOperation::TapTree),
        "formula_contract" => Ok(NativeHomebrewOperation::FormulaContract),
        "install" => Ok(NativeHomebrewOperation::Install),
        "adoption_contract" => Ok(NativeHomebrewOperation::AdoptionContract),
        _ => Err(internal("native_check_operation_invalid")),
    }
}
