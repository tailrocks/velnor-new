// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const TAR_BLOCK_BYTES: u64 = 512;

pub const TAR_RECORD_BYTES: u64 = 10_240;

pub const OUTER_ENTRY_NAME_RESERVE_BYTES: u64 = 4_608;

pub const OUTER_ENTRY_HEADER_BYTES: u64 =
    TAR_BLOCK_BYTES + TAR_BLOCK_BYTES + OUTER_ENTRY_NAME_RESERVE_BYTES;

pub const TAR_END_OF_ARCHIVE_BYTES: u64 = 2 * TAR_BLOCK_BYTES;

pub const ZSTD_MAX_INPUT_BYTES: u64 = 0xFF00_FF00_FF00_FF00;

const ZSTD_SMALL_INPUT_BYTES: u64 = 128 << 10;

#[must_use]
pub const fn zstd_compress_bound(source: u64) -> Option<u64> {
    if source >= ZSTD_MAX_INPUT_BYTES {
        return None;
    }
    let margin = if source < ZSTD_SMALL_INPUT_BYTES {
        (ZSTD_SMALL_INPUT_BYTES - source) >> 11
    } else {
        0
    };
    Some(source + (source >> 8) + margin)
}

#[must_use]
pub const fn outer_tar_bytes(inner: u64) -> Option<u64> {
    let blocks = match inner.checked_next_multiple_of(TAR_BLOCK_BYTES) {
        Some(blocks) => blocks,
        None => return None,
    };
    let Some(framed) = blocks.checked_add(OUTER_ENTRY_HEADER_BYTES + TAR_END_OF_ARCHIVE_BYTES)
    else {
        return None;
    };
    framed.checked_next_multiple_of(TAR_RECORD_BYTES)
}

#[must_use]
pub const fn outer_entry_bound(inner: u64) -> Option<u64> {
    match outer_tar_bytes(inner) {
        Some(tar) => zstd_compress_bound(tar),
        None => None,
    }
}
