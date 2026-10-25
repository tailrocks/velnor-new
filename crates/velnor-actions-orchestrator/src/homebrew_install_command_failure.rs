use std::path::Path;

use velnor_actions_mise::{CheckDeadline, ProcessOutput};

use crate::{
    OrchestratorError,
    homebrew::command::{HomebrewCommand, homebrew_command},
    homebrew::install::CAPTURE_LIMIT,
    internal::internal,
};

#[derive(Clone, Copy)]
pub(super) enum CommandFailurePhase {
    ConstructionRejected,
    ExecutionFailed,
    NonzeroExit,
}

pub(super) fn command_failure(
    operation: &'static str,
    phase: CommandFailurePhase,
    exit_code: Option<i32>,
) -> OrchestratorError {
    let phase = match phase {
        CommandFailurePhase::ConstructionRejected => "construction-rejected",
        CommandFailurePhase::ExecutionFailed => "execution-failed",
        CommandFailurePhase::NonzeroExit => "nonzero-exit",
    };
    let exit = exit_code.map_or_else(String::new, |code| format!(" exit={code}"));
    internal(&format!(
        "homebrew_install_command_failed operation={operation} phase={phase}{exit}"
    ))
}

pub(in crate::homebrew::install) fn run_command(
    operation: HomebrewCommand,
    home: &Path,
    temp: &Path,
    deadline: CheckDeadline,
) -> Result<ProcessOutput, OrchestratorError> {
    let operation_label = operation.diagnostic_label();
    let output = homebrew_command(operation, home, temp)
        .map_err(|_| {
            command_failure(
                operation_label,
                CommandFailurePhase::ConstructionRejected,
                None,
            )
        })?
        .run_quiescent_until(CAPTURE_LIMIT, deadline)
        .map_err(|_| {
            command_failure(operation_label, CommandFailurePhase::ExecutionFailed, None)
        })?;
    let (output, _drained) = output;
    if !output.success {
        return Err(command_failure(
            operation_label,
            CommandFailurePhase::NonzeroExit,
            output.code,
        ));
    }
    Ok(output)
}

#[cfg(test)]
#[path = "homebrew_install_command_failure_tests.rs"]
mod tests;
