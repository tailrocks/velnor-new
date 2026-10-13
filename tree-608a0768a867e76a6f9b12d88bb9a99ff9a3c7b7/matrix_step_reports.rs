//! Report and outcome shell wrappers used by matrix jobs.

use velnor_actions_contract::Step;
use velnor_actions_workflow_renderer::steps::{INTERNAL_OP_ENV, STAGED_BINARY_PREFIX};

use crate::OrchestratorError;
use crate::task_report::{EXIT_CODE_ENV, REPORT_OP, START_MS_ENV};

/// Staged helper path for this generator version (uniform preseed/consumer).
pub(crate) fn helper_path_for_version() -> String {
    format!("{STAGED_BINARY_PREFIX}{}", env!("CARGO_PKG_VERSION"))
}

/// `sh -c` argv wrapping one joined command with report capture.
///
/// Captures the wall-clock start with GNU `date`'s millisecond format,
/// runs the obligation, captures `$?`, reports through the staged
/// helper's [`REPORT_OP`], then exits with the obligation code (helper
/// failure surfaces only on an otherwise passing obligation, so failures
/// never mask each other). This wrapper is used only by generic jobs,
/// whose workflow runner labels are Ubuntu; named checks invoke the helper
/// directly and measure duration with Rust `Instant` on Linux or macOS.
/// Credential removal is the step constructor's job (`shell_step`
/// prefixes argv-wide `env -u`), not a script prelude's: obligations
/// execute repository code (build scripts), and the step env cannot
/// shadow runner-injected credentials (D3).
pub(crate) fn report_wrapper_argv(joined: &str, helper: &str) -> Vec<String> {
    vec![
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "s=$(date +%s%3N); {joined}; code=$?; {EXIT_CODE_ENV}=\"$code\" {START_MS_ENV}=\"$s\" {INTERNAL_OP_ENV}={REPORT_OP} \"{helper}\"; helper_code=$?; if [ \"$code\" -ne 0 ]; then exit \"$code\"; fi; exit \"$helper_code\""
        ),
    ]
}

/// `sh -c` argv saving one joined command's exit to an outcome file.
///
/// Two-phase shape for the plan-job workspace Format: the plan does
/// not exist yet at format time, so the wrapper records `$?` plus the
/// wall-clock start stamp, and a post-plan step reports through
/// [`deferred_report_argv`].
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

/// `sh -c` argv reporting one saved outcome through the staged helper.
///
/// Reads the exit code and start stamp the outcome wrapper saved (a
/// missing file leaves the value empty and the helper fails closed),
/// then invokes [`REPORT_OP`]; the step exits with the helper's code.
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

/// Shell-spelled outcome file for one matrix key under runner temp.
pub(crate) fn outcome_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/outcome-{matrix_key}")
}

/// Shell-spelled start-stamp file for one matrix key under runner temp.
pub(crate) fn start_path_for_key(matrix_key: &str) -> String {
    format!("$RUNNER_TEMP/velnor/start-{matrix_key}")
}

/// Plan-artifact download: report wrappers resolve identities from it.
pub(crate) fn download_plan_step() -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::download_plan_step().map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}

/// One always-on crate-report upload carrying a job's every entry.
pub(crate) fn crate_upload_step(job_id: &str) -> Result<Step, OrchestratorError> {
    velnor_actions_workflow_renderer::crate_job_report_upload_step(job_id).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })
}
