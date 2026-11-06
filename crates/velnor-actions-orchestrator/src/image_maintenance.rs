// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;

#[path = "image_maintenance/build.rs"]
mod build;
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
#[path = "image_maintenance/runner_profiles.rs"]
mod runner_profiles;
#[path = "image_maintenance/runner_qualification.rs"]
mod runner_qualification;
#[path = "image_maintenance/runner_records.rs"]
mod runner_records;

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
pub struct RecordRunnerCandidatesRequest {
    pub root: PathBuf,
    pub source_commit: String,
    pub runner_archive: PathBuf,
    pub dind_archive: PathBuf,
    pub probe_archive: PathBuf,
    pub output_directory: PathBuf,
    pub deadline: CheckDeadline,
}

pub fn record_runner_candidates(
    request: &RecordRunnerCandidatesRequest,
) -> Result<(), OrchestratorError> {
    runner_records::record(request)
}

#[derive(Debug, Clone)]
pub struct WriteRunnerQualificationRequest {
    pub root: PathBuf,
    pub source_commit: String,
    pub runner_archive: PathBuf,
    pub runner_record: PathBuf,
    pub dind_archive: PathBuf,
    pub dind_record: PathBuf,
    pub probe_archive: PathBuf,
    pub probe_record: PathBuf,
    pub runtime_result: PathBuf,
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub qualifier_job_id: u64,
    pub output: PathBuf,
    pub deadline: CheckDeadline,
}

pub fn write_runner_qualification(
    request: &WriteRunnerQualificationRequest,
) -> Result<(), OrchestratorError> {
    runner_qualification::write(request)
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
    publication::publish_image(request)
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
