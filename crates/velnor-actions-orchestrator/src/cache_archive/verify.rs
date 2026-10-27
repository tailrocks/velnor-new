// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;

use super::extract::{RawKind, RawMember, scan_archive_members};
use super::pack::io_error;
use super::types::ExtractedSnapshot;
use crate::OrchestratorError;
use crate::cache_snapshot::{
    MANIFEST_ARCHIVE_MEMBER, PROJECTION_ARCHIVE_PREFIX, PROJECTION_INDEX_ARCHIVE_MEMBER,
    REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER, STATE_INDEX_ARCHIVE_MEMBER, SnapshotManifestV1,
    StateEntryV1, parse_manifest_bytes, parse_projection_index_bytes,
    parse_required_output_bytes, parse_state_index_bytes, projection_index_digest,
    required_output_index_digest, state_index_digest,
};
use crate::internal::internal;
use crate::source_prep::cache_source_projection::{
    EntryKind, ProjectionEntry, restore_directory_times, restore_times,
};

fn read_index_file(destination: &Path, name: &str) -> Result<Vec<u8>, OrchestratorError> {
    let path = destination.join(name);
    let bytes = fs::read(&path).map_err(|error| io_error(&path, error))?;
    fs::remove_file(&path).map_err(|error| io_error(&path, error))?;
    Ok(bytes)
}

type SnapshotIndexes = (
    SnapshotManifestV1,
    Vec<StateEntryV1>,
    Vec<ProjectionEntry>,
    Vec<String>,
);

fn load_snapshot_indexes(destination: &Path) -> Result<SnapshotIndexes, OrchestratorError> {
    let manifest = parse_manifest_bytes(&read_index_file(destination, MANIFEST_ARCHIVE_MEMBER)?)?;
    let state_index =
        parse_state_index_bytes(&read_index_file(destination, STATE_INDEX_ARCHIVE_MEMBER)?)?;
    let projection = parse_projection_index_bytes(&read_index_file(
        destination,
        PROJECTION_INDEX_ARCHIVE_MEMBER,
    )?)?;
    let required = parse_required_output_bytes(&read_index_file(
        destination,
        REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER,
    )?)?;
    if state_index_digest(&state_index)? != manifest.state_index_digest {
        return Err(internal("snapshot_archive_state_digest_mismatch"));
    }
    if projection_index_digest(&projection)? != manifest.projection_index_digest {
        return Err(internal("snapshot_archive_projection_digest_mismatch"));
    }
    if required_output_index_digest(&required)? != manifest.required_output_index_digest {
        return Err(internal("snapshot_archive_required_digest_mismatch"));
    }
    Ok((manifest, state_index, projection, required))
}

fn verify_member_inventory(
    members: &[RawMember],
    state_index: &[StateEntryV1],
    projection: &[ProjectionEntry],
    required: &[String],
) -> Result<(), OrchestratorError> {
    let mut expected = BTreeSet::new();
    for entry in state_index {
        if !expected.insert(entry.path.clone()) {
            return Err(internal("snapshot_duplicate_path"));
        }
    }
    for record in projection {
        let member = format!("{PROJECTION_ARCHIVE_PREFIX}/{}", record.path);
        if !expected.insert(member) {
            return Err(internal("snapshot_duplicate_path"));
        }
    }
    let actual = members
        .iter()
        .filter(|member| {
            member.name != MANIFEST_ARCHIVE_MEMBER
                && member.name != STATE_INDEX_ARCHIVE_MEMBER
                && member.name != PROJECTION_INDEX_ARCHIVE_MEMBER
                && member.name != REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER
        })
        .map(|member| member.name.clone())
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(internal("snapshot_archive_inventory_mismatch"));
    }
    let present = expected;
    for path in required {
        if !present.contains(path) {
            return Err(internal("snapshot_required_output_missing"));
        }
    }
    Ok(())
}

fn verify_member_contents(
    members: &[RawMember],
    state_index: &[StateEntryV1],
    projection: &[ProjectionEntry],
) -> Result<(), OrchestratorError> {
    let mut states = BTreeMap::new();
    for entry in state_index {
        states.insert(entry.path.clone(), entry);
    }
    let mut projected = HashMap::new();
    for record in projection {
        projected.insert(
            format!("{PROJECTION_ARCHIVE_PREFIX}/{}", record.path),
            record,
        );
    }
    for member in members {
        if let Some(entry) = states.get(&member.name) {
            if entry.mode & !0o777 != 0 {
                return Err(internal("snapshot_mode_special_bits"));
            }
            match (entry.entry_kind()?, &member.kind) {
                (EntryKind::File, RawKind::File) => {
                    if member.hash != entry.content_sha256 || member.size != entry.size {
                        return Err(internal("snapshot_archive_content_mismatch"));
                    }
                }
                (EntryKind::Directory, RawKind::Directory) => {}
                (EntryKind::Symlink, RawKind::Symlink) => {
                    if member.link_target.as_deref() != entry.link_target.as_deref()
                        || member.hash != entry.content_sha256
                    {
                        return Err(internal("snapshot_archive_link_mismatch"));
                    }
                }
                _ => return Err(internal("snapshot_archive_kind_mismatch")),
            }
        }
        if let Some(record) = projected.get(&member.name) {
            if record.mode & !0o777 != 0 {
                return Err(internal("snapshot_mode_special_bits"));
            }
            match (&record.kind, &member.kind) {
                (EntryKind::File, RawKind::File) => {
                    if member.hash != record.content_sha256 {
                        return Err(internal("snapshot_archive_content_mismatch"));
                    }
                }
                (EntryKind::Directory, RawKind::Directory) => {}
                (EntryKind::Symlink, RawKind::Symlink) => {
                    if member.link_target.as_deref() != record.link_target.as_deref()
                        || member.hash != record.content_sha256
                    {
                        return Err(internal("snapshot_archive_link_mismatch"));
                    }
                }
                _ => return Err(internal("snapshot_archive_kind_mismatch")),
            }
        }
    }
    Ok(())
}

