// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "cache_snapshot/build.rs"]
mod build;
#[path = "cache_snapshot/index.rs"]
mod index;
#[path = "cache_snapshot/plan.rs"]
mod plan;
#[path = "cache_snapshot/types.rs"]
mod types;

pub use build::build_snapshot_plan;
pub use index::{collect_state_index, digest_record, is_excluded};
pub use plan::{
    archived_projection_records, excluded_policy_digest, manifest_canonical_bytes,
    parse_manifest_bytes, parse_projection_index_bytes, parse_required_output_bytes,
    parse_state_index_bytes, plan_projection_payload, projection_index_digest,
    required_output_index_digest, state_index_digest, validate_required_outputs,
    validate_state_index,
};
pub use types::{
    ArchivedProjectionEntry, LaneRoot, MANIFEST_ARCHIVE_MEMBER, MAX_SNAPSHOT_ENTRIES,
    MAX_SNAPSHOT_PAYLOAD_BYTES, PROJECTION_ARCHIVE_PREFIX, PROJECTION_INDEX_ARCHIVE_MEMBER,
    PlannedProjectionEntry, REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER, SNAPSHOT_ARCHIVE_FORMAT,
    SNAPSHOT_MTIME_PRECISION, SNAPSHOT_SCHEMA, STATE_ARCHIVE_PREFIX, STATE_INDEX_ARCHIVE_MEMBER,
    SnapshotInputs, SnapshotManifestV1, SnapshotPlan, StateEntryV1, StateMtime,
};
