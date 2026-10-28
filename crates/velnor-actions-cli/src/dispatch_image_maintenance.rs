// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::args::{ImageMaintenanceCommand, ImagePublicationProfileArg};
use std::{fmt::Display, path::Path, process::ExitCode, time::Duration};
use velnor_actions_orchestrator::{
    AdmitRunnerImagesRequest, BuildJobCandidateRequest, CheckDeadline, CheckJobSourceRequest,
    ImagePublicationProfile, InstallBuildxRequest, OrchestratorError, PublishImageRequest,
    ValidateJobCandidateRequest, admit_runner_images, build_job_candidate, check_job_source,
    install_buildx, publish_image, validate_job_candidate,
};

struct PublishImageArguments<'a> {
    profile: &'a ImagePublicationProfileArg,
    source_commit: &'a str,
    producer_run_id: u64,
    producer_run_attempt: u32,
    artifact_id: u64,
    archive_path: &'a Path,
    record_path: &'a Path,
    private_scratch: &'a Path,
    receipt_output: &'a Path,
}

pub(crate) fn run(command: &ImageMaintenanceCommand) -> ExitCode {
    let deadline = match CheckDeadline::after(Duration::from_mins(10)) {
        Ok(deadline) => deadline,
        Err(error) => return report_error(&error),
    };
    report(run_with_deadline(command, deadline))
}

fn run_with_deadline(
    command: &ImageMaintenanceCommand,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    match command {
        ImageMaintenanceCommand::InstallBuildx { root, runner_temp } => {
            install_request(root, runner_temp, deadline)
        }
        ImageMaintenanceCommand::CheckJobSource {
            root,
            source_commit,
            source_closure_path,
        } => check_source_request(root, source_commit, source_closure_path, deadline),
        ImageMaintenanceCommand::BuildJobCandidate {
            root,
            source_commit,
            runner_temp,
            archive_path,
            record_path,
            source_closure_path,
        } => build_candidate_request(
            root,
            source_commit,
            runner_temp,
            archive_path,
            record_path,
            source_closure_path,
            deadline,
        ),
        ImageMaintenanceCommand::ValidateJobCandidate {
            root,
            archive_path,
            record_path,
            source_closure_path,
        } => validate_candidate_request(
            root,
            archive_path,
            record_path,
            source_closure_path,
            deadline,
        ),
        ImageMaintenanceCommand::AdmitRunnerImages {
            root,
            source_commit,
            runner_archive,
            dind_archive,
            probe_archive,
            descriptors_output,
        } => admit_images_request(
            root,
            source_commit,
            runner_archive,
            dind_archive,
            probe_archive,
            descriptors_output,
            deadline,
        ),
        ImageMaintenanceCommand::PublishImage {
            profile,
            source_commit,
            producer_run_id,
            producer_run_attempt,
            artifact_id,
            archive_path,
            record_path,
            private_scratch,
            receipt_output,
        } => {
            let arguments = PublishImageArguments {
                profile,
                source_commit,
                producer_run_id: *producer_run_id,
                producer_run_attempt: *producer_run_attempt,
                artifact_id: *artifact_id,
                archive_path,
                record_path,
                private_scratch,
                receipt_output,
            };
            publish_image_request(&arguments, deadline)
        }
    }
}

fn install_request(
    root: &Path,
    runner_temp: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    install_buildx(&InstallBuildxRequest {
        root: root.to_path_buf(),
        runner_temp: runner_temp.to_path_buf(),
        deadline,
    })
}

fn check_source_request(
    root: &Path,
    source_commit: &str,
    source_closure_path: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_job_source(&CheckJobSourceRequest {
        root: root.to_path_buf(),
        source_commit: source_commit.to_owned(),
        source_closure_path: source_closure_path.to_path_buf(),
        deadline,
    })
}

fn build_candidate_request(
    root: &Path,
    source_commit: &str,
    runner_temp: &Path,
    archive_path: &Path,
    record_path: &Path,
    source_closure_path: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    build_job_candidate(&BuildJobCandidateRequest {
        root: root.to_path_buf(),
        source_commit: source_commit.to_owned(),
        runner_temp: runner_temp.to_path_buf(),
        archive_path: archive_path.to_path_buf(),
        record_path: record_path.to_path_buf(),
        source_closure_path: source_closure_path.to_path_buf(),
        deadline,
    })
}

fn validate_candidate_request(
    root: &Path,
    archive_path: &Path,
    record_path: &Path,
    source_closure_path: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    validate_job_candidate(&ValidateJobCandidateRequest {
        root: root.to_path_buf(),
        archive_path: archive_path.to_path_buf(),
        record_path: record_path.to_path_buf(),
        source_closure_path: source_closure_path.to_path_buf(),
        deadline,
    })
}

fn admit_images_request(
    root: &Path,
    source_commit: &str,
    runner_archive: &Path,
    dind_archive: &Path,
    probe_archive: &Path,
    descriptors_output: &Path,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    admit_runner_images(&AdmitRunnerImagesRequest {
        root: root.to_path_buf(),
        source_commit: source_commit.to_owned(),
        runner_archive: runner_archive.to_path_buf(),
        dind_archive: dind_archive.to_path_buf(),
        probe_archive: probe_archive.to_path_buf(),
        descriptors_output: descriptors_output.to_path_buf(),
        deadline,
    })
}

fn publish_image_request(
    arguments: &PublishImageArguments<'_>,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    publish_image(&PublishImageRequest {
        profile: publication_profile(*arguments.profile),
        source_commit: arguments.source_commit.to_owned(),
        producer_run_id: arguments.producer_run_id,
        producer_run_attempt: arguments.producer_run_attempt,
        artifact_id: arguments.artifact_id,
        archive_path: arguments.archive_path.to_path_buf(),
        record_path: arguments.record_path.to_path_buf(),
        private_scratch: arguments.private_scratch.to_path_buf(),
        receipt_output: arguments.receipt_output.to_path_buf(),
        deadline,
    })
}

const fn publication_profile(profile: ImagePublicationProfileArg) -> ImagePublicationProfile {
    match profile {
        ImagePublicationProfileArg::Job => ImagePublicationProfile::Job,
        ImagePublicationProfileArg::Runner => ImagePublicationProfile::Runner,
        ImagePublicationProfileArg::Dind => ImagePublicationProfile::Dind,
        ImagePublicationProfileArg::ResourceProbe => ImagePublicationProfile::ResourceProbe,
    }
}

fn report(result: Result<(), OrchestratorError>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(&error),
    }
}

fn report_error(error: &dyn Display) -> ExitCode {
    eprintln!("{error}");
    ExitCode::from(1)
}
