// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::Arc;

use crate::OrchestratorError;
use crate::filesystem_owner::effective_uid;
use crate::generation_filesystem::{
    CreatedDirectoryIdentity, FilesystemMetadata, FilesystemObjectType, GenerationDirectory,
    GenerationFilesystem, MountIdentity,
};

use super::super::fd::Directory;
use super::guards::unsafe_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::generate) struct FsIdentity {
    pub(super) device: u64,
    pub(super) inode: u64,
}

impl FsIdentity {
    pub(in crate::generate) fn from_metadata(metadata: &FilesystemMetadata) -> Self {
        Self {
            device: metadata.device,
            inode: metadata.inode,
        }
    }

    pub(in crate::generate) fn from_created_directory(
        identity: CreatedDirectoryIdentity,
    ) -> Self {
        Self {
            device: identity.device,
            inode: identity.inode,
        }
    }
}

#[derive(Debug)]
pub(in crate::generate) struct RootDirectory {
    pub(super) directory: Directory,
    pub(super) filesystem: Arc<dyn GenerationFilesystem>,
    pub(super) identity: FsIdentity,
    pub(super) mount: MountIdentity,
    pub(super) owner: u32,
}

impl RootDirectory {
    pub(in crate::generate) fn open(
        path: &Path,
        filesystem: Arc<dyn GenerationFilesystem>,
    ) -> Result<Self, OrchestratorError> {
        let directory = filesystem.open_root(path)?;
        let metadata = directory.metadata()?;
        if metadata.object_type != FilesystemObjectType::Directory {
            return Err(unsafe_path(path, "repository_root_not_directory"));
        }
        Ok(Self {
            identity: FsIdentity::from_metadata(&metadata),
            mount: metadata.mount,
            directory,
            filesystem,
            owner: effective_uid(),
        })
    }

    pub(in crate::generate) fn directory(&self) -> &Directory {
        &self.directory
    }

    pub(in crate::generate) fn owner(&self) -> u32 {
        self.owner
    }

    pub(in crate::generate) fn verify_mount(
        &self,
        path: &Path,
        directory: &dyn GenerationDirectory,
    ) -> Result<(), OrchestratorError> {
        if directory.metadata()?.mount != self.mount {
            return Err(unsafe_path(path, "cross_mount_boundary"));
        }
        Ok(())
    }

    pub(in crate::generate) fn verify_location(&self) -> Result<(), OrchestratorError> {
        let current = self.filesystem.open_root(self.directory.path())?;
        let metadata = current.metadata()?;
        if FsIdentity::from_metadata(&metadata) != self.identity {
            return Err(unsafe_path(
                self.directory.path(),
                "repository_root_identity_changed",
            ));
        }
        Ok(())
    }
}
