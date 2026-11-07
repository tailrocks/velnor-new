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

#[cfg(test)]
mod publication_boundary_worker {
    use super::{ImagePublicationProfile, PublishImageRequest, publish_image};
    use std::{error::Error, fs, io::Write, path::PathBuf, time::Duration};
    use velnor_actions_mise::CheckDeadline;

    #[test]
    #[ignore = "invoked only by the isolated image publication fixture"]
    fn publication() -> Result<(), Box<dyn Error>> {
        let path = PathBuf::from(
            std::env::var_os("VELNOR_IMAGE_TEST_INPUT").ok_or("missing fixture input")?,
        );
        if !path.is_absolute() || fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
            return Err("invalid fixture input".into());
        }
        let data: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let string = |key: &str| -> Result<String, Box<dyn Error>> {
            Ok(data[key]
                .as_str()
                .ok_or("missing fixture field")?
                .to_owned())
        };
        let number = |key: &str| -> Result<u64, Box<dyn Error>> {
            data[key]
                .as_u64()
                .ok_or_else(|| "missing fixture number".into())
        };
        let profile = match string("profile")?.as_str() {
            "job" => ImagePublicationProfile::Job,
            "runner" => ImagePublicationProfile::Runner,
            "dind" => ImagePublicationProfile::Dind,
            "resource_probe" => ImagePublicationProfile::ResourceProbe,
            _ => return Err("invalid publication profile".into()),
        };
        let report = if profile == ImagePublicationProfile::Job {
            serde_json::to_vec(&data["attestation"])?
        } else {
            serde_json::to_vec(&data["qualification_attestation"])?
        };
        crate::release::package_release_signer::inject_verified_output_for_test(report.clone());
        if profile == ImagePublicationProfile::Job {
            crate::release::package_release_signer::inject_verified_output_for_test(report);
        }
        publish_image(&PublishImageRequest {
            profile,
            source_commit: string("source")?,
            producer_run_id: number("producer_run_id")?,
            producer_run_attempt: u32::try_from(number("producer_run_attempt")?)?,
            artifact_id: number("artifact_id")?,
            archive_path: string("archive")?.into(),
            record_path: string("record")?.into(),
            qualification_proof: data["qualification_proof"].as_str().map(Into::into),
            job_attestation_run_id: if profile == ImagePublicationProfile::Job {
                Some(number("job_attestation_run_id")?)
            } else {
                None
            },
            job_attestation_run_attempt: if profile == ImagePublicationProfile::Job {
                Some(u32::try_from(number("producer_run_attempt")?)?)
            } else {
                None
            },
            private_scratch: string("scratch")?.into(),
            receipt_output: string("receipt")?.into(),
            deadline: CheckDeadline::after(Duration::from_secs(90))?,
        })?;
        crate::release::package_release_signer::assert_test_verified_outputs_consumed();
        let mut receipt = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(string("boundary_receipt")?)?;
        receipt.write_all(string("nonce")?.as_bytes())?;
        Ok(())
    }

    #[test]
    #[ignore = "invoked only by the isolated image promotion fixture"]
    fn promotion() -> Result<(), Box<dyn Error>> {
        let path = PathBuf::from(
            std::env::var_os("VELNOR_IMAGE_TEST_INPUT").ok_or("missing fixture input")?,
        );
        if !path.is_absolute() || fs::metadata(&path)?.len() > 16 * 1024 * 1024 {
            return Err("invalid fixture input".into());
        }
        let data: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let field = |key: &str| data[key].as_str().ok_or("missing fixture field");
        super::promotion::enable_test_promotion_verifier();
        super::promote_image(&super::PromoteImageRequest {
            image_id: field("image_id")?.to_owned(),
            source_commit: field("source")?.to_owned(),
            deadline: CheckDeadline::after(Duration::from_secs(90))?,
        })?;
        super::promotion::assert_test_promotion_verifier_used();
        let mut receipt = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(field("boundary_receipt")?)?;
        receipt.write_all(field("nonce")?.as_bytes())?;
        Ok(())
    }
}
