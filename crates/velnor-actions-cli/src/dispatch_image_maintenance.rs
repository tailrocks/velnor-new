//! Thin CLI adapter for closed source-bound image-maintenance operations.

use crate::args::ImageMaintenanceCommand;
use std::{process::ExitCode, time::Duration};
use velnor_actions_orchestrator::{
    BuildJobCandidateRequest, CheckDeadline, CheckJobSourceRequest, InstallBuildxRequest,
    ValidateJobCandidateRequest, build_job_candidate, check_job_source, install_buildx,
    validate_job_candidate,
};

pub(crate) fn run(command: &ImageMaintenanceCommand) -> ExitCode {
    let deadline = match CheckDeadline::after(Duration::from_mins(10)) {
        Ok(deadline) => deadline,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(1);
        }
    };
    let result = match command {
        ImageMaintenanceCommand::InstallBuildx { root, runner_temp } => {
            install_buildx(&InstallBuildxRequest {
                root: root.clone(),
                runner_temp: runner_temp.clone(),
                deadline,
            })
        }
        ImageMaintenanceCommand::CheckJobSource {
            root,
            source_commit,
            source_closure_path,
        } => check_job_source(&CheckJobSourceRequest {
            root: root.clone(),
            source_commit: source_commit.clone(),
            source_closure_path: source_closure_path.clone(),
            deadline,
        }),
        ImageMaintenanceCommand::BuildJobCandidate {
            root,
            source_commit,
            runner_temp,
            archive_path,
            record_path,
            source_closure_path,
        } => build_job_candidate(&BuildJobCandidateRequest {
            root: root.clone(),
            source_commit: source_commit.clone(),
            runner_temp: runner_temp.clone(),
            archive_path: archive_path.clone(),
            record_path: record_path.clone(),
            source_closure_path: source_closure_path.clone(),
            deadline,
        }),
        ImageMaintenanceCommand::ValidateJobCandidate {
            root,
            archive_path,
            record_path,
            source_closure_path,
        } => validate_job_candidate(&ValidateJobCandidateRequest {
            root: root.clone(),
            archive_path: archive_path.clone(),
            record_path: record_path.clone(),
            source_closure_path: source_closure_path.clone(),
            deadline,
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}
