//! Private plan and merge file-protocol handlers.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    DYNAMIC_MATRIX_OUTPUT_MODE, PLAN_MATRIX_OUTPUT_MODE_ENV, PlanOutputMode, merge_internal,
    merge_passed, plan_internal, plan_outputs_from_staged_admission, publish_final_report,
    publish_plan_files, response_path_for, validate_plan_response,
};

use crate::dispatch;

const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

/// Read the request, run the planner, then publish its validated outputs.
pub(crate) fn run_plan(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return dispatch::fail_internal(&format!("read request: {error}")),
    };
    let mode = match env::var_os(PLAN_MATRIX_OUTPUT_MODE_ENV) {
        None => PlanOutputMode::Static,
        Some(value) if value == DYNAMIC_MATRIX_OUTPUT_MODE => PlanOutputMode::DynamicMatrix,
        Some(_) => return dispatch::fail_internal("bad_plan_matrix_output_mode"),
    };
    let response = match plan_internal(&text) {
        Ok(response) => response,
        Err(error) => return dispatch::fail_internal(&error.to_string()),
    };
    if let Err(error) = validate_plan_response(&response) {
        return dispatch::fail_internal(&error.to_string());
    }
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return dispatch::fail_internal(&error.to_string()),
    };
    if let Err(error) = fs::write(&sibling, &response) {
        return dispatch::fail_internal(&format!("write response: {error}"));
    }
    publish_plan(&response, mode)
}

fn publish_plan(response: &str, mode: PlanOutputMode) -> ExitCode {
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return dispatch::fail_internal("missing runner temp");
    };
    let outputs = match plan_outputs_from_staged_admission(response, mode, Path::new(&runner_temp))
    {
        Ok(outputs) => outputs,
        Err(error) => return dispatch::fail_internal(&error.to_string()),
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return dispatch::fail_internal("missing github output");
    };
    if let Err(error) = publish_plan_files(response, &Path::new(&runner_temp).join("velnor")) {
        return dispatch::fail_internal(&error.to_string());
    }
    let mut body = String::new();
    for (name, value) in outputs.step_outputs() {
        body.push_str(name);
        body.push('=');
        body.push_str(value);
        body.push('\n');
    }
    match fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&output_path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(body.as_bytes())
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => dispatch::fail_internal(&format!("append outputs: {error}")),
    }
}

/// Read the request, run the merge, publish the verdict, and return it.
pub(crate) fn run_merge(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return dispatch::fail_internal(&format!("read request: {error}")),
    };
    let response = match merge_internal(&text) {
        Ok(response) => response,
        Err(error) => return dispatch::fail_internal(&error.to_string()),
    };
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return dispatch::fail_internal(&error.to_string()),
    };
    if let Err(error) = fs::write(&sibling, &response) {
        return dispatch::fail_internal(&format!("write response: {error}"));
    }
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return dispatch::fail_internal("missing runner temp");
    };
    if let Err(error) = publish_final_report(&response, &Path::new(&runner_temp).join("velnor")) {
        return dispatch::fail_internal(&error.to_string());
    }
    match merge_passed(&response) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(error) => dispatch::fail_internal(&error.to_string()),
    }
}
