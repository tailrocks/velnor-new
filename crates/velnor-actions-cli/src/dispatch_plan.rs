//! Early admission and final publication share the normal planner response.

use super::{GITHUB_OUTPUT_ENV, RUNNER_TEMP_ENV, fail_internal};
use std::path::Path;
use std::process::ExitCode;
use std::{env, fs};
use velnor_actions_orchestrator::{
    AnalysisPublicationOutputs, COVERED_TASKS_OUTPUT, EarlyPlanResult, OrchestratorError,
    PLAN_CARGO_FALLBACK_OUTPUT, plan_early_internal, plan_internal_with_analysis, plan_outputs,
    publish_plan_files, read_early_response, response_path_for,
};

/// Read the request, run the planner, publish files, write response plus outputs.
pub(super) fn run_plan_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    let (response, analysis) = match plan_response(path, &text, Path::new(&runner_temp)) {
        Ok(result) => result,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let sibling = match response_path_for(path) {
        Ok(sibling) => sibling,
        Err(error) => return fail_internal(&error.to_string()),
    };
    if env::var("VELNOR_EARLY_NEEDS_CARGO").as_deref() != Ok("false")
        && let Err(error) = fs::write(&sibling, &response)
    {
        return fail_internal(&format!("write response: {error}"));
    }
    if let Err(error) = publish_plan_files(&response, &Path::new(&runner_temp).join("velnor")) {
        return fail_internal(&error.to_string());
    }
    let outputs = match plan_outputs(&response) {
        Ok(outputs) => outputs,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    let fallback = env::var("VELNOR_EARLY_NEEDS_CARGO").as_deref() == Ok("true");
    let mut body = format!(
        "matrix={}\nplan={}\n{PLAN_CARGO_FALLBACK_OUTPUT}={fallback}\n",
        outputs.matrix, outputs.plan,
    );
    body.push_str(&format!(
        "analysis_artifact_name={}\nanalysis_artifact_path={}\n",
        analysis
            .as_ref()
            .map_or("", |value| value.artifact_name.as_str()),
        analysis.as_ref().map_or(String::new(), |value| value
            .artifact_path
            .display()
            .to_string()),
    ));
    // Always emitted, even when empty: the skip gate reads this output
    // through GitHub's evaluator, so an explicitly empty value keeps
    // the contract independent of unset-output semantics.
    body.push_str(COVERED_TASKS_OUTPUT);
    body.push('=');
    body.push_str(&outputs.covered_tasks);
    body.push('\n');
    match fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(&output_path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(body.as_bytes())
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("append outputs: {error}")),
    }
}

fn plan_response(
    path: &Path,
    text: &str,
    runner_temp: &Path,
) -> Result<(String, Option<AnalysisPublicationOutputs>), OrchestratorError> {
    plan_response_with_mode(
        path,
        text,
        runner_temp,
        env::var("VELNOR_EARLY_NEEDS_CARGO").ok().as_deref(),
    )
}

fn plan_response_with_mode(
    path: &Path,
    text: &str,
    runner_temp: &Path,
    mode: Option<&str>,
) -> Result<(String, Option<AnalysisPublicationOutputs>), OrchestratorError> {
    match mode {
        Some("false") => {
            let sibling = response_path_for(path)?;
            Ok((read_early_response(text, &sibling)?, None))
        }
        Some("true") | None => plan_internal_with_analysis(text, runner_temp),
        Some(_) => Err(OrchestratorError::Internal {
            problem: "early_output_invalid".to_owned(),
        }),
    }
}

pub(super) fn run_early_plan(path: &Path) -> ExitCode {
    let result = fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|text| plan_early_internal(&text).map_err(|error| error.to_string()));
    let needs_cargo = match result {
        Ok(EarlyPlanResult::Ready { response }) => {
            let sibling = match response_path_for(path) {
                Ok(sibling) => sibling,
                Err(error) => return fail_internal(&error.to_string()),
            };
            if let Err(error) = stage_early_response(&sibling, &response) {
                return fail_internal(&format!("write early response:{error}"));
            }
            false
        }
        Ok(EarlyPlanResult::NeedsCargo { reason }) => {
            eprintln!(
                "velnor-actions: Cargo required: {}",
                super::single_line(&reason)
            );
            true
        }
        Err(error) => return fail_internal(&error),
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    let body = format!("needs_cargo={needs_cargo}\n");
    match fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(output_path)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(body.as_bytes())
        }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail_internal(&format!("write early outputs:{error}")),
    }
}

fn stage_early_response(path: &Path, response: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(response.as_bytes())
}

#[cfg(test)]
#[path = "dispatch_plan_tests.rs"]
mod tests;
