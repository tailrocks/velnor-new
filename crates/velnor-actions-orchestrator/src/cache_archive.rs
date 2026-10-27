// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "cache_archive/extract.rs"]
mod extract;
#[path = "cache_archive/pack.rs"]
mod pack;
#[path = "cache_archive/types.rs"]
mod types;
#[path = "cache_archive/verify.rs"]
mod verify;

#[path = "cache_archive_envelope.rs"]
pub(crate) mod cache_archive_envelope;

pub use pack::pack_snapshot_archive;
pub use types::{
    ExtractedSnapshot, MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_ENTRY_BYTES,
    MAX_ARCHIVE_TOTAL_BYTES, PackRequest, PackedArchive,
};
pub use verify::extract_snapshot_archive;
