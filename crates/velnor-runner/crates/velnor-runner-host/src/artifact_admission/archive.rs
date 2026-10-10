//! Bounded validator for the exact OCI-layout tar produced by the image release.

use std::collections::{BTreeMap, BTreeSet};

use bollard::models::ContainerConfig;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::HostError;

use super::hash::lowercase_hex;
use super::manifest::{ARCHIVE_FORMAT, IMAGE_TAG, MAX_MANIFEST_BYTES, PLATFORM, ProbeManifest};

#[path = "archive/tar_archive.rs"]
mod tar_archive;
use tar_archive::read_archive;

#[cfg(test)]
#[path = "archive/tests.rs"]
mod tests;

const MAX_ARCHIVE_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARCHIVE_MEMBERS: usize = 256;
const MAX_MEMBER_BYTES: usize = 8 * 1024 * 1024;
mod image;
use image::{
    blob_path, file, has_annotation, parse_json, validate_image_descriptor, validate_referrer,
};

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ArchiveIdentity {
    pub(super) image_manifest_digest: String,
    pub(super) image_manifest_media_type: String,
    pub(super) image_manifest_size: u64,
    pub(super) config_digest: String,
    pub(super) config: ContainerConfig,
}

pub(super) fn validate(
    archive: &[u8],
    manifest: &ProbeManifest,
) -> Result<ArchiveIdentity, HostError> {
    if archive.is_empty()
        || archive.len() > MAX_ARCHIVE_BYTES
        || lowercase_hex(&Sha256::digest(archive)) != manifest.archive_sha256
        || manifest.archive_format != ARCHIVE_FORMAT
        || manifest.platform != PLATFORM
    {
        return Err(HostError::Identity);
    }
    let files = read_archive(archive)?;
    let layout = parse_json(file(&files, "oci-layout")?, "OCI layout")?;
    if layout != serde_json::json!({"imageLayoutVersion": "1.0.0"}) {
        return Err(HostError::Identity);
    }
    let index_bytes = file(&files, "index.json")?;
    if lowercase_hex(&Sha256::digest(index_bytes)) != manifest.oci_index_sha256 {
        return Err(HostError::Identity);
    }
    let index = parse_json(index_bytes, "OCI index")?;
    validate_index_header(&index)?;
    let (compatible_config, compatible_layers) = compatibility_entry(&files)?;
    let descriptors = index
        .get("manifests")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or(HostError::Identity)?;
    let mut referenced = BTreeSet::new();
    let mut images = Vec::new();
    let mut referrers = Vec::new();
    for descriptor in descriptors {
        if has_annotation(descriptor, "io.containerd.manifest.subject")? {
            referrers.push(descriptor);
        } else {
            images.push(validate_image_descriptor(
                descriptor,
                &files,
                &compatible_config,
                &compatible_layers,
                &manifest.source_commit,
                &mut referenced,
            )?);
        }
    }
    if images.len() != 1 || referrers.len() > 1 {
        return Err(HostError::Identity);
    }
    let image = images.pop().ok_or(HostError::Identity)?;
    if image.image_manifest_digest != manifest.image_manifest_digest
        || image.config_digest != manifest.config_digest
    {
        return Err(HostError::Identity);
    }
    if let Some(referrer) = referrers.first() {
        validate_referrer(
            referrer,
            &files,
            &image.image_manifest_digest,
            &mut referenced,
        )?;
    }
    let archive_blobs: BTreeSet<String> = files
        .keys()
        .filter(|name| name.starts_with("blobs/"))
        .cloned()
        .collect();
    if archive_blobs != referenced {
        return Err(HostError::Identity);
    }
    Ok(ArchiveIdentity {
        image_manifest_digest: image.image_manifest_digest,
        image_manifest_media_type: image.image_manifest_media_type,
        image_manifest_size: image.image_manifest_size,
        config_digest: image.config_digest,
        config: image.config,
    })
}

fn validate_index_header(index: &Value) -> Result<(), HostError> {
    if index.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || index.get("mediaType").and_then(Value::as_str)
            != Some("application/vnd.oci.image.index.v1+json")
    {
        return Err(HostError::Identity);
    }
    Ok(())
}

fn compatibility_entry(
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<(String, Vec<String>), HostError> {
    let bytes = file(files, "manifest.json")?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(HostError::Frame);
    }
    let document = parse_json(bytes, "Docker compatibility manifest")?;
    let rows = document
        .as_array()
        .filter(|rows| rows.len() == 1)
        .ok_or(HostError::Identity)?;
    let entry = rows.first().ok_or(HostError::Identity)?;
    if entry.get("RepoTags") != Some(&serde_json::json!([IMAGE_TAG])) {
        return Err(HostError::Identity);
    }
    let config = blob_path(
        entry
            .get("Config")
            .and_then(Value::as_str)
            .ok_or(HostError::Identity)?,
    )?;
    let layers = entry
        .get("Layers")
        .and_then(Value::as_array)
        .ok_or(HostError::Identity)?
        .iter()
        .map(|layer| blob_path(layer.as_str().ok_or(HostError::Identity)?))
        .collect::<Result<Vec<_>, HostError>>()?;
    Ok((config, layers))
}
