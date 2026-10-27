// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use super::types::{
    CheckoutState, EntryKind, InstallOutcome, ProjectionEntry, RealMtime, RestoreDecision,
};
use super::validate::{
    is_git_path, reject_link_parents, sha256_hex, staged_path, validate_link_target_in_root,
};
use crate::OrchestratorError;
use crate::internal::internal;

pub fn authenticate_entry(
    root: &Path,
    declared: &ProjectionEntry,
) -> Result<CheckoutState, OrchestratorError> {
    declared.validate()?;
    reject_link_parents(root, &declared.path)?;
    if declared.kind == EntryKind::Symlink {
        let Some(expected_target) = declared.link_target.as_deref() else {
            return Err(internal("projection_link_missing"));
        };
        validate_link_target_in_root(root, &declared.path, expected_target)?;
    }
    let current = staged_path(root, &declared.path)?;
    let metadata = match fs::symlink_metadata(&current) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CheckoutState::Missing);
        }
        Err(_) => return Err(internal("projection_checkout_unreadable")),
    };
    let file_type = metadata.file_type();
    let kind_matches = match declared.kind {
        EntryKind::File => file_type.is_file(),
        EntryKind::Directory => file_type.is_dir(),
        EntryKind::Symlink => file_type.is_symlink(),
    };
    if !kind_matches {
        return Ok(CheckoutState::Changed);
    }
    if declared.kind != EntryKind::Symlink
        && metadata.permissions().mode() & 0o7777 != declared.mode
    {
        return Ok(CheckoutState::Changed);
    }
    match declared.kind {
        EntryKind::File => {
            let bytes =
                fs::read(&current).map_err(|_| internal("projection_checkout_unreadable"))?;
            if sha256_hex(&bytes) == declared.content_sha256 {
                Ok(CheckoutState::Unchanged)
            } else {
                Ok(CheckoutState::Changed)
            }
        }
        EntryKind::Directory => Ok(CheckoutState::Unchanged),
        EntryKind::Symlink => {
            let target =
                fs::read_link(&current).map_err(|_| internal("projection_checkout_unreadable"))?;
            let text = target.to_string_lossy();
            if validate_link_target_in_root(root, &declared.path, &text).is_err() {
                return Ok(CheckoutState::Changed);
            }
            if Some(text.as_ref()) == declared.link_target.as_deref()
                && sha256_hex(text.as_bytes()) == declared.content_sha256
            {
                Ok(CheckoutState::Unchanged)
            } else {
                Ok(CheckoutState::Changed)
            }
        }
    }
}

#[must_use]
pub fn decide(
    staged: Option<&ProjectionEntry>,
    declared: Option<&ProjectionEntry>,
    state: CheckoutState,
) -> RestoreDecision {
    let Some(staged) = staged else {
        return RestoreDecision::KeepCurrent;
    };
    if is_git_path(&staged.path) {
        return RestoreDecision::KeepCurrent;
    }
    let Some(declared) = declared else {
        return RestoreDecision::Discard;
    };
    if !staged.semantic_matches(declared) {
        return RestoreDecision::Discard;
    }
    match state {
        CheckoutState::Unchanged => RestoreDecision::Restore,
        CheckoutState::Changed | CheckoutState::Missing => RestoreDecision::KeepCurrent,
    }
}

pub fn reconcile(
    staged: &ProjectionEntry,
    declared: &ProjectionEntry,
) -> Result<RestoreDecision, OrchestratorError> {
    staged.validate()?;
    declared.validate()?;
    if staged.path != declared.path {
        return Err(internal("projection_identity_mismatch"));
    }
    if !staged.semantic_matches(declared) {
        return Err(internal("projection_archive_mismatch"));
    }
    if staged.mtime.is_none() {
        return Err(internal("projection_mtime_missing"));
    }
    Ok(RestoreDecision::Restore)
}

pub fn restore_times(path: &Path, mtime: &RealMtime) -> Result<(), OrchestratorError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| internal("projection_restore_missing"))?;
    if metadata.file_type().is_symlink() {
        return Err(internal("projection_restore_link"));
    }
    let modified = mtime.system_time()?;
    let file = File::open(path).map_err(|_| internal("projection_restore_missing"))?;
    file.set_times(FileTimes::new().set_modified(modified))
        .map_err(|_| internal("projection_restore_times"))?;
    verify_times(path, mtime)
}

