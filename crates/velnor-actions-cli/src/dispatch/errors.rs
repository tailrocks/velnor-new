//! Shared CLI diagnostics and working-directory resolution.

use std::{env, path::PathBuf, process::ExitCode};
use velnor_actions_orchestrator::OrchestratorError;

/// Report a private failure without printing the private operation.
pub(crate) fn fail_internal(problem: &str) -> ExitCode {
    eprintln!("velnor-actions: internal request failed: {problem}");
    ExitCode::from(1)
}

/// Read the working directory, reporting failures as exit 1.
pub(crate) fn working_dir() -> Option<PathBuf> {
    match env::current_dir() {
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

/// Collapse error values to one log line so untrusted input cannot inject
/// additional stderr records.
fn single_line(text: &str) -> String {
    text.replace(['\n', '\r'], " ")
}
