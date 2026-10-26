// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;

use flate2::read::GzDecoder;
use tar::EntryType;

use super::pack::io_error;
use super::types::{MAX_ARCHIVE_BYTES, MAX_ARCHIVE_ENTRIES, MAX_ARCHIVE_ENTRY_BYTES, MAX_ARCHIVE_TOTAL_BYTES};
use crate::OrchestratorError;
use crate::cache_snapshot::{
    MANIFEST_ARCHIVE_MEMBER, PROJECTION_ARCHIVE_PREFIX, PROJECTION_INDEX_ARCHIVE_MEMBER,
    REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER, STATE_ARCHIVE_PREFIX, STATE_INDEX_ARCHIVE_MEMBER,
};
use crate::internal::internal;
use crate::source_prep::cache_source_projection::{
    is_git_path, reject_link_parents, sha256_hex, validate_link_target,
    validate_link_target_in_root, validate_relative_path,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum RawKind {
    File,
    Directory,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RawMember {
    pub(super) name: String,
    pub(super) kind: RawKind,
    pub(super) size: u64,
    pub(super) hash: String,
    pub(super) link_target: Option<String>,
}

fn archive_member_allowed(name: &str) -> bool {
    name == MANIFEST_ARCHIVE_MEMBER
        || name == STATE_INDEX_ARCHIVE_MEMBER
        || name == PROJECTION_INDEX_ARCHIVE_MEMBER
        || name == REQUIRED_OUTPUT_INDEX_ARCHIVE_MEMBER
        || name
            .strip_prefix(STATE_ARCHIVE_PREFIX)
            .is_some_and(|rest| rest.starts_with('/'))
        || name
            .strip_prefix(PROJECTION_ARCHIVE_PREFIX)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn normalize_member_name(raw: &[u8]) -> Result<(String, bool), OrchestratorError> {
    let text = std::str::from_utf8(raw).map_err(|_| internal("snapshot_archive_non_utf8_path"))?;
    let directory_marker = text.ends_with('/') && text.len() > 1;
    let name = if directory_marker {
        text.trim_end_matches('/').to_owned()
    } else {
        text.to_owned()
    };
    validate_relative_path(&name).map_err(|_| internal("snapshot_archive_unsafe_path"))?;
    if is_git_path(&name) {
        return Err(internal("snapshot_git_path"));
    }
    if !archive_member_allowed(&name) {
        return Err(internal("snapshot_archive_prefix"));
    }
    Ok((name, directory_marker))
}

fn ensure_member_parent(destination: &Path, name: &str) -> Result<(), OrchestratorError> {
    reject_link_parents(destination, name).map_err(|_| internal("snapshot_archive_parent_link"))?;
    if let Some(parent) = Path::new(name)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(destination.join(parent))
            .map_err(|error| io_error(&destination.join(parent), error))?;
        reject_link_parents(destination, name)
            .map_err(|_| internal("snapshot_archive_parent_link"))?;
    }
    Ok(())
}

fn write_member_file(
    destination: &Path,
    name: &str,
    reader: &mut impl Read,
    size: u64,
    total: &mut u64,
) -> Result<String, OrchestratorError> {
    ensure_member_parent(destination, name)?;
    let output = destination.join(name);
    if fs::symlink_metadata(&output).is_ok() {
        return Err(OrchestratorError::OverwriteRefused {
            path: output.display().to_string(),
        });
    }
    let mut file = File::create_new(&output).map_err(|error| io_error(&output, error))?;
    let mut remaining = size;
    let mut buffer = vec![0_u8; 64 * 1024];
    while remaining > 0 {
        let take = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| internal("snapshot_size_range"))?;
        let read = reader
            .read(&mut buffer[..take])
            .map_err(|error| io_error(&output, error))?;
        if read == 0 {
            return Err(internal("snapshot_archive_truncated_entry"));
        }
        file.write_all(&buffer[..read])
            .map_err(|error| io_error(&output, error))?;
        remaining -= read as u64;
    }
    file.flush().map_err(|error| io_error(&output, error))?;
    drop(file);
    *total = total
        .checked_add(size)
        .ok_or_else(|| internal("snapshot_size_overflow"))?;
    if *total > MAX_ARCHIVE_TOTAL_BYTES {
        return Err(internal("snapshot_archive_total_limit"));
    }
    let bytes = fs::read(&output).map_err(|error| io_error(&output, error))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) != size {
        return Err(internal("snapshot_archive_size_mismatch"));
    }
    Ok(sha256_hex(&bytes))
}

