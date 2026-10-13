//! Bounded parsing of the immutable qualification receipt artifact.

use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};
use velnor_actions_contract::workflow::MAX_QUALIFICATION_RECEIPT_BYTES;
use zip::{CompressionMethod, ZipArchive};

use crate::OrchestratorError;
use crate::internal::internal;

/// Maximum compressed and uncompressed receipt bytes.
pub(crate) const MAX_RECEIPT_ARCHIVE_BYTES: usize = MAX_QUALIFICATION_RECEIPT_BYTES;
pub(crate) const MAX_RECEIPT_DOCUMENT_BYTES: usize = MAX_QUALIFICATION_RECEIPT_BYTES;
/// Sole accepted file in the receipt artifact.
pub(crate) use velnor_actions_contract::workflow::QUALIFICATION_CACHE_RECEIPT_FILENAME as RECEIPT_FILENAME;

#[cfg(test)]
#[path = "qualification_receipt_archive_tests.rs"]
mod tests;

/// Verify the API digest, then extract the one expected regular file.
///
/// Parsing happens only after the complete captured ZIP has been hashed.
/// The archive and uncompressed document both have independent bounds.
///
/// # Errors
/// Returns a machine-readable internal error for any digest, ZIP-shape,
/// member, compression, or size mismatch.
pub(crate) fn verify_and_extract(
    archive_bytes: &[u8],
    api_digest: &str,
) -> Result<Vec<u8>, OrchestratorError> {
    verify_archive_size(archive_bytes)?;
    verify_digest(archive_bytes, api_digest)?;
    extract_receipt(archive_bytes)
}

fn verify_archive_size(bytes: &[u8]) -> Result<(), OrchestratorError> {
    if bytes.is_empty() || bytes.len() > MAX_RECEIPT_ARCHIVE_BYTES {
        return Err(internal("qualification_archive_size_invalid"));
    }
    Ok(())
}

fn verify_digest(bytes: &[u8], expected: &str) -> Result<(), OrchestratorError> {
    let Some(expected) = expected.strip_prefix("sha256:") else {
        return Err(internal("qualification_archive_digest_invalid"));
    };
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(internal("qualification_archive_digest_invalid"));
    }
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest != expected.to_ascii_lowercase() {
        return Err(internal("qualification_archive_digest_mismatch"));
    }
    Ok(())
}

fn extract_receipt(bytes: &[u8]) -> Result<Vec<u8>, OrchestratorError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| internal("qualification_archive_invalid"))?;
    if archive.len() != 1 {
        return Err(internal("qualification_archive_member_count"));
    }
    let member = archive
        .by_index(0)
        .map_err(|_| internal("qualification_archive_member_invalid"))?;
    if member.name() != RECEIPT_FILENAME
        || member.enclosed_name().as_deref() != Some(std::path::Path::new(RECEIPT_FILENAME))
        || !member.is_file()
        || member.encrypted()
        || !matches!(
            member.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        )
        || member.size() == 0
        || member.size() > MAX_RECEIPT_DOCUMENT_BYTES as u64
    {
        return Err(internal("qualification_archive_member_invalid"));
    }
    if member
        .unix_mode()
        .is_some_and(|mode| mode & 0o170000 != 0o100000)
    {
        return Err(internal("qualification_archive_member_type"));
    }
    let size = usize::try_from(member.size())
        .map_err(|_| internal("qualification_archive_document_size"))?;
    let mut document = Vec::with_capacity(size);
    member
        .take(MAX_RECEIPT_DOCUMENT_BYTES as u64 + 1)
        .read_to_end(&mut document)
        .map_err(|_| internal("qualification_archive_member_read"))?;
    if document.is_empty() || document.len() > MAX_RECEIPT_DOCUMENT_BYTES {
        return Err(internal("qualification_archive_document_size"));
    }
    Ok(document)
}
