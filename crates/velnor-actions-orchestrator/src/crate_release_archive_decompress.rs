// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{MAX_UNPACKED, OrchestratorError, internal};
use flate2::bufread::GzDecoder;
use std::io::{Cursor, Read};

pub(super) fn decompress_archive(bytes: &[u8]) -> Result<Vec<u8>, OrchestratorError> {
    let mut decoder = GzDecoder::new(Cursor::new(bytes));
    let mut uncompressed = Vec::new();
    decoder
        .by_ref()
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut uncompressed)
        .map_err(|_| internal("crate_archive_gzip_invalid"))?;
    if uncompressed.len() as u64 > MAX_UNPACKED
        || decoder.into_inner().position() != bytes.len() as u64
    {
        return Err(internal("crate_archive_gzip_trailing_or_large"));
    }
    Ok(uncompressed)
}
