// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use velnor_actions_contract::canonical::Digest;
use velnor_actions_contract::canonical::digest_b3_typed;
use velnor_actions_contract::canonical_json_bytes;

use super::plan::validate_state_index;
use super::types::{
    LaneRoot, MAX_SNAPSHOT_ENTRIES, MAX_SNAPSHOT_PAYLOAD_BYTES, STATE_ARCHIVE_PREFIX, StateEntryV1,
    StateMtime,
};
use crate::OrchestratorError;
use crate::internal::{internal, internal_contract};
use crate::source_prep::cache_source_projection::{
    EntryKind, is_git_path, sha256_hex, validate_link_target, validate_link_target_in_root,
    validate_relative_path,
};

pub fn digest_record<T: Serialize + ?Sized>(value: &T) -> Result<Digest, OrchestratorError> {
    let bytes = canonical_json_bytes(value).map_err(internal_contract)?;
    Ok(digest_b3_typed(&bytes))
}

#[must_use]
pub fn is_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    excluded
        .iter()
        .any(|root| path == root || path.starts_with(root))
}

fn file_state_mtime(path: &Path) -> Result<StateMtime, OrchestratorError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    let modified = metadata
        .modified()
        .map_err(|_| internal("snapshot_mtime_unavailable"))?;
    let elapsed = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| internal("snapshot_mtime_range"))?;
    let seconds = i64::try_from(elapsed.as_secs()).map_err(|_| internal("snapshot_mtime_range"))?;
    let mtime = StateMtime {
        seconds,
        nanoseconds: elapsed.subsec_nanos(),
    };
    mtime.validate()?;
    Ok(mtime)
}

