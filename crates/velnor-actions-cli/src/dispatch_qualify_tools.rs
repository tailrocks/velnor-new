use crate::dispatch::fail_public;
use std::path::Path;
use std::process::ExitCode;

pub(crate) fn run(request: &Path, work: &Path, output: &Path) -> ExitCode {
    match velnor_actions_orchestrator::qualify_tools(request, work, output) {
        Ok(()) => {
            println!(
                "Qualified-tool authoring candidate written; independent CI verification required."
            );
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}
