// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::OrchestratorError;

mod host;
pub use host::HostGenerationFilesystem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesystemObjectType {
    Directory,
    RegularFile,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountIdentity {
    Linux {
        mount_id: u64,
    },
    Macos {
        mount_point: Vec<u8>,
        filesystem_id: u64,
    },
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemMetadata {
    pub device: u64,
    pub inode: u64,
    pub owner: u32,
    pub mode: u32,
    pub link_count: u64,
    pub object_type: FilesystemObjectType,
    pub mount: MountIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreatedDirectoryIdentity {
    pub device: u64,
    pub inode: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationBackendId(Arc<()>);

impl GenerationBackendId {
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(()))
    }

    #[must_use]
    pub fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Default for GenerationBackendId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationFileOpen {
    ReadOnly,
    WriteCreateNew,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationRename {
    NoReplace,
    Exchange,
}

pub trait GenerationFilesystem: std::fmt::Debug + Send + Sync {
    fn open_root(&self, path: &Path) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError>;
}

pub trait GenerationDirectory: std::fmt::Debug + Send + Sync {
    fn path(&self) -> &Path;
    fn child_path(&self, name: &OsStr) -> PathBuf {
        self.path().join(name)
    }
    fn backend_id(&self) -> &GenerationBackendId;
    fn handle_id(&self) -> u64;
    fn metadata(&self) -> Result<FilesystemMetadata, OrchestratorError>;
    fn stat(&self, name: &OsStr) -> Result<FilesystemMetadata, OrchestratorError>;
    fn stat_opt(&self, name: &OsStr) -> Result<Option<FilesystemMetadata>, OrchestratorError>;
    fn try_clone(&self) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError>;
    fn open_dir(&self, name: &OsStr) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError>;
    fn open_file(
        &self,
        name: &OsStr,
        access: GenerationFileOpen,
    ) -> Result<Box<dyn GenerationFile>, OrchestratorError>;
    fn create_dir(&self, name: &OsStr, mode: u32) -> Result<(), OrchestratorError>;
    fn create_dir_raw(&self, name: &OsStr, mode: u32) -> std::io::Result<CreatedDirectoryIdentity>;
    fn set_mode(&self, mode: u32) -> Result<(), OrchestratorError>;
    fn entries(&self) -> Result<Vec<OsString>, OrchestratorError>;
    fn read_link(&self, name: &OsStr) -> Result<OsString, OrchestratorError>;
    fn symlink(&self, target: &OsStr, name: &OsStr) -> Result<(), OrchestratorError>;
    fn remove_file(&self, name: &OsStr) -> Result<(), OrchestratorError>;
    fn remove_dir(&self, name: &OsStr) -> Result<(), OrchestratorError>;
    fn rename_to(
        &self,
        name: &OsStr,
        destination: &dyn GenerationDirectory,
        destination_name: &OsStr,
        mode: GenerationRename,
    ) -> std::io::Result<()>;
}

pub trait GenerationFile: Read + Write + Seek + std::fmt::Debug + Send {
    fn metadata(&self) -> Result<FilesystemMetadata, OrchestratorError>;
    fn set_mode(&self, mode: u32) -> Result<(), OrchestratorError>;
}

pub(crate) fn same_object(first: &FilesystemMetadata, second: &FilesystemMetadata) -> bool {
    first.device == second.device && first.inode == second.inode
}

pub(crate) fn io(path: &Path, error: &impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

pub(crate) fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}
