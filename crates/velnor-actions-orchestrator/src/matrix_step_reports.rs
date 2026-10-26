// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::OrchestratorError;
use velnor_actions_contract::Step;

pub(crate) fn helper_path_for_version(version: &str) -> String {
    velnor_actions_workflow_renderer::plan_report::helper_path_for_version(version)
}

pub(crate) fn report_wrapper_argv(joined: &str, helper: &str) -> Vec<String> {
    velnor_actions_workflow_renderer::plan_report::report_wrapper_argv(joined, helper)
}

pub(crate) fn deferred_report_argv(
    outcome_path: &str,
    helper: &str,
    start_path: &str,
) -> Vec<String> {
    velnor_actions_workflow_renderer::plan_report::deferred_report_argv(
        outcome_path,
        helper,
        start_path,
    )
}

pub(crate) fn outcome_path_for_key(matrix_key: &str) -> String {
    velnor_actions_workflow_renderer::plan_report::outcome_path_for_key(matrix_key)
}

pub(crate) fn start_path_for_key(matrix_key: &str) -> String {
    velnor_actions_workflow_renderer::plan_report::start_path_for_key(matrix_key)
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
