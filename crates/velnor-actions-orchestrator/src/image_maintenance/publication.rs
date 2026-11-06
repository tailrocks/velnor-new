// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};

use crate::{OrchestratorError, internal::internal};
use serde::Serialize;
use tempfile::Builder;
use velnor_actions_contract::exclusive_write;

use super::{ImagePublicationProfile, PublishImageRequest};

#[path = "publication_attestation.rs"]
mod attestation;
#[path = "publication_authority.rs"]
mod authority;
#[path = "publication_crane.rs"]
pub(super) mod crane;
#[path = "publication_github.rs"]
pub(super) mod github;
#[path = "publication_qualification.rs"]
mod qualification;

const RECEIPT_SCHEMA: &str = "velnor.image-publication.v1";
const PENDING_SCHEMA: &str = "velnor.image-publication-pending.v1";

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum ReceiptStatus {
    Pending,
    Verified,
}

#[derive(Serialize)]
struct PendingReceipt<'a> {
    schema: &'static str,
    status: ReceiptStatus,
    profile: &'static str,
    source_commit: &'a str,
    repository: &'a str,
    tag: &'a str,
    expected_digest: &'a str,
    archive_sha256: &'a str,
    record_sha256: &'a str,
}


#[derive(Serialize)]
struct PublicationReceipt<'a> {
    schema: &'static str,
    status: ReceiptStatus,
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
    let proof = qualification::load(request, &candidate)?;
    let (producer, _) =
        github::authenticate(request, &candidate, proof.as_ref(), request.deadline)?;
    let evidence = authority::inspect_archive(request, &candidate)?;
    let repository = authority::repository(request.profile);
    let tag = authority::tag(&request.source_commit);
    let mut receipt_file =
        exclusive_write::reserve_exclusive(&request.receipt_output, "image_publication_receipt")?;
    let mut journal_started = false;
    let result = with_owned_directory(&request.private_scratch, |directory| {
        let layout = directory.join("oci");
        materialize(request, &evidence, &layout)?;
        let pending = PendingReceipt {
            schema: PENDING_SCHEMA,
            status: ReceiptStatus::Pending,
            profile: profile_name(request.profile),
            source_commit: &request.source_commit,
            repository,
            tag: &tag,
            expected_digest: &candidate.oci.platform_manifest_digest,
            archive_sha256: &candidate.archive.sha256,
            record_sha256: &candidate.record_sha256,
        };
        let pending_bytes = serde_json::to_vec(&pending)
            .map_err(|_| internal("image_publication_receipt_encode"))?;
        exclusive_write::verify_reserved(&receipt_file, &request.receipt_output)?;
        append_receipt(&mut receipt_file, &pending_bytes)?;
        journal_started = true;
        let parent = request.receipt_output.parent().unwrap_or_else(|| Path::new("."));
        fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| internal("image_publication_receipt_unwritable"))?;
        exclusive_write::verify_reserved(&receipt_file, &request.receipt_output)?;
        let published = crane::publish(
            &layout,
            repository,
            &tag,
            &candidate.oci,
            request.deadline,
            || {
                if let Some(proof) = &proof {
                    qualification::unchanged(proof)?;
                }
                github::revalidate_before_publish(request, &producer, request.deadline)
            },
        )?;
        let receipt = make_receipt(request, &candidate, &producer, repository, &tag, published);
        let bytes = serde_json::to_vec(&receipt)
            .map_err(|_| internal("image_publication_receipt_encode"))?;
        exclusive_write::verify_reserved(&receipt_file, &request.receipt_output)?;
        append_receipt(&mut receipt_file, &bytes)?;
        exclusive_write::verify_reserved(&receipt_file, &request.receipt_output)?;
        Ok(())
    });
    if result.is_err() && !journal_started
        && exclusive_write::verify_reserved(&receipt_file, &request.receipt_output).is_ok()
    {
        fs::remove_file(&request.receipt_output)
            .map_err(|_| internal("image_publication_receipt_cleanup"))?;
    }
    result
}

fn append_receipt(file: &mut fs::File, bytes: &[u8]) -> Result<(), OrchestratorError> {
    file.write_all(bytes)
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|_| internal("image_publication_receipt_unwritable"))
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
        status: ReceiptStatus::Verified,
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
