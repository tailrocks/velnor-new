//! Private output step for a declared verification task.

use std::env;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator_internal::artifact_export::materialize_verification_artifact_from_environment;

/// Stage verified outputs and publish only their plan-derived artifact name.
pub(crate) fn run_verification_artifact_internal(velnor_dir: &Path) -> ExitCode {
    let materialized = match materialize_verification_artifact_from_environment(velnor_dir) {
        Ok(value) => value,
        Err(error) => return fail(&error.to_string()),
    };
    let Some(output_path) = env::var_os("GITHUB_OUTPUT").filter(|value| !value.is_empty()) else {
        return fail("missing github output");
    };
    let body = format!("artifact_name={}\n", materialized.result.artifact_name);
    match OpenOptions::new()
        .append(true)
        .create(true)
        .open(output_path)
        .and_then(|mut file| file.write_all(body.as_bytes()))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => fail(&format!("append outputs: {error}")),
    }
}

fn fail(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}
