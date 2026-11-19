// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The receipt artifact archive: its size and digest are the ones the API gave, and it holds
//! exactly one regular member, the receipt.

use std::io::{Cursor, Read};

use sha2::{Digest, Sha256};

use super::types::{Artifact, MAX_RECEIPT_BYTES, RECEIPT_FILENAME};
use crate::error::CiError;
use crate::write_request::internal;

const MAX_ARCHIVE_BYTES: usize = 1_048_576;
const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) fn receipt_bytes(archive: &[u8], artifact: &Artifact) -> Result<Vec<u8>, CiError> {
    if archive.is_empty()
        || archive.len() > MAX_ARCHIVE_BYTES
        || archive.len() as u64 != artifact.size_bytes
        || sha256(archive) != artifact.digest
    {
        return Err(internal("qualification_artifact_integrity"));
    }
    let invalid = || internal("qualification_zip_invalid");
    let mut zip = zip::ZipArchive::new(Cursor::new(archive)).map_err(|_| invalid())?;
    zip.validate_header_consistency().map_err(|_| invalid())?;
    if zip.len() != 1 {
        return Err(internal("qualification_zip_member_count"));
    }
    let file = zip.by_index(0).map_err(|_| invalid())?;
    if file.name_raw() != RECEIPT_FILENAME.as_bytes()
        || !file.is_file()
        || file.is_symlink()
        || file.enclosed_name().is_none()
        || file.encrypted()
        || file.size() > MAX_RECEIPT_BYTES as u64
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
    let mut text = String::from("sha256:");
    for byte in Sha256::digest(bytes) {
        text.push(char::from(LOWER_HEX[usize::from(byte >> 4)]));
        text.push(char::from(LOWER_HEX[usize::from(byte & 0x0f)]));
    }
    text
}
