// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, os::unix::fs::PermissionsExt, path::Path};

use crate::{OrchestratorError, exclusive_write, internal::internal};
use serde::Serialize;
use tempfile::Builder;

use super::{ImagePublicationProfile, PublishImageRequest};

#[path = "publication_attestation.rs"]
mod attestation;
#[path = "publication_authority.rs"]
mod authority;
#[path = "publication_crane.rs"]
mod crane;
#[path = "publication_github.rs"]
mod github;

const RECEIPT_SCHEMA: &str = "velnor.image-publication.v1";

#[derive(Serialize)]
struct PublicationReceipt<'a> {
    schema: &'static str,
    profile: &'static str,
    source_commit: &'a str,
    source_ref: &'a str,
    producer_run_id: u64,
    producer_run_attempt: u32,
    artifact_id: u64,
    workflow_path: &'a str,
    workflow_authority_sha: &'a str,
    ci_run_id: u64,
    ci_run_attempt: u32,
    repository: &'a str,
    tag: String,
    registry_digest: String,
    platform_manifest_digest: &'a str,
    image_configuration_digest: &'a str,
    archive_sha256: &'a str,
    archive_size_bytes: u64,
    record_sha256: &'a str,
    lock_sha256: &'a str,
    idempotent: bool,
}

pub(super) fn publish_image(request: &PublishImageRequest) -> Result<(), OrchestratorError> {
    request
        .deadline
        .remaining()
        .map_err(|_| internal("image_publication_deadline"))?;
    validate_scratch(&request.private_scratch)?;
    let candidate = authority::load_candidate(request)?;
    authority::verify_archive_bytes(request, &candidate)?;
    let (producer, _) = github::authenticate(request, &candidate, request.deadline)?;
    let evidence = authority::inspect_archive(request, &candidate)?;
    let repository = authority::repository(request.profile);
    let tag = authority::tag(&request.source_commit);
    let published = with_owned_directory(&request.private_scratch, |directory| {
        let layout = directory.join("oci");
        materialize(request, &evidence, &layout)?;
        crane::publish(&layout, repository, &tag, &candidate.oci, request.deadline)
    })?;
    let receipt = make_receipt(request, &candidate, &producer, repository, &tag, published);
    let bytes =
        serde_json::to_vec(&receipt).map_err(|_| internal("image_publication_receipt_encode"))?;
    exclusive_write::write_exclusive_until(
        &request.receipt_output,
        &bytes,
        "image_publication_receipt",
        || {
            request
                .deadline
                .remaining()
                .map_err(|_| internal("image_publication_deadline"))?;
            Ok(())
        },
    )
}

fn validate_scratch(path: &Path) -> Result<(), OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("image_publication_scratch_missing"))?;
    if !path.is_absolute()
        || !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || metadata.permissions().mode() & 0o777 != 0o700
        || path.canonicalize().ok().as_deref() != Some(path)
    {
        return Err(internal("image_publication_scratch_private"));
    }
    Ok(())
}

fn with_owned_directory<T>(
    scratch: &Path,
    operation: impl FnOnce(&Path) -> Result<T, OrchestratorError>,
) -> Result<T, OrchestratorError> {
    let directory = Builder::new()
        .prefix("velnor-image-publish-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(scratch)
        .map_err(|_| internal("image_publication_scratch_create"))?;
    let path = directory.path().display().to_string();
    let result = operation(directory.path());
    match (result, directory.close()) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(original), Ok(())) => Err(original),
        (Ok(_), Err(cleanup)) => Err(OrchestratorError::io(
            path,
            format!("image_publication_scratch_cleanup: {cleanup}"),
        )),
        (Err(original), Err(cleanup)) => Err(OrchestratorError::io(
            path,
            format!("image_publication_scratch_cleanup: {cleanup}; original error: {original}"),
        )),
    }
}

fn materialize(
    request: &PublishImageRequest,
    evidence: &velnor_archive_guard::owned_archive_guard::oci_candidate::OciCandidateEvidence,
    layout: &Path,
) -> Result<(), OrchestratorError> {
    let materialized = request
        .deadline
        .scope(|| {
            velnor_archive_guard::owned_archive_guard::oci_candidate::materialize_oci_candidate(
                &request.archive_path,
                layout,
                evidence,
            )
        })
        .map_err(|_| internal("image_publication_deadline"))?
        .map_err(|error| {
            if error.is_deadline_exceeded() {
                internal("image_publication_deadline")
            } else {
                internal(&error.to_string())
            }
        })?;
    if &materialized != evidence {
        return Err(internal("image_publication_materialization_identity"));
    }
    Ok(())
}

fn make_receipt<'a>(
    request: &'a PublishImageRequest,
    candidate: &'a authority::CandidateRecord,
    producer: &'a github::AuthenticatedProducer,
    repository: &'a str,
    tag: &str,
    published: crane::PublishedImage,
) -> PublicationReceipt<'a> {
    let lock_sha = candidate
        .source_inputs
        .get(".velnor/runner.lock")
        .map(String::as_str)
        .unwrap_or_default();
    PublicationReceipt {
        schema: RECEIPT_SCHEMA,
        profile: profile_name(request.profile),
        source_commit: &candidate.source_commit,
        source_ref: &candidate.source_ref,
        producer_run_id: request.producer_run_id,
        producer_run_attempt: request.producer_run_attempt,
        artifact_id: request.artifact_id,
        workflow_path: &producer.workflow_path,
        workflow_authority_sha: &producer.head_sha,
        ci_run_id: producer.ci_run_id,
        ci_run_attempt: producer.ci_run_attempt,
        repository,
        tag: tag.to_owned(),
        registry_digest: published.digest,
        platform_manifest_digest: &candidate.oci.platform_manifest_digest,
        image_configuration_digest: &candidate.oci.image_configuration_digest,
        archive_sha256: &candidate.archive.sha256,
        archive_size_bytes: candidate.archive.size_bytes,
        record_sha256: &candidate.record_sha256,
        lock_sha256: lock_sha,
        idempotent: published.idempotent,
    }
}

const fn profile_name(profile: ImagePublicationProfile) -> &'static str {
    match profile {
        ImagePublicationProfile::Job => "job",
        ImagePublicationProfile::Runner => "runner",
        ImagePublicationProfile::Dind => "dind",
        ImagePublicationProfile::ResourceProbe => "resource_probe",
    }
}