fn create_member_directory(destination: &Path, name: &str) -> Result<(), OrchestratorError> {
    reject_link_parents(destination, name).map_err(|_| internal("snapshot_archive_parent_link"))?;
    let output = destination.join(name);
    match fs::symlink_metadata(&output) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => Ok(()),
        Ok(_) => Err(OrchestratorError::OverwriteRefused {
            path: output.display().to_string(),
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&output).map_err(|error| io_error(&output, error))?;
            let created =
                fs::symlink_metadata(&output).map_err(|error| io_error(&output, error))?;
            if created.file_type().is_symlink() || !created.is_dir() {
                return Err(internal("snapshot_archive_output_directory"));
            }
            Ok(())
        }
        Err(error) => Err(io_error(&output, error)),
    }
}

fn create_member_link(
    destination: &Path,
    name: &str,
    target: &str,
) -> Result<(), OrchestratorError> {
    ensure_member_parent(destination, name)?;
    let output = destination.join(name);
    if fs::symlink_metadata(&output).is_ok() {
        return Err(OrchestratorError::OverwriteRefused {
            path: output.display().to_string(),
        });
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, &output).map_err(|error| io_error(&output, error))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = (target, output);
        Err(OrchestratorError::unsupported(
            "snapshot_archive_symlink",
            "platform_has_no_posix_symlink_projection",
        ))
    }
}

fn ingest_typed_member(
    destination: &Path,
    name: String,
    directory_marker: bool,
    entry: &mut tar::Entry<'_, GzDecoder<File>>,
    size: u64,
    total: &mut u64,
) -> Result<RawMember, OrchestratorError> {
    match entry.header().entry_type() {
        EntryType::Regular | EntryType::Continuous => {
            if directory_marker {
                return Err(internal("snapshot_archive_file_directory_name"));
            }
            let hash = write_member_file(destination, &name, entry, size, total)?;
            Ok(RawMember {
                name,
                kind: RawKind::File,
                size,
                hash,
                link_target: None,
            })
        }
        EntryType::Directory => {
            create_member_directory(destination, &name)?;
            Ok(RawMember {
                name,
                kind: RawKind::Directory,
                size: 0,
                hash: sha256_hex(&[]),
                link_target: None,
            })
        }
        EntryType::Symlink => {
            if directory_marker {
                return Err(internal("snapshot_archive_file_directory_name"));
            }
            let raw_target = entry
                .link_name_bytes()
                .ok_or_else(|| internal("snapshot_archive_link_target"))?;
            let target = std::str::from_utf8(raw_target.as_ref())
                .map_err(|_| internal("snapshot_archive_non_utf8_link"))?;
            validate_link_target(target).map_err(|_| internal("snapshot_archive_link_target"))?;
            validate_link_target_in_root(destination, &name, target)
                .map_err(|_| internal("snapshot_archive_link_escape"))?;
            create_member_link(destination, &name, target)?;
            Ok(RawMember {
                name,
                kind: RawKind::Symlink,
                size: 0,
                hash: sha256_hex(target.as_bytes()),
                link_target: Some(target.to_owned()),
            })
        }
        _ => Err(internal("snapshot_archive_link_or_special")),
    }
}

pub(super) fn scan_archive_members(
    archive: &Path,
    destination: &Path,
) -> Result<Vec<RawMember>, OrchestratorError> {
    let metadata = fs::symlink_metadata(archive).map_err(|error| io_error(archive, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(internal("snapshot_archive_not_regular"));
    }
    if metadata.len() > MAX_ARCHIVE_BYTES {
        return Err(internal("snapshot_archive_input_limit"));
    }
    let file = File::open(archive).map_err(|error| io_error(archive, error))?;
    let decoder = GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let mut members = Vec::new();
    let mut seen = HashSet::new();
    let mut total = 0_u64;
    let entries = tar
        .entries()
        .map_err(|error| internal(&format!("snapshot_archive_entries:{error}")))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|error| internal(&format!("snapshot_archive_entry:{error}")))?;
        if members.len() >= MAX_ARCHIVE_ENTRIES {
            return Err(internal("snapshot_archive_entry_limit"));
        }
        let (name, directory_marker) = normalize_member_name(entry.path_bytes().as_ref())?;
        if !seen.insert(name.clone()) {
            return Err(internal("snapshot_archive_duplicate_path"));
        }
        let size = entry.size();
        if size > MAX_ARCHIVE_ENTRY_BYTES {
            return Err(internal("snapshot_archive_entry_size_limit"));
        }
        members.push(ingest_typed_member(
            destination,
            name,
            directory_marker,
            &mut entry,
            size,
            &mut total,
        )?);
    }
    let tar = tar.into_inner();
    let mut trailing = Vec::new();
    tar.take(8 * 512 + 1)
        .read_to_end(&mut trailing)
        .map_err(|error| internal(&format!("snapshot_archive_trailing:{error}")))?;
    if trailing.len() > 8 * 512 || trailing.iter().any(|byte| *byte != 0) {
        return Err(internal("snapshot_archive_trailing_bytes"));
    }
    Ok(members)
}
