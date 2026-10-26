use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::steps::{INTERNAL_OP_ENV, STAGED_BINARY_PREFIX};

use crate::OrchestratorError;
use crate::task_report::{EXIT_CODE_ENV, REPORT_OP, START_MS_ENV};

pub(crate) fn helper_path_for_version(version: &str) -> String {
    format!("{STAGED_BINARY_PREFIX}{version}")
}

pub(crate) fn report_wrapper_argv(joined: &str, helper: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "s=$(date +%s%3N); {joined}; code=$?; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$s\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
        ),
    ]
}

pub(crate) fn outcome_wrapper_argv(
    joined: &str,
    outcome_path: &str,
    start_path: &str,
) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "date +%s%3N > \"{start_path}\"; {joined}; code=$?; echo \"$code\" > \"{outcome_path}\"; exit \"$code\""
        ),
    ]
}

pub(crate) fn deferred_report_argv(
    outcome_path: &str,
    helper: &str,
    start_path: &str,
) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "read -r code rest < \"{outcome_path}\"; read -r start_ms rest < \"{start_path}\"; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$start_ms\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\""
        ),
    ]
}

pub(crate) fn outcome_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/outcome-{matrix_key}")
}

pub(crate) fn start_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/start-{matrix_key}")
}

pub(crate) fn download_plan_step() -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::download_plan_step().map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

pub(crate) fn crate_upload_step(job_id: &str) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::crate_job_report_upload_step(job_id).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}
