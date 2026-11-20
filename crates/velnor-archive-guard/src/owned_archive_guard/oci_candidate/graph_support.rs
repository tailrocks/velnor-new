// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{
    OciCandidateError, Result,
    archive::{OciArchive, read_blob},
    layer::LayerDescriptor,
};
use sha2::Digest;

pub(super) use velnor_runner_image_manifest::OciDescriptor;

impl From<OciDescriptor> for LayerDescriptor {
    fn from(value: OciDescriptor) -> Self {
        Self {
            digest: value.digest,
            size: value.size,
            media_type: value.media_type,
        }
    }
}

pub(super) fn descriptor(value: &serde_json::Value, strict: bool) -> Result<OciDescriptor> {
    velnor_runner_image_manifest::parse_oci_descriptor(value, strict)
        .map_err(|error| OciCandidateError(error.0))
}

pub(super) fn read_referenced(
    archive: &mut OciArchive,
    descriptor: &OciDescriptor,
) -> Result<Vec<u8>> {
    let blob = archive
        .blobs
        .get(&descriptor.digest)
        .ok_or(OciCandidateError("oci_blob_missing"))?;
    if blob.size != descriptor.size {
        return Err(OciCandidateError("oci_blob_size"));
    }
    let bytes = read_blob(&mut archive.file, blob, 64 * 1024 * 1024)?;
    if sha256(&bytes) != descriptor.digest {
        return Err(OciCandidateError("oci_blob_digest"));
    }
    Ok(bytes)
}

pub(super) fn add_reference(
    references: &mut std::collections::BTreeMap<String, (u64, String)>,
    digest: &str,
    size: u64,
    media_type: &str,
) -> Result<()> {
    let value = (size, media_type.to_owned());
    if references
        .get(digest)
        .is_some_and(|existing| existing != &value)
    {
        return Err(OciCandidateError("oci_descriptor_conflict"));
    }
    references.insert(digest.to_owned(), value);
    Ok(())
}

pub(super) fn reject_unknown(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<()> {
    velnor_runner_image_manifest::reject_unknown_oci_fields(object, allowed)
        .map_err(|error| OciCandidateError(error.0))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = sha2::Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}
