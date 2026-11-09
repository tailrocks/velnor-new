// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::args::{ImageMaintenanceCommand, ImagePublicationProfileArg};
use std::{fmt::Display, path::Path, process::ExitCode, time::Duration};
use velnor_actions_orchestrator::{
    AdmitRunnerImagesRequest, BuildJobCandidateRequest, BuildRunnerImagesRequest, CheckDeadline,
    CheckJobSourceRequest, ImagePublicationProfile, InstallBuildxRequest, OrchestratorError,
    PromoteImageRequest, PublishImageRequest, PublishRunnerImagesRequest, RunnerImageBuildPurpose,
    ValidateJobCandidateRequest, admit_runner_images, build_job_candidate, build_runner_images,
    check_job_source, install_buildx, promote_image, publish_image, publish_runner_images,
    validate_job_candidate, verify_runner_publication,
};

struct PublishImageArguments<'a> {
    profile: &'a ImagePublicationProfileArg,
    source_commit: &'a str,
    producer_run_id: u64,
    producer_run_attempt: u32,
    artifact_id: u64,
    archive_path: &'a Path,
    record_path: &'a Path,
    qualification_proof: Option<&'a Path>,
    job_attestation_run_id: Option<u64>,
    job_attestation_run_attempt: Option<u32>,
    private_scratch: &'a Path,
    receipt_output: &'a Path,
}

#[cfg(test)]
#[path = "dispatch_image_maintenance_tests.rs"]
mod tests;

fn deadline_minutes(command: &ImageMaintenanceCommand) -> u16 {
    if matches!(command, ImageMaintenanceCommand::BuildRunnerImages { .. }) {
        velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_BUILD_TIMEOUT_MINUTES
    } else if matches!(
        command,
        ImageMaintenanceCommand::PublishRunnerImages(_)
            | ImageMaintenanceCommand::VerifyRunnerPublication(_)
    ) {
        velnor_actions_contract::config::RUNNER_IMAGE_FAMILY_PUBLICATION_TIMEOUT_MINUTES
    } else {
        10
    }
}

pub(crate) fn run(command: &ImageMaintenanceCommand) -> ExitCode {
    let minutes = deadline_minutes(command);
    let deadline = match CheckDeadline::after(Duration::from_mins(u64::from(minutes))) {
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
        ImageMaintenanceCommand::BuildRunnerImages { .. } => {
            build_runner_request(command, deadline)
        }
        ImageMaintenanceCommand::PromoteImage {
            image_id,
            source_commit,
        } => promote_image(&PromoteImageRequest {
            image_id: image_id.clone(),
            source_commit: source_commit.clone(),
            deadline,
        }),
        ImageMaintenanceCommand::PublishImage { .. } => publish_request(command, deadline),
        ImageMaintenanceCommand::PublishRunnerImages(args) => {
            runner_publication_request(args, deadline, false)
        }
        ImageMaintenanceCommand::VerifyRunnerPublication(args) => {
            runner_publication_request(args, deadline, true)
        }
    }
}

fn build_runner_request(
    command: &ImageMaintenanceCommand,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let ImageMaintenanceCommand::BuildRunnerImages {
        root,
        source_commit,
        runner_temp,
        contexts_dir,
        publication_records,
        output_dir,
    } = command
    else {
        unreachable!()
    };
    build_runner_images(&BuildRunnerImagesRequest {
        root: root.clone(),
        source_commit: source_commit.to_owned(),
        runner_temp: runner_temp.clone(),
        contexts_dir: contexts_dir.clone(),
        purpose: if *publication_records {
            RunnerImageBuildPurpose::Publication
        } else {
            RunnerImageBuildPurpose::Qualification
        },
        output_dir: output_dir.clone(),
        deadline,
    })
}

fn runner_publication_request(
    args: &crate::args::RunnerFamilyPublicationArgs,
    deadline: CheckDeadline,
    verify_only: bool,
) -> Result<(), OrchestratorError> {
    let request = PublishRunnerImagesRequest {
        source_commit: args.source_commit.clone(),
        producer_run_id: args.producer_run_id,
        producer_run_attempt: args.producer_run_attempt,
        artifact_id: args.artifact_id,
        assets_dir: args.assets_dir.clone(),
        qualification_proof: args.qualification_proof.clone(),
        private_scratch: args.private_scratch.clone(),
        manifest_output: args.manifest_output.clone(),
        deadline,
    };
    if verify_only {
        verify_runner_publication(&request)
    } else {
        publish_runner_images(&request)
    }
}

fn publish_request(
    command: &ImageMaintenanceCommand,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let ImageMaintenanceCommand::PublishImage {
        profile,
        source_commit,
        producer_run_id,
        producer_run_attempt,
        artifact_id,
        archive_path,
        record_path,
        qualification_proof,
        job_attestation_run_id,
        job_attestation_run_attempt,
        private_scratch,
        receipt_output,
    } = command
    else {
        unreachable!()
    };

    let arguments = PublishImageArguments {
        profile,
        source_commit,
        producer_run_id: *producer_run_id,
        producer_run_attempt: *producer_run_attempt,
        artifact_id: *artifact_id,
        archive_path,
        record_path,
        qualification_proof: qualification_proof.as_deref(),
        job_attestation_run_id: *job_attestation_run_id,
        job_attestation_run_attempt: *job_attestation_run_attempt,
        private_scratch,
        receipt_output,
    };
    publish_image_request(&arguments, deadline)
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
        qualification_proof: arguments.qualification_proof.map(Path::to_path_buf),
        job_attestation_run_id: arguments.job_attestation_run_id,
        job_attestation_run_attempt: arguments.job_attestation_run_attempt,
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
