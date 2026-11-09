// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};

use super::super::types::Descriptor;
use super::model::ArchiveFiles;
use super::{ProviderFailure, hex, valid_hex};

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

pub(super) fn blob_path(digest: &str) -> Result<String, ProviderFailure> {
    let digest = digest
        .strip_prefix("sha256:")
        .filter(|value| valid_hex(value, 64))
        .ok_or(ProviderFailure::Archive)?;
    Ok(format!("blobs/sha256/{digest}"))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