pub fn verify_times(path: &Path, mtime: &RealMtime) -> Result<(), OrchestratorError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| internal("projection_verify_missing"))?;
    if metadata.file_type().is_symlink() {
        return Err(internal("projection_verify_link"));
    }
    let actual = metadata
        .modified()
        .map_err(|_| internal("projection_verify_times"))?;
    let expected = mtime.system_time()?;
    if actual != expected {
        return Err(internal("projection_mtime_roundtrip"));
    }
    Ok(())
}

pub fn sort_deepest_first(paths: &mut [PathBuf]) {
    paths.sort_by(|first, second| {
        second
            .components()
            .count()
            .cmp(&first.components().count())
            .then_with(|| second.cmp(first))
    });
}

pub fn restore_directory_times(entries: &[(PathBuf, RealMtime)]) -> Result<(), OrchestratorError> {
    let mut ordered: Vec<(PathBuf, RealMtime)> = entries.to_vec();
    ordered.sort_by(|first, second| {
        second
            .0
            .components()
            .count()
            .cmp(&first.0.components().count())
            .then_with(|| second.0.cmp(&first.0))
    });
    for (path, mtime) in &ordered {
        restore_times(path, mtime)?;
    }
    Ok(())
}

pub fn install_staged_file(
    staged: &Path,
    destination: &Path,
    expected: &ProjectionEntry,
) -> Result<InstallOutcome, OrchestratorError> {
    expected.validate()?;
    if expected.kind != EntryKind::File {
        return Err(internal("projection_install_kind"));
    }
    let Some(mtime) = &expected.mtime else {
        return Err(internal("projection_mtime_missing"));
    };
    let staged_meta =
        fs::symlink_metadata(staged).map_err(|_| internal("projection_staged_missing"))?;
    if !staged_meta.file_type().is_file() {
        return Err(internal("projection_staged_kind"));
    }
    let bytes = fs::read(staged).map_err(|_| internal("projection_staged_unreadable"))?;
    if sha256_hex(&bytes) != expected.content_sha256 {
        return Err(internal("projection_staged_mismatch"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|_| internal("projection_destination_parent"))?;
    }
    fs::rename(staged, destination).map_err(|_| internal("projection_install_move"))?;
    fs::set_permissions(destination, fs::Permissions::from_mode(expected.mode))
        .map_err(|_| internal("projection_install_mode"))?;
    restore_times(destination, mtime)?;
    Ok(InstallOutcome {
        mtime_restored: true,
    })
}

pub fn install_staged_symlink(
    staged: &Path,
    root: &Path,
    expected: &ProjectionEntry,
) -> Result<InstallOutcome, OrchestratorError> {
    expected.validate()?;
    if expected.kind != EntryKind::Symlink {
        return Err(internal("projection_install_kind"));
    }
    let Some(target) = expected.link_target.as_deref() else {
        return Err(internal("projection_link_missing"));
    };
    validate_link_target_in_root(root, &expected.path, target)?;
    let staged_meta =
        fs::symlink_metadata(staged).map_err(|_| internal("projection_staged_missing"))?;
    if !staged_meta.file_type().is_symlink() {
        return Err(internal("projection_staged_kind"));
    }
    let staged_target =
        fs::read_link(staged).map_err(|_| internal("projection_staged_unreadable"))?;
    if staged_target.to_string_lossy() != target {
        return Err(internal("projection_staged_mismatch"));
    }
    let destination = staged_path(root, &expected.path)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|_| internal("projection_destination_parent"))?;
    }
    match fs::symlink_metadata(&destination) {
        Ok(_) => {
            fs::remove_file(&destination).map_err(|_| internal("projection_install_replace"))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(internal("projection_destination_unreadable")),
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, &destination)
        .map_err(|_| internal("projection_install_link"))?;
    #[cfg(not(unix))]
    {
        let _ = (target, &destination);
        return Err(internal("projection_install_platform"));
    }
    #[cfg(unix)]
    Ok(InstallOutcome {
        mtime_restored: false,
    })
}

#[must_use]
pub fn checkout_metadata_matches(metadata: &fs::Metadata, kind: EntryKind, mode: u32) -> bool {
    let file_type = metadata.file_type();
    let kind_matches = match kind {
        EntryKind::File => file_type.is_file(),
        EntryKind::Directory => file_type.is_dir(),
        EntryKind::Symlink => file_type.is_symlink(),
    };
    kind_matches && metadata.permissions().mode() & 0o7777 == mode
}
