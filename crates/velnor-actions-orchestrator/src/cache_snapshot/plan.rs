// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::canonical_json_bytes;
use velnor_actions_contract::strict_json::parse_strict_json_bytes;

use super::index::digest_record;
use super::types::{
    ArchivedProjectionEntry, MAX_SNAPSHOT_ENTRIES, PlannedProjectionEntry, SnapshotManifestV1,
    StateEntryV1,
};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::source_prep::cache_source_projection::{
    CheckoutState, ProjectionEntry, RestoreDecision, authenticate_entry, decide, is_git_path,
    reconcile, validate_relative_path,
};

pub fn validate_state_index(entries: &[StateEntryV1]) -> Result<(), OrchestratorError> {
    if entries.len() > MAX_SNAPSHOT_ENTRIES {
        return Err(internal("snapshot_entry_limit"));
    }
    let mut seen = BTreeSet::new();
    for entry in entries {
        entry.validate()?;
        if !seen.insert(entry.path.clone()) {
            return Err(internal("snapshot_duplicate_path"));
        }
    }
    Ok(())
}

pub fn state_index_digest(entries: &[StateEntryV1]) -> Result<Digest, OrchestratorError> {
    validate_state_index(entries)?;
    digest_record(entries)
}

pub fn archived_projection_records(
    entries: &[ProjectionEntry],
) -> Result<Vec<ArchivedProjectionEntry>, OrchestratorError> {
    for entry in entries {
        entry.validate()?;
        if entry.mtime.is_none() {
            return Err(internal("snapshot_projection_mtime_missing"));
        }
    }
    let mut records = entries
        .iter()
        .map(ArchivedProjectionEntry::from_entry)
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(records)
}

pub fn projection_index_digest(entries: &[ProjectionEntry]) -> Result<Digest, OrchestratorError> {
    digest_record(&archived_projection_records(entries)?)
}

pub fn required_output_index_digest(paths: &[String]) -> Result<Digest, OrchestratorError> {
    validate_required_outputs(paths)?;
    digest_record(paths)
}

pub fn validate_required_outputs(paths: &[String]) -> Result<(), OrchestratorError> {
    let mut seen = BTreeSet::new();
    for path in paths {
        validate_relative_path(path)?;
        if is_git_path(path) {
            return Err(internal("snapshot_git_path"));
        }
        if !seen.insert(path.clone()) {
            return Err(internal("snapshot_duplicate_path"));
        }
    }
    if !paths
        .iter()
        .zip(paths.iter().skip(1))
        .all(|(left, right)| left < right)
    {
        return Err(internal("snapshot_required_outputs_unsorted"));
    }
    Ok(())
}

pub fn excluded_policy_digest(
    excluded: &[PathBuf],
    policy: &str,
) -> Result<Digest, OrchestratorError> {
    let mut roots = excluded
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>();
    roots.sort();
    digest_record(&(roots, policy))
}

pub fn plan_projection_payload(
    staged: &[ProjectionEntry],
    declared: &[ProjectionEntry],
    checkout_root: &Path,
) -> Result<Vec<PlannedProjectionEntry>, OrchestratorError> {
    if !checkout_root.is_absolute() {
        return Err(internal("snapshot_checkout_root_not_absolute"));
    }
    for entry in staged.iter().chain(declared.iter()) {
        entry.validate()?;
    }
    let mut seen = BTreeSet::new();
    for entry in declared {
        if !seen.insert(entry.path.clone()) {
            return Err(internal("snapshot_duplicate_path"));
        }
    }
    let mut planned = Vec::with_capacity(declared.len());
    for required in declared {
        let state = authenticate_entry(checkout_root, required)?;
        let archived = staged
            .iter()
            .find(|candidate| candidate.path == required.path);
        let decision = decide(archived, Some(required), state);
        let restored = match (archived, decision) {
            (Some(candidate), RestoreDecision::Restore) => {
                reconcile(candidate, required)?;
                Some(candidate.clone())
            }
            _ => None,
        };
        let missing_source = match (state, archived) {
            (CheckoutState::Missing, Some(candidate))
                if candidate.semantic_matches(required) && candidate.mtime.is_some() =>
            {
                Some(candidate.clone())
            }
            _ => None,
        };
        if let Some(record) = restored {
            planned.push(PlannedProjectionEntry {
                record,
                decision,
                staged: true,
            });
        } else if let Some(record) = missing_source {
            planned.push(PlannedProjectionEntry {
                record,
                decision,
                staged: true,
            });
        } else {
            planned.push(PlannedProjectionEntry {
                record: required.clone(),
                decision,
                staged: false,
            });
        }
    }
    Ok(planned)
}

pub fn manifest_canonical_bytes(
    manifest: &SnapshotManifestV1,
) -> Result<Vec<u8>, OrchestratorError> {
    manifest.validate()?;
    canonical_json_bytes(manifest).map_err(internal_contract)
}

pub fn parse_manifest_bytes(bytes: &[u8]) -> Result<SnapshotManifestV1, OrchestratorError> {
    let value = parse_strict_json_bytes(bytes, 8 * 1024 * 1024).map_err(internal_contract)?;
    let manifest: SnapshotManifestV1 = serde_json::from_value(value)
        .map_err(|error| internal(&format!("snapshot_manifest:{error}")))?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn parse_state_index_bytes(bytes: &[u8]) -> Result<Vec<StateEntryV1>, OrchestratorError> {
    let value = parse_strict_json_bytes(bytes, 64 * 1024 * 1024).map_err(internal_contract)?;
    let entries: Vec<StateEntryV1> = serde_json::from_value(value)
        .map_err(|error| internal(&format!("snapshot_state_index:{error}")))?;
    validate_state_index(&entries)?;
    Ok(entries)
}

pub fn parse_projection_index_bytes(
    bytes: &[u8],
) -> Result<Vec<ProjectionEntry>, OrchestratorError> {
    let value = parse_strict_json_bytes(bytes, 64 * 1024 * 1024).map_err(internal_contract)?;
    let records: Vec<ArchivedProjectionEntry> = serde_json::from_value(value)
        .map_err(|error| internal(&format!("snapshot_projection_index:{error}")))?;
    let mut entries = Vec::with_capacity(records.len());
    for record in &records {
        entries.push(record.to_entry()?);
    }
    let canonical = archived_projection_records(&entries)?;
    let round_trip = canonical
        .iter()
        .map(|record| {
            (
                record.path.clone(),
                record.kind.clone(),
                record.mode,
                record.content_sha256.clone(),
                record.link_target.clone(),
            )
        })
        .collect::<Vec<_>>();
    let supplied = records
        .iter()
        .map(|record| {
            (
                record.path.clone(),
                record.kind.clone(),
                record.mode,
                record.content_sha256.clone(),
                record.link_target.clone(),
            )
        })
        .collect::<Vec<_>>();
    if round_trip != supplied {
        return Err(internal("snapshot_projection_index_order"));
    }
    Ok(entries)
}

pub fn parse_required_output_bytes(bytes: &[u8]) -> Result<Vec<String>, OrchestratorError> {
    let value = parse_strict_json_bytes(bytes, 1024 * 1024).map_err(internal_contract)?;
    let paths: Vec<String> = serde_json::from_value(value)
        .map_err(|error| internal(&format!("snapshot_required_outputs:{error}")))?;
    validate_required_outputs(&paths)?;
    Ok(paths)
}
