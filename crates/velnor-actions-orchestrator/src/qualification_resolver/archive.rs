//! Bounded single-member ZIP verification for immutable receipt artifacts.

use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};
use velnor_actions_contract::{QUALIFICATION_CACHE_RECEIPT_FILENAME, QualificationCacheArtifact};

use crate::OrchestratorError;
use crate::internal::internal;

/// Maximum compressed artifact bytes read from the API.
pub(super) const MAX_ARCHIVE_BYTES: usize = 1_048_576;
const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

/// Verify the immutable archive and return its sole receipt file.
pub(super) fn receipt_bytes(
    archive: &[u8],
    artifact: &QualificationCacheArtifact,
) -> Result<Vec<u8>, OrchestratorError> {
    if archive.is_empty()
        || archive.len() > MAX_ARCHIVE_BYTES
        || archive.len() as u64 != artifact.size_bytes
        || sha256(archive) != artifact.digest
    {
        return Err(internal("qualification_artifact_integrity"));
    }
    let cursor = Cursor::new(archive);
    let mut zip =
        zip::ZipArchive::new(cursor).map_err(|_| internal("qualification_zip_invalid"))?;
    if zip.len() != 1 {
        return Err(internal("qualification_zip_member_count"));
    }
    let file = zip
        .by_index(0)
        .map_err(|_| internal("qualification_zip_invalid"))?;
    if file.name_raw() != QUALIFICATION_CACHE_RECEIPT_FILENAME.as_bytes()
        || !file.is_file()
        || file.is_symlink()
        || file.enclosed_name().is_none()
        || file.encrypted()
        || file.size() > velnor_actions_contract::MAX_QUALIFICATION_RECEIPT_BYTES as u64
        || file.compressed_size() > MAX_ARCHIVE_BYTES as u64
    {
        return Err(internal("qualification_zip_member_invalid"));
    }
    let expected =
        usize::try_from(file.size()).map_err(|_| internal("qualification_receipt_size"))?;
    let mut bytes = Vec::with_capacity(expected);
    file.take((expected + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| internal("qualification_zip_crc_or_read"))?;
    if bytes.len() != expected {
        return Err(internal("qualification_receipt_size"));
    }
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::from("sha256:");
    for byte in digest {
        text.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        text.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    text
}

#[cfg(test)]
#[path = "archive_tests.rs"]
mod tests;
