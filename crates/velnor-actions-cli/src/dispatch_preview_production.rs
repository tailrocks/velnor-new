//! Closed private preview operation dispatch; admission executes no source.

use std::process::ExitCode;
use velnor_actions_orchestrator::{admit_release_aggregate, execute_release_producer};

pub(super) fn produce() -> ExitCode {
    native(execute_release_producer())
}

pub(super) fn admit() -> ExitCode {
    native(admit_release_aggregate())
}

pub(super) fn native(
    value: Result<(), velnor_actions_orchestrator::OrchestratorError>,
) -> ExitCode {
    match value {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}

/// Append only the canonical native digest; no credential is handed to source.
pub(super) fn acquire() -> ExitCode {
    use std::io::Write;
    let digest = match velnor_actions_orchestrator::acquire_release_leaves() {
        Ok(digest) => digest,
        Err(error) => return super::fail_internal(&error.to_string()),
    };
    let Some(path) = std::env::var_os("GITHUB_OUTPUT") else {
        return super::fail_internal("missing github output");
    };
    match std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .and_then(|mut file| writeln!(file, "admission_digest={digest}"))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}
