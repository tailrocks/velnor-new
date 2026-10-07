//! Private publish entrypoint: stage the baseline, emit its name.
//!
//! Split from `dispatch` so that module keeps the 400-line gate. Reads
//! the publish request, stages `baseline.json` through the
//! orchestrator op, and appends the derived `artifact_name` output the
//! upload step consumes.

use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use velnor_actions_orchestrator_baseline_publish::baseline_publish::baseline_publish;

/// Environment variable carrying the `$GITHUB_OUTPUT` path. Never printed.
const GITHUB_OUTPUT_ENV: &str = "GITHUB_OUTPUT";
/// Environment variable carrying the runner temp dir. Never printed.
const RUNNER_TEMP_ENV: &str = "RUNNER_TEMP";

/// Read the request, run the publish op, append the artifact name.
pub(crate) fn run_publish_internal(path: &Path) -> ExitCode {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => return fail_internal(&format!("read request: {error}")),
    };
    let Some(runner_temp) = env::var_os(RUNNER_TEMP_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing runner temp");
    };
    let outputs = match baseline_publish(&text, Path::new(&runner_temp)) {
        Ok(outputs) => outputs,
        Err(error) => return fail_internal(&error.to_string()),
    };
    let Some(output_path) = env::var_os(GITHUB_OUTPUT_ENV).filter(|value| !value.is_empty()) else {
        return fail_internal("missing github output");
    };
    let body = format!("artifact_name={}\n", outputs.artifact_name);
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

/// Report a private failure without printing the private operation.
fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}
