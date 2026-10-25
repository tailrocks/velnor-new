use std::collections::BTreeMap;
use std::path::Path;

use super::super::types::{Descriptor, OciIndex, OciManifest};
use super::archive_read::{hash_archive, read_archive};
use super::layout::{
    get_blob, outer_annotations, referenced_blobs, sha256, verify_descriptor, verify_layout,
};
use super::model::{
    ArchiveFiles, OCI_INDEX_MEDIA_TYPE, ParsedWorkerArchive, VerifiedSelection, WorkerImage,
};
use super::{ProviderFailure, valid_hex, valid_sha};

pub(in crate::launch::resource_probe::provider) fn verify(
    path: &Path,
    image: WorkerImage,
    source_sha: &str,
    expected_archive_sha256: &str,
    expected_repo_tag: &str,
) -> Result<ParsedWorkerArchive, ProviderFailure> {
    if !valid_sha(source_sha) || !valid_hex(expected_archive_sha256, 64) {
        return Err(ProviderFailure::Archive);
    }
    let archive_sha256 = hash_archive(path)?;
    if archive_sha256 != expected_archive_sha256 {
        return Err(ProviderFailure::Archive);
    }
    let files = read_archive(path)?;
    let verified = verify_files(&files, image, source_sha, expected_repo_tag)?;
    let archive_index = files
        .files
        .get("index.json")
        .ok_or(ProviderFailure::Archive)?;
    Ok(ParsedWorkerArchive {
        archive_sha256,
        archive_index_digest: sha256(archive_index),
        image_index_digest: verified.image_index_digest,
        image_index_size: verified.image_index_size,
        manifest_digest: verified.manifest_digest,
        manifest_size: verified.manifest_size,
        config_digest: verified.config_digest,
        config: verified.config,
        diff_ids: verified.diff_ids,
        repo_tag: verified.repo_tag,
        blob_inventory: verified.blob_inventory,
    })
}

fn verify_files(
    archive: &ArchiveFiles,
    image: WorkerImage,
    source_sha: &str,
    expected_repo_tag: &str,
) -> Result<VerifiedSelection, ProviderFailure> {
    let selected = select_descriptors(archive, image, expected_repo_tag)?;
    let manifest: OciManifest =
        serde_json::from_slice(get_blob(archive, &selected.platform_manifest)?)
            .map_err(|_| ProviderFailure::Archive)?;
    let (config, diff_ids, config_digest) =
        super::runtime::verify_platform_image(archive, image, &manifest)?;
    super::provenance::verify_buildkit_attestation(
        archive,
        image,
        source_sha,
        &selected.platform_manifest,
        &selected.attestation_manifest,
    )?;
    let repo_tag = super::runtime::verify_legacy_manifest(
        archive
            .files
            .get("manifest.json")
            .ok_or(ProviderFailure::Archive)?,
        image,
        &config_digest,
        &manifest.layers,
        expected_repo_tag,
    )?;
    let blob_inventory = verify_blob_inventory(archive, &selected, &manifest)?;
    Ok(VerifiedSelection {
        image_index_digest: selected.image_index.digest,
        image_index_size: selected.image_index.size,
        manifest_digest: selected.platform_manifest.digest,
        manifest_size: selected.platform_manifest.size,
        config_digest,
        config,
        diff_ids,
        repo_tag,
        blob_inventory,
    })
}

struct SelectedDescriptors {
    image_index: Descriptor,
    platform_manifest: Descriptor,
    attestation_manifest: Descriptor,
}

fn select_descriptors(
    archive: &ArchiveFiles,
    image: WorkerImage,
    expected_repo_tag: &str,
) -> Result<SelectedDescriptors, ProviderFailure> {
    verify_layout(
        archive
            .files
            .get("oci-layout")
            .ok_or(ProviderFailure::Archive)?,
    )?;
    let index: OciIndex = serde_json::from_slice(
        archive
            .files
            .get("index.json")
            .ok_or(ProviderFailure::Archive)?,
    )
    .map_err(|_| ProviderFailure::Archive)?;
    if index.schema_version != 2
        || index.media_type != OCI_INDEX_MEDIA_TYPE
        || index.manifests.len() != 1
    {
        return Err(ProviderFailure::Archive);
    }
    let image_index = index.manifests.first().ok_or(ProviderFailure::Archive)?;
    verify_descriptor(image_index)?;
    if image_index.media_type != OCI_INDEX_MEDIA_TYPE
        || image_index.platform.is_some()
        || image_index.artifact_type.is_some()
        || image_index.annotations.as_ref() != Some(&outer_annotations(image, expected_repo_tag))
    {
        return Err(ProviderFailure::Archive);
    }
    let nested: OciIndex = serde_json::from_slice(get_blob(archive, image_index)?)
        .map_err(|_| ProviderFailure::Archive)?;
    if nested.schema_version != 2
        || nested.media_type != OCI_INDEX_MEDIA_TYPE
        || nested.manifests.len() != 2
    {
        return Err(ProviderFailure::Archive);
    }
    let (platform_manifest, attestation_manifest) =
        super::runtime::select_children(&nested.manifests)?;
    Ok(SelectedDescriptors {
        image_index: image_index.clone(),
        platform_manifest: platform_manifest.clone(),
        attestation_manifest: attestation_manifest.clone(),
    })
}

fn verify_blob_inventory(
    archive: &ArchiveFiles,
    selected: &SelectedDescriptors,
    manifest: &OciManifest,
) -> Result<BTreeMap<String, u64>, ProviderFailure> {
    let expected = referenced_blobs(
        &selected.image_index,
        &selected.platform_manifest,
        &selected.attestation_manifest,
        manifest,
        archive,
    )?;
    if expected.len() != archive.blobs.len()
        || archive.blobs.keys().any(|path| !expected.contains(path))
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(archive
        .blobs
        .iter()
        .map(|(path, blob)| (format!("{}:{}", path, blob.digest), blob.size))
        .collect())
}
