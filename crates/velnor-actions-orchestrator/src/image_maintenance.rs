// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;

#[path = "image_maintenance/build.rs"]
mod build;
#[path = "image_maintenance/source.rs"]
mod source;

#[path = "image_maintenance/validate.rs"]
mod validate;

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
