use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::types::{Descriptor, OciManifest};
use super::model::{ArchiveFiles, WorkerImage};
use super::{ProviderFailure, hex, valid_hex};

pub(super) fn referenced_blobs(
    image_index: &Descriptor,
    image_manifest: &Descriptor,
    attestation_manifest: &Descriptor,
    manifest: &OciManifest,
    archive: &ArchiveFiles,
) -> Result<HashSet<String>, ProviderFailure> {
    let mut expected = HashSet::new();
    for descriptor in [image_index, image_manifest, attestation_manifest] {
        expected.insert(blob_path(&descriptor.digest)?);
    }
    for descriptor in [&manifest.config].into_iter().chain(manifest.layers.iter()) {
        expected.insert(blob_path(&descriptor.digest)?);
    }
    let attestation = get_manifest(archive, attestation_manifest)?;
    expected.insert(blob_path(&attestation.config.digest)?);
    for descriptor in &attestation.layers {
        expected.insert(blob_path(&descriptor.digest)?);
    }
    Ok(expected)
}

pub(super) fn get_manifest(
    archive: &ArchiveFiles,
    descriptor: &Descriptor,
) -> Result<OciManifest, ProviderFailure> {
    let bytes = get_blob(archive, descriptor)?;
    serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)
}

pub(super) fn get_blob<'a>(
    archive: &'a ArchiveFiles,
    descriptor: &Descriptor,
) -> Result<&'a [u8], ProviderFailure> {
    let path = verify_blob(archive, descriptor)?;
    let blob = archive.blobs.get(&path).ok_or(ProviderFailure::Archive)?;
    blob.metadata.as_deref().ok_or(ProviderFailure::Archive)
}

pub(super) fn verify_blob(
    archive: &ArchiveFiles,
    descriptor: &Descriptor,
) -> Result<String, ProviderFailure> {
    verify_descriptor(descriptor)?;
    let path = blob_path(&descriptor.digest)?;
    let blob = archive.blobs.get(&path).ok_or(ProviderFailure::Archive)?;
    if blob.digest
        != descriptor
            .digest
            .strip_prefix("sha256:")
            .unwrap_or_default()
        || blob.size != descriptor.size
    {
        return Err(ProviderFailure::Archive);
    }
    Ok(path)
}

pub(super) fn verify_descriptor(descriptor: &Descriptor) -> Result<(), ProviderFailure> {
    let digest = descriptor
        .digest
        .strip_prefix("sha256:")
        .filter(|digest| valid_hex(digest, 64))
        .ok_or(ProviderFailure::Archive)?;
    if descriptor.size == 0 || digest.is_empty() {
        Err(ProviderFailure::Archive)
    } else {
        Ok(())
    }
}

pub(super) fn verify_layout(bytes: &[u8]) -> Result<(), ProviderFailure> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    value
        .as_object()
        .is_some_and(|object| {
            object.len() == 1
                && object.get("imageLayoutVersion").and_then(Value::as_str) == Some("1.0.0")
        })
        .then_some(())
        .ok_or(ProviderFailure::Archive)
}

pub(super) fn outer_annotations(image: WorkerImage, tag: &str) -> HashMap<String, String> {
    let Some((name, tag)) = tag.split_once(':') else {
        return HashMap::new();
    };
    if name != image.image_name() || tag.is_empty() {
        return HashMap::new();
    }
    HashMap::from([
        (
            "io.containerd.image.name".to_owned(),
            format!("{}:{tag}", image.docker_image_name()),
        ),
        (
            "org.opencontainers.image.ref.name".to_owned(),
            tag.to_owned(),
        ),
    ])
}

pub(super) fn blob_path(digest: &str) -> Result<String, ProviderFailure> {
    let digest = digest
        .strip_prefix("sha256:")
        .filter(|value| valid_hex(value, 64))
        .ok_or(ProviderFailure::Archive)?;
    Ok(format!("blobs/sha256/{digest}"))
}

pub(super) fn valid_sha256(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|digest| valid_hex(digest, 64))
}

pub(super) fn valid_docker_reference(image: WorkerImage, reference: &str) -> bool {
    let Some((name, tag)) = reference.split_once(':') else {
        return false;
    };
    name == image.image_name()
        && !tag.is_empty()
        && tag.len() <= 128
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.-".contains(&byte))
        && tag.as_bytes()[0].is_ascii_alphanumeric()
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
