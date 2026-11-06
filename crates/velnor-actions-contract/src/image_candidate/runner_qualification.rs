// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::image_candidate_record::{ImageCandidateArchive, ImageCandidateOci};
use crate::ContractError;

pub const RUNNER_QUALIFICATION_SCHEMA: &str = "velnor.runner-image-qualification.v1";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImageQualification {
    pub schema: String,
    pub source_commit: String,
    pub source_ref: String,
    pub runner_lock_sha256: String,
    pub producer_run_id: u64,
    pub producer_run_attempt: u32,
    pub qualifier_run_id: u64,
    pub qualifier_run_attempt: u32,
    pub qualifier_job_id: u64,
    pub images: RunnerImageQualificationImages,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImageQualificationImages {
    pub runner: RunnerImageQualificationImage,
    pub dind: RunnerImageQualificationImage,
    pub resource_probe: RunnerImageQualificationImage,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImageQualificationImage {
    pub archive: ImageCandidateArchive,
    pub oci: ImageCandidateOci,
}

pub fn parse_runner_image_qualification(
    bytes: &[u8],
) -> Result<RunnerImageQualification, ContractError> {
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let value = crate::strict_json::parse_strict_json_with_limit(text, 1024 * 1024)
        .map_err(|_| invalid())?;
    let proof: RunnerImageQualification = serde_json::from_value(value).map_err(|_| invalid())?;
    validate_runner_image_qualification(&proof)?;
    Ok(proof)
}

pub fn validate_runner_image_qualification(
    proof: &RunnerImageQualification,
) -> Result<(), ContractError> {
    if proof.schema != RUNNER_QUALIFICATION_SCHEMA
        || !hex(&proof.source_commit, 40)
        || proof.source_ref != "refs/heads/main"
        || !hex(&proof.runner_lock_sha256, 64)
        || proof.producer_run_id == 0
        || proof.producer_run_attempt == 0
        || proof.qualifier_run_id == 0
        || proof.qualifier_run_attempt == 0
        || proof.qualifier_job_id == 0
        || proof.producer_run_id == proof.qualifier_run_id
        || [
            &proof.images.runner,
            &proof.images.dind,
            &proof.images.resource_probe,
        ]
        .iter()
        .any(|image| !valid_image(image))
    {
        return Err(invalid());
    }
    Ok(())
}

fn valid_image(image: &RunnerImageQualificationImage) -> bool {
    let oci_digest = |value: &str| {
        value
            .strip_prefix("sha256:")
            .is_some_and(|value| hex(value, 64))
    };
    hex(&image.archive.sha256, 64)
        && image.archive.size_bytes != 0
        && hex(&image.oci.archive_index_digest, 64)
        && image.oci.archive_index_size_bytes != 0
        && oci_digest(&image.oci.platform_manifest_digest)
        && image.oci.platform_manifest_size_bytes != 0
        && oci_digest(&image.oci.image_configuration_digest)
        && image.oci.image_configuration_size_bytes != 0
        && !image.oci.layer_digests.is_empty()
        && image
            .oci
            .layer_digests
            .iter()
            .all(|value| oci_digest(value))
}

fn hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn invalid() -> ContractError {
    ContractError::CanonicalJson("runner_image_qualification_invalid".to_owned())
}
