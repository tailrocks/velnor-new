//! Report command failures and working-directory errors.

use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::OrchestratorError;

/// Report a private failure without printing the private operation.
pub(crate) fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}

/// Return the working directory or report its error.
pub(crate) fn working_dir() -> Option<PathBuf> {
    match std::env::current_dir() {
        Ok(dir) => Some(dir),
        Err(error) => {
            eprintln!("velnor-actions: working directory: {error}");
            None
        }
    }
}

/// Report an orchestrator failure as exit 1.
pub(crate) fn fail_public(error: &OrchestratorError) -> ExitCode {
    eprintln!("velnor-actions: {}", single_line(&error.to_string()));
    ExitCode::from(1)
}

/// Keep each error on one line.
fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
