//! Content-addressed Docker load transport for the verified probe image.

use std::io::{Cursor, Read};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tar::{Archive, Builder, Header};

use super::types::{LegacyManifest, OciIndex};
use super::unpack::unpack;
use super::{IMAGE_TAG, MAX_ARCHIVE_BYTES, ProviderFailure, hex, valid_hex, valid_sha};

const INDEX_FILE: &str = "index.json";
const LEGACY_MANIFEST_FILE: &str = "manifest.json";
const IMAGE_NAME: &str = "velnor-resource-probe";
const OCI_IMAGE_NAME_ANNOTATION: &str = "io.containerd.image.name";
const OCI_REFERENCE_ANNOTATION: &str = "org.opencontainers.image.ref.name";

pub(super) struct DerivedLoadArchive {
    pub(super) bytes: Vec<u8>,
    pub(super) archive_sha256: String,
    pub(super) index_digest: String,
    pub(super) image_reference: String,
}

pub(super) fn derive(
    archive: &[u8],
    source_sha: &str,
    archive_sha256: &str,
) -> Result<DerivedLoadArchive, ProviderFailure> {
    if !valid_sha(source_sha)
        || !valid_hex(archive_sha256, 64)
        || sha256(archive) != archive_sha256
        || archive.len() > MAX_ARCHIVE_BYTES
    {
        return Err(ProviderFailure::Archive);
    }
    let image_reference = format!("{IMAGE_NAME}:v{source_sha}-{archive_sha256}");
    let original_files = unpack(archive)?;
    let index_bytes = original_files
        .get(INDEX_FILE)
        .ok_or(ProviderFailure::Archive)?;
    let manifest_bytes = original_files
        .get(LEGACY_MANIFEST_FILE)
        .ok_or(ProviderFailure::Archive)?;
    let rewritten_index = rewrite_index(index_bytes, &image_reference)?;
    let rewritten_manifest = rewrite_manifest(manifest_bytes, &image_reference)?;

    let mut expected_files = original_files.clone();
    expected_files.insert(INDEX_FILE.to_owned(), rewritten_index.clone());
    expected_files.insert(LEGACY_MANIFEST_FILE.to_owned(), rewritten_manifest.clone());
    let bytes = pack_with_rewritten_metadata(archive, &rewritten_index, &rewritten_manifest)?;
    if bytes.len() > MAX_ARCHIVE_BYTES || unpack(&bytes)? != expected_files {
        return Err(ProviderFailure::Archive);
    }

    Ok(DerivedLoadArchive {
        archive_sha256: sha256(&bytes),
        index_digest: sha256(&rewritten_index),
        bytes,
        image_reference,
    })
}

fn rewrite_index(bytes: &[u8], image_reference: &str) -> Result<Vec<u8>, ProviderFailure> {
    let _: OciIndex = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let mut index: Value = serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let descriptor = index
        .get_mut("manifests")
        .and_then(Value::as_array_mut)
        .filter(|manifests| manifests.len() == 1)
        .and_then(|manifests| manifests.first_mut())
        .ok_or(ProviderFailure::Archive)?;
    let annotations = descriptor
        .get_mut("annotations")
        .and_then(Value::as_object_mut)
        .filter(|annotations| annotations.len() == 2)
        .ok_or(ProviderFailure::Archive)?;
    let original_image_name = format!("docker.io/library/{IMAGE_TAG}");
    if annotations
        .get(OCI_IMAGE_NAME_ANNOTATION)
        .and_then(Value::as_str)
        != Some(original_image_name.as_str())
        || annotations
            .get(OCI_REFERENCE_ANNOTATION)
            .and_then(Value::as_str)
            != Some("linux-amd64")
    {
        return Err(ProviderFailure::Archive);
    }
    let tag = image_reference
        .split_once(':')
        .map(|(_, tag)| tag)
        .ok_or(ProviderFailure::Archive)?;
    annotations.insert(
        OCI_IMAGE_NAME_ANNOTATION.to_owned(),
        Value::String(format!("docker.io/library/{image_reference}")),
    );
    annotations.insert(
        OCI_REFERENCE_ANNOTATION.to_owned(),
        Value::String(tag.to_owned()),
    );
    serde_json::to_vec(&index).map_err(|_| ProviderFailure::Archive)
}

fn rewrite_manifest(bytes: &[u8], image_reference: &str) -> Result<Vec<u8>, ProviderFailure> {
    let entries: Vec<LegacyManifest> =
        serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    if entries.len() != 1 || entries[0].repo_tags != [IMAGE_TAG] {
        return Err(ProviderFailure::Archive);
    }
    let mut legacy_manifest: Value =
        serde_json::from_slice(bytes).map_err(|_| ProviderFailure::Archive)?;
    let entry = legacy_manifest
        .as_array_mut()
        .filter(|entries| entries.len() == 1)
        .and_then(|entries| entries.first_mut())
        .ok_or(ProviderFailure::Archive)?;
    entry["RepoTags"] = Value::Array(vec![Value::String(image_reference.to_owned())]);
    serde_json::to_vec(&legacy_manifest).map_err(|_| ProviderFailure::Archive)
}

fn pack_with_rewritten_metadata(
    archive_bytes: &[u8],
    rewritten_index: &[u8],
    rewritten_manifest: &[u8],
) -> Result<Vec<u8>, ProviderFailure> {
    let mut archive = Archive::new(Cursor::new(archive_bytes));
    let entries = archive.entries().map_err(|_| ProviderFailure::Archive)?;
    let mut builder = Builder::new(Vec::new());
    for entry in entries {
        let mut entry = entry.map_err(|_| ProviderFailure::Archive)?;
        let path = entry
            .path()
            .map_err(|_| ProviderFailure::Archive)?
            .to_str()
            .ok_or(ProviderFailure::Archive)?
            .trim_end_matches('/')
            .to_owned();
        let mut header = Header::new_gnu();
        header
            .as_mut_bytes()
            .copy_from_slice(entry.header().as_bytes());
        let mut original_contents = Vec::new();
        entry
            .read_to_end(&mut original_contents)
            .map_err(|_| ProviderFailure::Archive)?;
        let replacement = match path.as_str() {
            INDEX_FILE => Some(rewritten_index),
            LEGACY_MANIFEST_FILE => Some(rewritten_manifest),
            _ => None,
        };
        let contents = replacement.unwrap_or(&original_contents);
        if replacement.is_some() {
            header.set_size(u64::try_from(contents.len()).map_err(|_| ProviderFailure::Archive)?);
            header.set_cksum();
        }
        builder
            .append(&header, contents)
            .map_err(|_| ProviderFailure::Archive)?;
    }
    builder.into_inner().map_err(|_| ProviderFailure::Archive)
}

fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
