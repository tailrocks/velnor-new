// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

#[path = "image_maintenance/mac_publication_context.rs"]
mod mac_publication_context;
pub use mac_publication_context::{AdmitMacPublicationRequest, AdmittedMacPublication, admit_mac_publication};

#[path = "image_maintenance/mac_signer.rs"]
mod mac_signer;
pub use mac_signer::{VerifiedMacAttestation, VerifyMacAttestationRequest, verify_mac_attestation};

#[path = "image_maintenance/mac_expected.rs"]
mod mac_expected;
#[path = "image_maintenance/mac_inbox.rs"]
mod mac_inbox;
#[path = "image_maintenance/mac_preview_sdk.rs"]
mod mac_preview_sdk;

#[path = "image_maintenance/mac_proof.rs"]
pub(crate) mod mac_proof;

#[path = "image_maintenance/build.rs"]
mod build;
#[path = "image_maintenance/runner_source_batch.rs"]
mod runner_source_batch;
#[path = "image_maintenance/source.rs"]
mod source;

#[path = "image_maintenance/validate.rs"]
mod validate;

#[path = "image_maintenance/promotion.rs"]
mod promotion;
#[path = "image_maintenance/publication.rs"]
mod publication;
#[path = "image_maintenance/runner_admission.rs"]
mod runner_admission;
#[path = "image_maintenance/runner_builder.rs"]
mod runner_builder;
#[path = "image_maintenance/runner_builder_output.rs"]
mod runner_builder_output;
#[path = "image_maintenance/runner_buildx.rs"]
mod runner_buildx;
#[path = "image_maintenance/runner_contexts.rs"]
mod runner_contexts;
#[path = "image_maintenance/runner_family_admission/mod.rs"]
mod runner_family_admission;
#[path = "image_maintenance/runner_profiles.rs"]
mod runner_profiles;

#[derive(Debug, Clone)]
pub struct InstallBuildxRequest {
    pub root: PathBuf,
    pub runner_temp: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone)]
pub struct CheckJobSourceRequest {
    pub root: PathBuf,
    pub source_commit: String,
    pub source_closure_path: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone)]
pub struct BuildJobCandidateRequest {
    pub root: PathBuf,
    pub source_commit: String,
    pub runner_temp: PathBuf,
    pub archive_path: PathBuf,
    pub record_path: PathBuf,
    pub source_closure_path: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone)]
pub struct ValidateJobCandidateRequest {
    pub root: PathBuf,
    pub archive_path: PathBuf,
    pub record_path: PathBuf,
    pub source_closure_path: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone)]
pub struct AdmitRunnerImagesRequest {
    pub root: PathBuf,
    pub source_commit: String,
    pub runner_archive: PathBuf,
    pub dind_archive: PathBuf,
    pub probe_archive: PathBuf,
    pub descriptors_output: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone)]
pub struct BuildRunnerImagesRequest {
    pub root: PathBuf,
    pub source_commit: String,
    pub runner_temp: PathBuf,
    pub contexts_dir: PathBuf,
    pub output_dir: PathBuf,
    pub purpose: RunnerImageBuildPurpose,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerImageBuildPurpose {
    Qualification,
    Publication,
}

pub fn build_runner_images(request: &BuildRunnerImagesRequest) -> Result<(), OrchestratorError> {
    request
        .deadline
        .scope(|| runner_builder::build(request))
        .map_err(|_| internal("runner_build_deadline"))?
}

pub trait RunnerImageBuildExecution {
    fn run_build(
        &self,
        command: &velnor_actions_mise::IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<velnor_actions_mise::ProcessOutput, velnor_actions_mise::MiseError>;

    fn run(
        &self,
        command: &velnor_actions_mise::IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<velnor_actions_mise::ProcessOutput, velnor_actions_mise::MiseError>;
}

impl RunnerImageBuildExecution for velnor_actions_mise::ImageCandidateCommand {
    fn run_build(
        &self,
        command: &velnor_actions_mise::IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<velnor_actions_mise::ProcessOutput, velnor_actions_mise::MiseError> {
        velnor_actions_mise::ImageCandidateCommand::run_build(command, deadline)
    }

    fn run(
        &self,
        command: &velnor_actions_mise::IsolatedCommand,
        deadline: CheckDeadline,
    ) -> Result<velnor_actions_mise::ProcessOutput, velnor_actions_mise::MiseError> {
        velnor_actions_mise::ImageCandidateCommand::run(command, deadline)
    }
}

pub fn build_runner_images_with_execution(
    request: &BuildRunnerImagesRequest,
    execution: &dyn RunnerImageBuildExecution,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .scope(|| runner_builder::build_with_execution(request, execution))
        .map_err(|_| internal("runner_build_deadline"))?
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImagePublicationProfile {
    Job,
    Runner,
    Dind,
    ResourceProbe,
}

#[derive(Debug, Clone)]
pub struct PublishImageRequest {
    pub profile: ImagePublicationProfile,
    pub source_commit: String,
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub artifact_id: u64,
    pub archive_path: PathBuf,
    pub record_path: PathBuf,
    pub qualification_proof: Option<PathBuf>,
    pub job_attestation_run_id: Option<u64>,
    pub job_attestation_run_attempt: Option<u32>,
    pub private_scratch: PathBuf,
    pub receipt_output: PathBuf,
    pub deadline: CheckDeadline,
}

#[derive(Debug, Clone)]
pub struct PublishRunnerImagesRequest {
    pub source_commit: String,
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub artifact_id: u64,
    pub assets_dir: PathBuf,
    pub qualification_proof: PathBuf,
    pub private_scratch: PathBuf,
    pub manifest_output: PathBuf,
    pub deadline: CheckDeadline,
}

pub fn publish_runner_images(
    request: &PublishRunnerImagesRequest,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .scope(|| publication::runner_family(request, false))
        .map_err(|_| internal("image_publication_deadline"))?
}

pub fn verify_runner_publication(
    request: &PublishRunnerImagesRequest,
) -> Result<(), OrchestratorError> {
    request
        .deadline
        .scope(|| publication::runner_family(request, true))
        .map_err(|_| internal("image_publication_deadline"))?
}

#[derive(Debug, Clone)]
pub struct PromoteImageRequest {
    pub image_id: String,
    pub source_commit: String,
    pub deadline: CheckDeadline,
}

pub fn promote_image(request: &PromoteImageRequest) -> Result<(), OrchestratorError> {
    promotion::promote(request)
}

pub fn install_buildx(request: &InstallBuildxRequest) -> Result<(), OrchestratorError> {
    build::install_buildx(request)
}

pub fn check_job_source(request: &CheckJobSourceRequest) -> Result<(), OrchestratorError> {
    source::check_job_source(request)
}

pub fn build_job_candidate(request: &BuildJobCandidateRequest) -> Result<(), OrchestratorError> {
    build::build_job_candidate(request)
}

pub fn validate_job_candidate(
    request: &ValidateJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    validate::validate_job_candidate(request)
}

pub fn admit_runner_images(request: &AdmitRunnerImagesRequest) -> Result<(), OrchestratorError> {
    runner_admission::admit_runner_images(request)
}

pub fn publish_image(request: &PublishImageRequest) -> Result<(), OrchestratorError> {
    request
        .deadline
        .scope(|| publication::publish_image(request))
        .map_err(|_| internal("image_publication_deadline"))?
}

pub(crate) fn read_source_file(
    root: &Path,
    source_commit: &str,
    relative: &str,
    limit: usize,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    source::read_source_file(root, source_commit, relative, limit, deadline)
}
