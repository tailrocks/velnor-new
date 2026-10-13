//! Generator-only source infrastructure preview, never a tool runner/publisher.

use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::{SourceQualificationTrigger, preview_owned_tool_candidates};

/// Dispatch a `Generate` command already known to request the owned preview.
///
/// `run_public` routes here only when `owned_tool_candidates_only` is set,
/// so any other variant is unreachable; it exits 2 like a usage error.
pub(super) fn run_owned_command(command: crate::args::Command) -> ExitCode {
    let crate::args::Command::Generate {
        output_dir,
        owned_tool_candidates_push,
        ..
    } = command
    else {
        return ExitCode::from(2);
    };
    run(output_dir, owned_tool_candidates_push)
}

pub(super) fn run(destination: Option<PathBuf>, reviewed_push: bool) -> ExitCode {
    let Some(destination) = destination else {
        eprintln!("velnor-actions: owned tool candidates require --output-dir");
        return ExitCode::from(2);
    };
    let Some(cwd) = super::working_dir() else {
        return ExitCode::from(1);
    };
    let trigger = if reviewed_push {
        SourceQualificationTrigger::ReviewedInfrastructurePush
    } else {
        SourceQualificationTrigger::DefaultBranchDispatch
    };
    match preview_owned_tool_candidates(&cwd, &destination, trigger) {
        Ok(paths) => {
            eprintln!("Source candidate preview: {}", destination.display());
            for path in paths {
                eprintln!("{path}");
            }
            eprintln!("Hosted qualification and publication pending.");
            ExitCode::SUCCESS
        }
        Err(error) => super::fail_public(&error),
    }
}
