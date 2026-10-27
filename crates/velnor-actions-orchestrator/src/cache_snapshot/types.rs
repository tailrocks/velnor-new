// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::workflow::cache_context::JobBindingV1;
use velnor_actions_contract::workflow::cache_identity::{
    GC1_SCHEMA, GenerationId, require_sorted_unique,
};

use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::source_prep::cache_source_projection::{
    EntryKind, ProjectionEntry, RealMtime, RestoreDecision, is_git_path, sha256_hex,
    validate_link_target, validate_relative_path, validate_sha256,
};

pub const SNAPSHOT_SCHEMA: u32 = GC1_SCHEMA;

pub const SNAPSHOT_ARCHIVE_FORMAT: &str = "payload-tar-gzip-v1";

pub const SNAPSHOT_MTIME_PRECISION: &str = "nanoseconds";

pub const STATE_ARCHIVE_PREFIX: &str = "state";

pub const PROJECTION_ARCHIVE_PREFIX: &str = "projection";

pub const MANIFEST_ARCHIVE_MEMBER: &str = "manifest.json";

pub const STATE_INDEX_ARCHIVE_MEMBER: &str = "state-index.json";

pub const PROJECTION_INDEX_ARCHIVE_MEMBER: &str = "projection-index.json";

pub const REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER: &str = "required-output-index.json";

pub const MAX_SNAPSHOT_ENTRIES: usize = 100_000;

pub const MAX_SNAPSHOT_PAYLOAD_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateMtime {
    pub seconds: i64,
    pub nanoseconds: u32,
}

