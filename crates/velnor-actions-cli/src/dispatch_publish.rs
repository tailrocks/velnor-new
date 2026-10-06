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

use velnor_actions_orchestrator::{PublishOutputs, baseline_publish};

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
    let body = output_body(&outputs);
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

/// GitHub output protocol: retry status and the original evidence attempt.
fn output_body(outputs: &PublishOutputs) -> String {
    format!(
        "artifact_name={}\nupload_needed={}\nbaseline_run_attempt={}\n",
        outputs.artifact_name, outputs.upload_needed, outputs.baseline_run_attempt
    )
}

/// Report a private failure without printing the private operation.
fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_output_preserves_original_attempt_and_disables_upload() {
        let outputs = PublishOutputs {
            artifact_name: "velnor-baseline-existing".to_owned(),
            upload_needed: false,
            baseline_run_attempt: 1,
        };
        assert_eq!(
            output_body(&outputs),
            "artifact_name=velnor-baseline-existing\nupload_needed=false\nbaseline_run_attempt=1\n"
        );
        assert!(
            output_body(&PublishOutputs {
                upload_needed: true,
                ..outputs
            })
            .contains("\nupload_needed=true\n")
        );
    }
}
