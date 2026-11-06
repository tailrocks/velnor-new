// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use velnor_actions_contract::parse_runner_image_qualification;

use super::super::validate;
use super::{ImagePublicationProfile, PublishImageRequest, authority};
use crate::{OrchestratorError, internal::internal};

const PROOF_NAME: &str = "RUNNER_IMAGE_QUALIFICATION.json";
pub(super) const SIGNER_WORKFLOW: &str = ".github/workflows/runner-image-qualification-attest.yml";

pub(super) struct AdmittedProof {
    pub path: PathBuf,
    pub sha256: String,
    pub qualifier_run_id: u64,
    pub qualifier_run_attempt: u32,
    pub qualifier_job_id: u64,
}

pub(super) fn load(
    request: &PublishImageRequest,
    candidate: &authority::CandidateRecord,
) -> Result<Option<AdmittedProof>, OrchestratorError> {
    if request.profile != ImagePublicationProfile::Job
        && (request.job_attestation_run_id.is_some()
            || request.job_attestation_run_attempt.is_some())
    {
        return Err(internal("image_job_attestation_run_unexpected"));
    }
    let path = match (request.profile, request.qualification_proof.as_deref()) {
        (ImagePublicationProfile::Job, None) => return Ok(None),
        (ImagePublicationProfile::Job, Some(_)) | (_, None) => {
            return Err(internal("image_qualification_proof_required"));
        }
        (_, Some(path)) => path,
    };
    if path.file_name().and_then(|name| name.to_str()) != Some(PROOF_NAME) {
        return Err(internal("image_qualification_proof_filename"));
    }
    let bytes = validate::read_limited(path, authority::MAX_RECORD_BYTES)?;
    let proof = parse_runner_image_qualification(&bytes)
        .map_err(|_| internal("image_qualification_proof_invalid"))?;
    let lock = candidate
        .lock
        .as_ref()
        .ok_or_else(|| internal("image_qualification_proof_lock"))?;
    if proof.schema != "velnor.runner-image-qualification.v1"
        || proof.source_commit != candidate.source_commit
        || proof.source_ref != candidate.source_ref
        || proof.runner_lock_sha256 != lock.sha256
        || proof.producer_run_id != request.producer_run_id
        || proof.producer_run_attempt != request.producer_run_attempt
        || proof.qualifier_run_id == 0
        || proof.qualifier_run_attempt == 0
        || proof.qualifier_job_id == 0
    {
        return Err(internal("image_qualification_proof_identity"));
    }
    let image = match request.profile {
        ImagePublicationProfile::Runner => &proof.images.runner,
        ImagePublicationProfile::Dind => &proof.images.dind,
        ImagePublicationProfile::ResourceProbe => &proof.images.resource_probe,
        ImagePublicationProfile::Job => return Err(internal("image_qualification_proof_profile")),
    };
    if image.archive != candidate.archive || image.oci != candidate.oci {
        return Err(internal("image_qualification_proof_candidate"));
    }
    Ok(Some(AdmittedProof {
        path: path.to_path_buf(),
        sha256: authority::hex(&Sha256::digest(bytes)),
        qualifier_run_id: proof.qualifier_run_id,
        qualifier_run_attempt: proof.qualifier_run_attempt,
        qualifier_job_id: proof.qualifier_job_id,
    }))
}

pub(super) fn expected_subject(proof: &AdmittedProof) -> BTreeMap<String, String> {
    BTreeMap::from([(PROOF_NAME.to_owned(), proof.sha256.clone())])
}

pub(super) fn unchanged(proof: &AdmittedProof) -> Result<(), OrchestratorError> {
    if authority::record_file_digest(Path::new(&proof.path))? != proof.sha256 {
        return Err(internal("image_qualification_proof_changed"));
    }
    Ok(())
}
