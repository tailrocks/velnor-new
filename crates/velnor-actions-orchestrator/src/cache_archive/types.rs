// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use crate::cache_snapshot::{SnapshotManifestV1, SnapshotPlan, StateEntryV1};
use crate::source_prep::cache_source_projection::ProjectionEntry;

pub const MAX_ARCHIVE_BYTES: u64 = 1024 * 1024 * 1024;

pub const MAX_ARCHIVE_ENTRIES: usize = 100_000;

pub const MAX_ARCHIVE_ENTRY_BYTES: u64 = 256 * 1024 * 1024;

pub const MAX_ARCHIVE_TOTAL_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRequest<'a> {
    pub plan: &'a SnapshotPlan,
    pub checkout_root: &'a Path,
    pub staged_projection_root: &'a Path,
    pub out_path: &'a Path,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedArchive {
    pub archive_sha256: String,
    pub archive_bytes: u64,
    pub member_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedSnapshot {
    pub manifest: SnapshotManifestV1,
    pub state_index: Vec<StateEntryV1>,
    pub projection: Vec<ProjectionEntry>,
    pub required_outputs: Vec<String>,
}
