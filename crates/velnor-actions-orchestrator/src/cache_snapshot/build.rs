// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use velnor_actions_contract::workflow::cache_identity::GenerationId;

use super::index::collect_state_index;
use super::plan::{
    excluded_policy_digest, plan_projection_payload, projection_index_digest,
    required_output_index_digest, state_index_digest, validate_required_outputs,
};
use super::types::{
    LaneRoot, PROJECTION_ARCHIVE_PREFIX, SNAPSHOT_ARCHIVE_FORMAT, SNAPSHOT_MTIME_PRECISION,
    SNAPSHOT_SCHEMA, SnapshotInputs, SnapshotManifestV1, SnapshotPlan, StateEntryV1,
    PlannedProjectionEntry,
};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::source_prep::cache_source_projection::EntryKind;

fn planned_payload_bytes(
    planned: &[PlannedProjectionEntry],
    checkout_root: &Path,
    staged_root: &Path,
) -> Result<BTreeMap<String, u64>, OrchestratorError> {
    let mut sizes = BTreeMap::new();
    for entry in planned {
        let root = if entry.staged {
            staged_root
        } else {
            checkout_root
        };
        let absolute = root.join(&entry.record.path);
        let metadata = fs::symlink_metadata(&absolute)
            .map_err(|_| internal("snapshot_projection_payload_missing"))?;
        let size = if entry.record.kind == EntryKind::File {
            if !metadata.is_file() {
                return Err(internal("snapshot_projection_payload_kind"));
            }
            metadata.len()
        } else {
            0
        };
        sizes.insert(entry.record.path.clone(), size);
    }
    Ok(sizes)
}

fn verify_required_presence(
    state_index: &[StateEntryV1],
    projection: &[PlannedProjectionEntry],
    required: &[String],
) -> Result<(), OrchestratorError> {
    let state_files = state_index
        .iter()
        .filter(|entry| entry.kind != EntryKind::Directory.as_str())
        .map(|entry| entry.path.clone())
        .collect::<BTreeSet<_>>();
    let projection_files = projection
        .iter()
        .filter(|entry| entry.record.kind == EntryKind::File)
        .map(|entry| format!("{PROJECTION_ARCHIVE_PREFIX}/{}", entry.record.path))
        .collect::<BTreeSet<_>>();
    for path in required {
        if !state_files.contains(path) && !projection_files.contains(path) {
            return Err(internal("snapshot_required_output_missing"));
        }
    }
    Ok(())
}

fn component_bytes(
    state_index: &[StateEntryV1],
    projection_sizes: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, u64>, OrchestratorError> {
    let state_bytes = state_index
        .iter()
        .filter(|entry| entry.kind == EntryKind::File.as_str())
        .map(|entry| entry.size)
        .try_fold(0_u64, |total, size| {
            total
                .checked_add(size)
                .ok_or_else(|| internal("snapshot_size_overflow"))
        })?;
    let projection_bytes = projection_sizes.values().try_fold(0_u64, |total, size| {
        total
            .checked_add(*size)
            .ok_or_else(|| internal("snapshot_size_overflow"))
    })?;
    Ok(BTreeMap::from([
        ("state".to_owned(), state_bytes),
        ("projection".to_owned(), projection_bytes),
    ]))
}

fn assemble_manifest(
    inputs: &SnapshotInputs,
    state_index: &[StateEntryV1],
    projection: &[PlannedProjectionEntry],
) -> Result<SnapshotManifestV1, OrchestratorError> {
    let projection_records = projection
        .iter()
        .map(|entry| entry.record.clone())
        .collect::<Vec<_>>();
    let projection_sizes = planned_payload_bytes(
        projection,
        &inputs.checkout_root,
        &inputs.staged_projection_root,
    )?;
    let mut lane_roots = inputs
        .lanes
        .iter()
        .map(LaneRoot::archive_prefix)
        .collect::<Result<Vec<_>, _>>()?;
    lane_roots.sort();
    let manifest = SnapshotManifestV1 {
        schema: SNAPSHOT_SCHEMA,
        generation: inputs.generation.clone(),
        producer_job: inputs.producer_job.clone(),
        source_binding_digest: inputs.source_binding_digest.clone(),
        claimed_completed_recipe_ids: inputs.claimed_completed_recipe_ids.clone(),
        requested_coverage_digest: inputs.requested_coverage_digest.clone(),
        compatibility_digest: inputs.compatibility_digest.clone(),
        paired_source_generation: inputs.paired_source_generation.clone(),
        paired_tool_generation: inputs.paired_tool_generation.clone(),
        paired_source_archive_digest: inputs.paired_source_archive_digest.clone(),
        paired_tool_archive_digest: inputs.paired_tool_archive_digest.clone(),
        lane_roots,
        projection_index_digest: projection_index_digest(&projection_records)?,
        state_index_digest: state_index_digest(state_index)?,
        required_output_index_digest: required_output_index_digest(&inputs.required_output_paths)?,
        archive_format: SNAPSHOT_ARCHIVE_FORMAT.to_owned(),
        mtime_precision: SNAPSHOT_MTIME_PRECISION.to_owned(),
        excluded_path_policy_digest: excluded_policy_digest(
            &inputs.excluded_roots,
            &inputs.excluded_path_policy,
        )?,
        raw_bytes_by_component: component_bytes(state_index, &projection_sizes)?,
        entry_count: u64::try_from(state_index.len() + projection.len())
            .map_err(|_| internal("snapshot_size_range"))?,
    };
    manifest.validate()?;
    Ok(manifest)
}

pub fn build_snapshot_plan(inputs: &SnapshotInputs) -> Result<SnapshotPlan, OrchestratorError> {
    GenerationId::validate_key(&inputs.generation.cache_key()).map_err(internal_contract)?;
    inputs.producer_job.validate().map_err(internal_contract)?;
    if !inputs.checkout_root.is_absolute() {
        return Err(internal("snapshot_checkout_root_not_absolute"));
    }
    if !inputs.staged_projection_root.is_absolute() {
        return Err(internal("snapshot_staged_root_not_absolute"));
    }
    if !inputs.checkout_root.is_dir() || !inputs.staged_projection_root.is_dir() {
        return Err(internal("snapshot_projection_root_not_directory"));
    }
    let state_index = collect_state_index(&inputs.lanes, &inputs.excluded_roots)?;
    let projection = plan_projection_payload(
        &inputs.projection_staged,
        &inputs.projection_declared,
        &inputs.checkout_root,
    )?;
    validate_required_outputs(&inputs.required_output_paths)?;
    verify_required_presence(&state_index, &projection, &inputs.required_output_paths)?;
    let manifest = assemble_manifest(inputs, &state_index, &projection)?;
    Ok(SnapshotPlan {
        manifest,
        state_index,
        projection,
        lanes: inputs.lanes.clone(),
        required_outputs: inputs.required_output_paths.clone(),
    })
}
