// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsFd;
use std::path::PathBuf;

use rustix::fs;

use crate::OrchestratorError;
use crate::generation_filesystem::{FilesystemMetadata, GenerationFile, io};

#[cfg(target_os = "linux")]
use super::metadata::linux_mount_from_fd;
#[cfg(target_os = "macos")]
use super::metadata::macos_mount_from_fd;
use super::metadata::{metadata_from_stat, mode_from_permissions};
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
use crate::generation_filesystem::MountIdentity;

#[derive(Debug)]
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) struct HostFile {
    pub(super) file: std::fs::File,
    pub(super) path: PathBuf,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Read for HostFile {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.file.read(buffer)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Write for HostFile {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Seek for HostFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        self.file.seek(position)
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl GenerationFile for HostFile {
    fn metadata(&self) -> Result<FilesystemMetadata, OrchestratorError> {
        let stat = fs::fstat(self.file.as_fd()).map_err(|error| io(&self.path, &error))?;
        #[cfg(target_os = "linux")]
        let mount = linux_mount_from_fd(self.file.as_fd(), &self.path)?;
        #[cfg(target_os = "macos")]
        let mount = macos_mount_from_fd(self.file.as_fd(), &self.path)?;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let mount = MountIdentity::Unsupported;
        Ok(metadata_from_stat(&stat, mount))
    }

    fn set_mode(&self, mode: u32) -> Result<(), OrchestratorError> {
        let mode = mode_from_permissions(mode).map_err(|error| io(&self.path, &error))?;
        fs::fchmod(self.file.as_fd(), mode).map_err(|error| io(&self.path, &error))
    }
}
