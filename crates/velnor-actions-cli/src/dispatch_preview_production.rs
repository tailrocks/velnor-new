//! Closed private preview operation dispatch; admission executes no source.

use std::process::ExitCode;
use velnor_actions_orchestrator::{admit_preview_aggregate, execute_preview_producer};

pub(super) fn produce() -> ExitCode {
    result(execute_preview_producer())
}

pub(super) fn admit() -> ExitCode {
    result(admit_preview_aggregate())
}

fn result(value: Result<(), velnor_actions_orchestrator::OrchestratorError>) -> ExitCode {
    match value {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => super::fail_internal(&error.to_string()),
    }
}