impl StateMtime {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        RealMtime {
            seconds: self.seconds,
            nanoseconds: self.nanoseconds,
        }
        .validate()
    }

    #[must_use]
    pub fn from_real(value: &RealMtime) -> Self {
        Self {
            seconds: value.seconds,
            nanoseconds: value.nanoseconds,
        }
    }

    pub fn real_mtime(&self) -> Result<RealMtime, OrchestratorError> {
        let mtime = RealMtime {
            seconds: self.seconds,
            nanoseconds: self.nanoseconds,
        };
        mtime.validate()?;
        Ok(mtime)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateEntryV1 {
    pub path: String,
    pub kind: String,
    pub mode: u32,
    pub size: u64,
    pub content_sha256: String,
    pub link_target: Option<String>,
    pub mtime: StateMtime,
}

impl StateEntryV1 {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        validate_relative_path(&self.path)?;
        if is_git_path(&self.path) {
            return Err(internal("snapshot_git_path"));
        }
        let kind = EntryKind::parse(&self.kind)?;
        if self.mode > 0o7777 {
            return Err(internal("snapshot_mode_invalid"));
        }
        validate_sha256(&self.content_sha256)?;
        if kind == EntryKind::Directory {
            if self.content_sha256 != sha256_hex(&[]) {
                return Err(internal("snapshot_directory_digest"));
            }
            if self.size != 0 {
                return Err(internal("snapshot_directory_size"));
            }
            if self.link_target.is_some() {
                return Err(internal("snapshot_link_unexpected"));
            }
        }
        if kind == EntryKind::File && self.link_target.is_some() {
            return Err(internal("snapshot_link_unexpected"));
        }
        if kind == EntryKind::Symlink {
            let Some(target) = self.link_target.as_deref() else {
                return Err(internal("snapshot_link_missing"));
            };
            validate_link_target(target)?;
            if self.size != u64::try_from(target.len()).unwrap_or(u64::MAX) {
                return Err(internal("snapshot_link_size"));
            }
        }
        self.mtime.validate()?;
        Ok(())
    }

    pub fn entry_kind(&self) -> Result<EntryKind, OrchestratorError> {
        EntryKind::parse(&self.kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArchivedProjectionEntry {
    pub path: String,
    pub kind: String,
    pub mode: u32,
    pub content_sha256: String,
    pub link_target: Option<String>,
    pub mtime: StateMtime,
}

impl ArchivedProjectionEntry {
    #[must_use]
    pub fn from_entry(entry: &ProjectionEntry) -> Self {
        let mtime = entry.mtime.as_ref().map_or(
            StateMtime {
                seconds: 0,
                nanoseconds: 0,
            },
            StateMtime::from_real,
        );
        Self {
            path: entry.path.clone(),
            kind: entry.kind.as_str().to_owned(),
            mode: entry.mode,
            content_sha256: entry.content_sha256.clone(),
            link_target: entry.link_target.clone(),
            mtime,
        }
    }

    pub fn to_entry(&self) -> Result<ProjectionEntry, OrchestratorError> {
        let kind = EntryKind::parse(&self.kind)?;
        let entry = ProjectionEntry {
            path: self.path.clone(),
            kind,
            mode: self.mode,
            content_sha256: self.content_sha256.clone(),
            link_target: self.link_target.clone(),
            mtime: Some(self.mtime.real_mtime()?),
        };
        entry.validate()?;
        Ok(entry)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifestV1 {
    pub schema: u32,
    pub generation: GenerationId,
    pub producer_job: JobBindingV1,
    pub source_binding_digest: Digest,
    pub claimed_completed_recipe_ids: Vec<String>,
    pub requested_coverage_digest: Digest,
    pub compatibility_digest: Digest,
    pub paired_source_generation: Option<GenerationId>,
    pub paired_tool_generation: Option<GenerationId>,
    pub paired_source_archive_digest: Option<Digest>,
    pub paired_tool_archive_digest: Option<Digest>,
    pub lane_roots: Vec<String>,
    pub projection_index_digest: Digest,
    pub state_index_digest: Digest,
    pub required_output_index_digest: Digest,
    pub archive_format: String,
    pub mtime_precision: String,
    pub excluded_path_policy_digest: Digest,
    pub raw_bytes_by_component: BTreeMap<String, u64>,
    pub entry_count: u64,
}

impl SnapshotManifestV1 {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        if self.schema != SNAPSHOT_SCHEMA {
            return Err(internal("snapshot_schema_unsupported"));
        }
        GenerationId::validate_key(&self.generation.cache_key()).map_err(internal_contract)?;
        self.producer_job.validate().map_err(internal_contract)?;
        if self.claimed_completed_recipe_ids.is_empty() {
            return Err(internal("snapshot_claimed_recipes_empty"));
        }
        require_sorted_unique(
            "snapshot.claimed_completed_recipe_ids",
            &self.claimed_completed_recipe_ids,
        )
        .map_err(internal_contract)?;
        if let Some(generation) = &self.paired_source_generation {
            GenerationId::validate_key(&generation.cache_key()).map_err(internal_contract)?;
        }
        if let Some(generation) = &self.paired_tool_generation {
            GenerationId::validate_key(&generation.cache_key()).map_err(internal_contract)?;
        }
        if self.lane_roots.is_empty() {
            return Err(internal("snapshot_lane_roots_empty"));
        }
        require_sorted_unique("snapshot.lane_roots", &self.lane_roots)
            .map_err(internal_contract)?;
        for lane_root in &self.lane_roots {
            validate_relative_path(lane_root)?;
        }
        if self.archive_format != SNAPSHOT_ARCHIVE_FORMAT {
            return Err(internal("snapshot_archive_format"));
        }
        if self.mtime_precision != SNAPSHOT_MTIME_PRECISION {
            return Err(internal("snapshot_mtime_precision"));
        }
        let expected_entries = self
            .raw_bytes_by_component
            .keys()
            .filter(|component| *component == "state" || *component == "projection")
            .count();
        if expected_entries != self.raw_bytes_by_component.len() {
            return Err(internal("snapshot_component_names"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneRoot {
    pub lane_id: String,
    pub root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedProjectionEntry {
    pub record: ProjectionEntry,
    pub decision: RestoreDecision,
    pub staged: bool,
}

impl LaneRoot {
    pub fn validate(&self) -> Result<(), OrchestratorError> {
        validate_relative_path(&self.lane_id)?;
        if self.lane_id.contains('/') {
            return Err(internal("snapshot_lane_nested"));
        }
        if !self.root.is_absolute() {
            return Err(internal("snapshot_lane_root_not_absolute"));
        }
        Ok(())
    }

    pub fn archive_prefix(&self) -> Result<String, OrchestratorError> {
        self.validate()?;
        Ok(format!("{STATE_ARCHIVE_PREFIX}/{}", self.lane_id))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotPlan {
    pub manifest: SnapshotManifestV1,
    pub state_index: Vec<StateEntryV1>,
    pub projection: Vec<PlannedProjectionEntry>,
    pub lanes: Vec<LaneRoot>,
    pub required_outputs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotInputs {
    pub generation: GenerationId,
    pub producer_job: JobBindingV1,
    pub source_binding_digest: Digest,
    pub claimed_completed_recipe_ids: Vec<String>,
    pub requested_coverage_digest: Digest,
    pub compatibility_digest: Digest,
    pub paired_source_generation: Option<GenerationId>,
    pub paired_tool_generation: Option<GenerationId>,
    pub paired_source_archive_digest: Option<Digest>,
    pub paired_tool_archive_digest: Option<Digest>,
    pub lanes: Vec<LaneRoot>,
    pub excluded_roots: Vec<PathBuf>,
    pub excluded_path_policy: String,
    pub projection_staged: Vec<ProjectionEntry>,
    pub projection_declared: Vec<ProjectionEntry>,
    pub checkout_root: PathBuf,
    pub staged_projection_root: PathBuf,
    pub required_output_paths: Vec<String>,
}
