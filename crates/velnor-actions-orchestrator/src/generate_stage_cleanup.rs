// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::Path;

use crate::OrchestratorError;
use crate::generation_filesystem::{FilesystemMetadata, FilesystemObjectType, GenerationDirectory};

use super::{FsIdentity, RootDirectory};

const CLEANUP_MODE: u32 = 0o700;

pub(super) fn remove_tree(
    root: &RootDirectory,
    parent: &dyn GenerationDirectory,
    name: &OsStr,
    expected: FsIdentity,
) -> Result<(), OrchestratorError> {
    let before = parent.stat(name)?;
    if FsIdentity::from_metadata(&before) != expected
        || before.object_type != FilesystemObjectType::Directory
    {
        return Err(unsafe_path(
            &parent.path().join(name),
            "cleanup_root_identity_changed",
        ));
    }
    let directory = parent.open_dir(name)?;
    let opened = directory.metadata()?;
    if FsIdentity::from_metadata(&opened) != expected {
        return Err(unsafe_path(
            directory.path(),
            "cleanup_root_identity_changed",
        ));
    }
    validate_directory(root, directory.as_ref(), &opened)?;
    remove_children(root, directory.as_ref())?;
    let after = parent.stat(name)?;
    if FsIdentity::from_metadata(&after) != expected {
        return Err(unsafe_path(
            &parent.path().join(name),
            "cleanup_root_identity_changed",
        ));
    }
    parent.remove_dir(name)
}

pub(super) fn remove_children(
    root: &RootDirectory,
    directory: &dyn GenerationDirectory,
) -> Result<(), OrchestratorError> {
    validate_directory(root, directory, &directory.metadata()?)?;
    directory.set_mode(CLEANUP_MODE)?;
    for name in directory.entries()? {
        let metadata = directory.stat(&name)?;
        let path = directory.path().join(&name);
        if metadata.object_type == FilesystemObjectType::Directory {
            let child = directory.open_dir(&name)?;
            let opened = child.metadata()?;
            let expected = FsIdentity::from_metadata(&metadata);
            if FsIdentity::from_metadata(&opened) != expected {
                return Err(unsafe_path(&path, "cleanup_entry_identity_changed"));
            }
            validate_directory(root, child.as_ref(), &opened)?;
            remove_children(root, child.as_ref())?;
            let current = directory.stat(&name)?;
            if FsIdentity::from_metadata(&current) != expected {
                return Err(unsafe_path(&path, "cleanup_entry_identity_changed"));
            }
            directory.remove_dir(&name)?;
        } else {
            let current = directory.stat(&name)?;
            if FsIdentity::from_metadata(&current) != FsIdentity::from_metadata(&metadata) {
                return Err(unsafe_path(&path, "cleanup_entry_identity_changed"));
            }
            directory.remove_file(&name)?;
        }
    }
    Ok(())
}

fn validate_directory(
    root: &RootDirectory,
    directory: &dyn GenerationDirectory,
    metadata: &FilesystemMetadata,
) -> Result<(), OrchestratorError> {
    if metadata.object_type != FilesystemObjectType::Directory {
        return Err(unsafe_path(directory.path(), "cleanup_root_not_directory"));
    }
    if metadata.owner != root.owner() {
        return Err(unsafe_path(directory.path(), "foreign_directory_owner"));
    }
    root.verify_mount(directory.path(), directory)
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
