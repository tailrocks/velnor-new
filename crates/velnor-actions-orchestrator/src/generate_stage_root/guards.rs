// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::path::Path;

use crate::OrchestratorError;
use crate::generation_filesystem::{FilesystemMetadata, FilesystemObjectType, GenerationDirectory};

use super::root::{FsIdentity, RootDirectory};
use super::{PRIVATE_MODE, PRIVATE_STAGE_PREFIX, TARGET};

pub(super) struct PrivateStageOwnership<'root> {
    root: &'root RootDirectory,
    name: String,
    identity: FsIdentity,
    active: bool,
}

impl<'root> PrivateStageOwnership<'root> {
    fn new(root: &'root RootDirectory, name: String, identity: FsIdentity) -> Self {
        Self {
            root,
            name,
            identity,
            active: true,
        }
    }

    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn identity(&self) -> FsIdentity {
        self.identity
    }

    pub(super) fn abort(&mut self, cause: OrchestratorError) -> OrchestratorError {
        match self.cleanup() {
            Ok(()) => cause,
            Err(cleanup) => OrchestratorError::io(
                self.root.directory.path().join(&self.name).display().to_string(),
                format!("generation_aborted:{cause}; staging_cleanup_failed:{cleanup}"),
            ),
        }
    }

    pub(super) fn disarm(&mut self) {
        self.active = false;
    }

    fn cleanup(&mut self) -> Result<(), OrchestratorError> {
        super::cleanup::remove_tree(
            self.root,
            self.root.directory.as_ref(),
            OsStr::new(&self.name),
            self.identity,
        )?;
        self.active = false;
        Ok(())
    }
}

impl Drop for PrivateStageOwnership<'_> {
    fn drop(&mut self) {
        if self.active {
            let _cleanup = self.cleanup();
        }
    }
}

pub(super) fn create_private_stage(
    root: &RootDirectory,
) -> Result<PrivateStageOwnership<'_>, OrchestratorError> {
    for _ in 0..32 {
        let mut random = [0u8; 16];
        getrandom::fill(&mut random).map_err(|error| {
            OrchestratorError::io(
                root.directory.path().display().to_string(),
                error.to_string(),
            )
        })?;
        let name = format!("{PRIVATE_STAGE_PREFIX}{}", hex(&random));
        let created = match root
            .directory
            .create_dir_raw(OsStr::new(&name), PRIVATE_MODE)
        {
            Ok(identity) => Some(identity),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => None,
            Err(error) => {
                return Err(io(root.directory.path(), &error));
            }
        };
        if let Some(created) = created {
            return Ok(PrivateStageOwnership::new(
                root,
                name,
                FsIdentity {
                    device: created.device,
                    inode: created.inode,
                },
            ));
        }
    }
    Err(OrchestratorError::io(
        root.directory.path().display().to_string(),
        "private_staging_name_exhausted",
    ))
}

pub(super) fn validate_target(
    root: &RootDirectory,
) -> Result<(Option<FsIdentity>, Option<u32>), OrchestratorError> {
    let name = OsStr::new(TARGET);
    let Some(metadata) = root.directory.stat_opt(name)? else {
        return Ok((None, None));
    };
    let path = root.directory.path().join(TARGET);
    if metadata.object_type != FilesystemObjectType::Directory {
        return Err(unsafe_path(
            &path,
            if metadata.object_type == FilesystemObjectType::Symlink {
                "symlink_refused"
            } else {
                "not_a_directory"
            },
        ));
    }
    let directory = root.directory.open_dir(name)?;
    let opened = directory.metadata()?;
    let identity = FsIdentity::from_metadata(&metadata);
    if FsIdentity::from_metadata(&opened) != identity {
        return Err(unsafe_path(&path, "target_identity_changed"));
    }
    validate_owned_directory(root, directory.as_ref(), &opened)?;
    validate_tree_owners(root, directory.as_ref())?;
    Ok((Some(identity), Some(metadata.mode)))
}

pub(super) fn validate_owned_directory(
    root: &RootDirectory,
    directory: &dyn GenerationDirectory,
    metadata: &FilesystemMetadata,
) -> Result<(), OrchestratorError> {
    if metadata.object_type != FilesystemObjectType::Directory {
        return Err(unsafe_path(directory.path(), "not_a_directory"));
    }
    if metadata.owner != root.owner {
        return Err(unsafe_path(directory.path(), "staging_owner_mismatch"));
    }
    root.verify_mount(directory.path(), directory)
}

pub(super) fn validate_private_directory(
    root: &RootDirectory,
    directory: &dyn GenerationDirectory,
    metadata: &FilesystemMetadata,
    expected_mode: u32,
    reason: &str,
) -> Result<(), OrchestratorError> {
    validate_owned_directory(root, directory, metadata)?;
    if metadata.mode != expected_mode {
        return Err(unsafe_path(directory.path(), reason));
    }
    Ok(())
}

pub(super) fn validate_tree_owners(
    root: &RootDirectory,
    directory: &dyn GenerationDirectory,
) -> Result<(), OrchestratorError> {
    let metadata = directory.metadata()?;
    validate_owned_directory(root, directory, &metadata)?;
    for name in directory.entries()? {
        let child_metadata = directory.stat(&name)?;
        if child_metadata.object_type != FilesystemObjectType::Directory {
            continue;
        }
        let child = directory.open_dir(&name)?;
        let opened = child.metadata()?;
        if FsIdentity::from_metadata(&opened) != FsIdentity::from_metadata(&child_metadata) {
            return Err(unsafe_path(child.path(), "directory_identity_changed"));
        }
        validate_tree_owners(root, child.as_ref())?;
    }
    Ok(())
}

pub(super) fn same_device(first: FsIdentity, second: FsIdentity) -> bool {
    first.device == second.device
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

pub(super) fn io(path: &Path, error: &impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

pub(super) fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