#[cfg(unix)]
fn entry_mode(path: &Path) -> Result<u32, OrchestratorError> {
    use std::os::unix::fs::PermissionsExt;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| OrchestratorError::io(path.display().to_string(), error.to_string()))?;
    Ok(metadata.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn entry_mode(path: &Path) -> Result<u32, OrchestratorError> {
    let _ = path;
    Ok(0o644)
}

fn index_one_file(
    absolute: &Path,
    archive_path: &str,
    entries: &mut Vec<StateEntryV1>,
    payload_bytes: &mut u64,
) -> Result<(), OrchestratorError> {
    let bytes = fs::read(absolute).map_err(|error| {
        OrchestratorError::io(absolute.display().to_string(), error.to_string())
    })?;
    let size = u64::try_from(bytes.len()).map_err(|_| internal("snapshot_size_range"))?;
    *payload_bytes = payload_bytes
        .checked_add(size)
        .ok_or_else(|| internal("snapshot_size_overflow"))?;
    if *payload_bytes > MAX_SNAPSHOT_PAYLOAD_BYTES {
        return Err(internal("snapshot_payload_limit"));
    }
    entries.push(StateEntryV1 {
        path: archive_path.to_owned(),
        kind: EntryKind::File.as_str().to_owned(),
        mode: entry_mode(absolute)?,
        size,
        content_sha256: sha256_hex(&bytes),
        link_target: None,
        mtime: file_state_mtime(absolute)?,
    });
    Ok(())
}

fn index_one_link(
    lane_root: &Path,
    archive_path: &str,
    absolute: &Path,
    link_text: &str,
    entries: &mut Vec<StateEntryV1>,
) -> Result<(), OrchestratorError> {
    let relative = archive_path
        .strip_prefix(&format!("{STATE_ARCHIVE_PREFIX}/"))
        .unwrap_or(archive_path);
    let lane_relative = relative.split_once('/').map_or(relative, |(_, rest)| rest);
    validate_link_target_in_root(lane_root, lane_relative, link_text)?;
    entries.push(StateEntryV1 {
        path: archive_path.to_owned(),
        kind: EntryKind::Symlink.as_str().to_owned(),
        mode: entry_mode(absolute)?,
        size: u64::try_from(link_text.len()).unwrap_or(u64::MAX),
        content_sha256: sha256_hex(link_text.as_bytes()),
        link_target: Some(link_text.to_owned()),
        mtime: file_state_mtime(absolute)?,
    });
    Ok(())
}

fn collect_tree(
    lane: &LaneRoot,
    excluded: &[PathBuf],
    directory: &Path,
    entries: &mut Vec<StateEntryV1>,
    payload_bytes: &mut u64,
) -> Result<(), OrchestratorError> {
    let mut children = fs::read_dir(directory)
        .map_err(|error| OrchestratorError::io(directory.display().to_string(), error.to_string()))?
        .map(|child| {
            child.map(|child| child.path()).map_err(|error| {
                OrchestratorError::io(directory.display().to_string(), error.to_string())
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for child in children {
        if is_excluded(&child, excluded) {
            continue;
        }
        let relative = child
            .strip_prefix(&lane.root)
            .map_err(|_| internal("snapshot_lane_escape"))?;
        let text = relative
            .to_str()
            .ok_or_else(|| internal("snapshot_path_not_utf8"))?;
        if text.contains('\0') {
            return Err(internal("snapshot_path_invalid"));
        }
        let archive_path = format!("{STATE_ARCHIVE_PREFIX}/{}/{}", lane.lane_id, text);
        validate_relative_path(&archive_path)?;
        if is_git_path(text) || is_git_path(&archive_path) {
            return Err(internal("snapshot_git_path"));
        }
        let metadata = fs::symlink_metadata(&child).map_err(|error| {
            OrchestratorError::io(child.display().to_string(), error.to_string())
        })?;
        let file_type = metadata.file_type();
        if file_type.is_file() {
            index_one_file(&child, &archive_path, entries, payload_bytes)?;
        } else if file_type.is_dir() {
            entries.push(StateEntryV1 {
                path: archive_path.clone(),
                kind: EntryKind::Directory.as_str().to_owned(),
                mode: entry_mode(&child)?,
                size: 0,
                content_sha256: sha256_hex(&[]),
                link_target: None,
                mtime: file_state_mtime(&child)?,
            });
            collect_tree(lane, excluded, &child, entries, payload_bytes)?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(&child).map_err(|error| {
                OrchestratorError::io(child.display().to_string(), error.to_string())
            })?;
            let link_text = target
                .to_str()
                .ok_or_else(|| internal("snapshot_link_not_utf8"))?;
            validate_link_target(link_text)?;
            index_one_link(&lane.root, &archive_path, &child, link_text, entries)?;
        } else {
            return Err(internal("snapshot_special_file"));
        }
        if entries.len() > MAX_SNAPSHOT_ENTRIES {
            return Err(internal("snapshot_entry_limit"));
        }
    }
    Ok(())
}

pub fn collect_state_index(
    lanes: &[LaneRoot],
    excluded: &[PathBuf],
) -> Result<Vec<StateEntryV1>, OrchestratorError> {
    for lane in lanes {
        lane.validate()?;
    }
    for root in excluded {
        if !root.is_absolute() {
            return Err(internal("snapshot_excluded_root_not_absolute"));
        }
    }
    let mut seen_ids = BTreeSet::new();
    for lane in lanes {
        if !seen_ids.insert(lane.lane_id.clone()) {
            return Err(internal("snapshot_lane_duplicate"));
        }
        for other in lanes {
            if lane.lane_id != other.lane_id
                && (lane.root == other.root
                    || lane.root.starts_with(&other.root)
                    || other.root.starts_with(&lane.root))
            {
                return Err(internal("snapshot_lane_overlap"));
            }
        }
        if is_excluded(&lane.root, excluded) {
            return Err(internal("snapshot_lane_excluded"));
        }
        if !lane.root.is_dir() {
            return Err(internal("snapshot_lane_not_directory"));
        }
    }
    let mut entries = Vec::new();
    let mut payload_bytes = 0_u64;
    for lane in lanes {
        collect_tree(lane, excluded, &lane.root, &mut entries, &mut payload_bytes)?;
    }
    validate_state_index(&entries)?;
    Ok(entries)
}
