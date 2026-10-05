//! Fixed source-only workflow preview, independent of runtime qualification.

use std::path::PathBuf;
use std::process::ExitCode;

use velnor_actions_orchestrator::preview_foundation_qualification;

pub(super) fn run(destination: Option<PathBuf>) -> ExitCode {
    let Some(destination) = destination else {
        eprintln!("velnor-actions: Foundation qualification requires --output-dir");
        return ExitCode::from(2);
    };
    let Some(cwd) = super::working_dir() else {
        return ExitCode::from(1);
    };
    match preview_foundation_qualification(&cwd, &destination) {
        Ok(paths) => {
            eprintln!(
                "Foundation qualification source preview: {}",
                destination.display()
            );
            for path in paths {
                eprintln!("{path}");
            }
            eprintln!("Native qualification and runtime admission pending.");
            ExitCode::SUCCESS
        }
        Err(error) => super::fail_public(&error),
    }
}