fn apply_snapshot_modes(
    destination: &Path,
    state_index: &[StateEntryV1],
    projection: &[ProjectionEntry],
) -> Result<(), OrchestratorError> {
    #[cfg(not(unix))]
    {
        let _ = (destination, state_index, projection);
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for entry in state_index {
            if entry.entry_kind()? == EntryKind::Symlink {
                continue;
            }
            let path = destination.join(&entry.path);
            fs::set_permissions(&path, fs::Permissions::from_mode(entry.mode & 0o777))
                .map_err(|error| io_error(&path, error))?;
        }
        for record in projection {
            if record.kind == EntryKind::Symlink {
                continue;
            }
            let path = destination.join(format!("{PROJECTION_ARCHIVE_PREFIX}/{}", record.path));
            fs::set_permissions(&path, fs::Permissions::from_mode(record.mode & 0o777))
                .map_err(|error| io_error(&path, error))?;
        }
        Ok(())
    }
}

fn restore_snapshot_times(
    destination: &Path,
    state_index: &[StateEntryV1],
    projection: &[ProjectionEntry],
) -> Result<(), OrchestratorError> {
    for entry in state_index {
        if entry.entry_kind()? != EntryKind::File {
            continue;
        }
        restore_times(&destination.join(&entry.path), &entry.mtime.real_mtime()?)
            .map_err(|_| internal("snapshot_archive_mtime_roundtrip"))?;
    }
    for record in projection {
        if record.kind != EntryKind::File {
            continue;
        }
        let Some(mtime) = &record.mtime else {
            return Err(internal("snapshot_projection_mtime_missing"));
        };
        restore_times(
            &destination.join(format!("{PROJECTION_ARCHIVE_PREFIX}/{}", record.path)),
            mtime,
        )
        .map_err(|_| internal("snapshot_archive_mtime_roundtrip"))?;
    }
    let mut directories = Vec::new();
    for entry in state_index {
        if entry.entry_kind()? != EntryKind::Directory {
            continue;
        }
        directories.push((destination.join(&entry.path), entry.mtime.real_mtime()?));
    }
    for record in projection {
        if record.kind != EntryKind::Directory {
            continue;
        }
        let Some(mtime) = &record.mtime else {
            return Err(internal("snapshot_projection_mtime_missing"));
        };
        directories.push((
            destination.join(format!("{PROJECTION_ARCHIVE_PREFIX}/{}", record.path)),
            *mtime,
        ));
    }
    restore_directory_times(&directories)
        .map_err(|_| internal("snapshot_archive_mtime_roundtrip"))?;
    Ok(())
}

pub fn extract_snapshot_archive(
    archive: &Path,
    destination: &Path,
) -> Result<ExtractedSnapshot, OrchestratorError> {
    if !destination.is_absolute() {
        return Err(internal("snapshot_archive_destination_not_absolute"));
    }
    let parent = destination.parent().unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        return Err(internal("snapshot_archive_parent_missing"));
    }
    if fs::symlink_metadata(destination).is_ok() {
        return Err(OrchestratorError::OverwriteRefused {
            path: destination.display().to_string(),
        });
    }
    fs::create_dir(destination).map_err(|error| io_error(destination, error))?;
    let scanned = scan_archive_members(archive, destination)
        .inspect_err(|_| drop(fs::remove_dir_all(destination)))?;
    let (manifest, state_index, projection, required) = load_snapshot_indexes(destination)
        .inspect_err(|_| drop(fs::remove_dir_all(destination)))?;
    let verified = (|| -> Result<(), OrchestratorError> {
        verify_member_inventory(&scanned, &state_index, &projection, &required)?;
        verify_member_contents(&scanned, &state_index, &projection)?;
        apply_snapshot_modes(destination, &state_index, &projection)?;
        restore_snapshot_times(destination, &state_index, &projection)?;
        Ok(())
    })();
    if verified.is_err() {
        drop(fs::remove_dir_all(destination));
    }
    verified?;
    Ok(ExtractedSnapshot {
        manifest,
        state_index,
        projection,
        required_outputs: required,
    })
}
