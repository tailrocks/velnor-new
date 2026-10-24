//! Native, source-bound construction and validation of one job-image candidate.

use std::path::PathBuf;

use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;

#[path = "image_maintenance/build.rs"]
mod build;
#[path = "image_maintenance/source.rs"]
mod source;
#[cfg(test)]
#[path = "image_maintenance_tests.rs"]
mod tests;
#[path = "image_maintenance/validate.rs"]
mod validate;

/// Fixed Buildx installation request; callers choose only workspace locations and deadline.
#[derive(Debug, Clone)]
pub struct InstallBuildxRequest {
    /// Checked-out repository root.
    pub root: PathBuf,
    /// Private job temporary directory for the verified plugin.
    pub runner_temp: PathBuf,
    /// Shared absolute workflow deadline.
    pub deadline: CheckDeadline,
}

/// Source identity and closure-artifact request.
#[derive(Debug, Clone)]
pub struct CheckJobSourceRequest {
    /// Checked-out repository root.
    pub root: PathBuf,
    /// Expected full Git commit.
    pub source_commit: String,
    /// Exclusive native source-closure output path.
    pub source_closure_path: PathBuf,
    /// Shared absolute workflow deadline.
    pub deadline: CheckDeadline,
}

/// Isolated OCI build request.
#[derive(Debug, Clone)]
pub struct BuildJobCandidateRequest {
    /// Checked-out repository root.
    pub root: PathBuf,
    /// Expected full Git commit.
    pub source_commit: String,
    /// Private job temporary directory.
    pub runner_temp: PathBuf,
    /// New OCI archive output path.
    pub archive_path: PathBuf,
    /// New v1 candidate record output path.
    pub record_path: PathBuf,
    /// Authenticated native source-closure artifact.
    pub source_closure_path: PathBuf,
    /// Shared absolute workflow deadline.
    pub deadline: CheckDeadline,
}

/// Candidate validation request for the download-side workflow operation.
#[derive(Debug, Clone)]
pub struct ValidateJobCandidateRequest {
    /// Checked-out repository root.
    pub root: PathBuf,
    /// Candidate OCI archive.
    pub archive_path: PathBuf,
    /// Candidate v1 record.
    pub record_path: PathBuf,
    /// Separately attested native producer source closure.
    pub source_closure_path: PathBuf,
    /// Shared absolute workflow deadline.
    pub deadline: CheckDeadline,
}

/// Install the exact Buildx asset pinned by `.velnor/runner.lock`.
///
/// # Errors
/// Rejects a non-hosted operation, invalid lock, download, or checksum mismatch.
pub fn install_buildx(request: &InstallBuildxRequest) -> Result<(), OrchestratorError> {
    build::install_buildx(request)
}

/// Verify checkout identity and write the independently attested implementation closure.
///
/// # Errors
/// Rejects source drift, dirty/untracked files, unsafe inputs, or an existing output.
pub fn check_job_source(request: &CheckJobSourceRequest) -> Result<(), OrchestratorError> {
    source::check_job_source(request)
}

/// Build and validate the candidate archive and legacy v1 record.
///
/// # Errors
/// Rejects source/workflow mismatch, unsafe paths, invalid Buildx, or invalid OCI bytes.
pub fn build_job_candidate(request: &BuildJobCandidateRequest) -> Result<(), OrchestratorError> {
    build::build_job_candidate(request)
}

/// Validate downloaded candidate bytes against the current source and native closure.
///
/// # Errors
/// Rejects artifact, record, closure, source, workflow, or OCI identity mismatch.
pub fn validate_job_candidate(
    request: &ValidateJobCandidateRequest,
) -> Result<(), OrchestratorError> {
    validate::validate_job_candidate(request)
}
