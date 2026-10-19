//! Generate the job image candidate workflow through the CLI.

use std::{path::PathBuf, process::ExitCode};

use velnor_actions_orchestrator::{parse_dispatch_mode, preview_job_image_candidate};

pub(crate) fn run(destination: Option<PathBuf>, mode: Option<&str>) -> ExitCode {
    let Some(destination) = destination else {
        eprintln!("velnor-actions: job image candidates require --output-dir");
        return ExitCode::from(2);
    };
    let Some(cwd) = crate::dispatch::working_dir() else {
        return ExitCode::from(1);
    };
    let mode = match mode.map(parse_dispatch_mode).transpose() {
        Ok(mode) => mode,
        Err(error) => return crate::dispatch::fail_public(&error),
    };
    match preview_job_image_candidate(&cwd, &destination, mode) {
        Ok(paths) => {
            for path in paths {
                println!("{path}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => crate::dispatch::fail_public(&error),
    }
}
