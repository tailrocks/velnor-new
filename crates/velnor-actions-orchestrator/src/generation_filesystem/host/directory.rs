// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Weak};

use rustix::fs::{self, AtFlags, Dir, Mode, OFlags, RenameFlags, Stat};

use crate::OrchestratorError;
use crate::generation_filesystem::{
    CreatedDirectoryIdentity, FilesystemMetadata, FilesystemObjectType, GenerationBackendId,
    GenerationDirectory, GenerationFile, GenerationFileOpen, GenerationRename, MountIdentity, io,
    same_object, unsafe_path,
};

#[cfg(target_os = "linux")]
use super::metadata::linux_mount_from_fd;
#[cfg(target_os = "macos")]
use super::metadata::macos_mount_from_fd;
use super::metadata::{metadata_from_stat, mode_from_permissions, object_type};
use super::{HostState, file::HostFile};

#[derive(Debug)]
#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) struct HostDirectory {
    pub(super) fd: OwnedFd,
    pub(super) path: PathBuf,
    pub(super) state: Weak<HostState>,
    pub(super) backend_id: GenerationBackendId,
    pub(super) handle_id: u64,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl Drop for HostDirectory {
    fn drop(&mut self) {
        if let Some(state) = self.state.upgrade() {
            state.remove(self.handle_id);
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl HostDirectory {
    fn state(&self) -> Result<Arc<HostState>, OrchestratorError> {
        self.state
            .upgrade()
            .ok_or_else(|| OrchestratorError::Internal {
                problem: "filesystem_backend_expired".to_owned(),
            })
    }

    fn entry_metadata(
        &self,
        name: &OsStr,
        stat: Stat,
    ) -> Result<FilesystemMetadata, OrchestratorError> {
        #[cfg(target_os = "linux")]
        {
            use rustix::fs::{StatxFlags, statx};

            let mount = statx(
                self.fd.as_fd(),
                name,
                AtFlags::SYMLINK_NOFOLLOW,
                StatxFlags::MNT_ID,
            )
            .map_err(|error| io(&self.child_path(name), &error))?;
            if !StatxFlags::from_bits_retain(mount.stx_mask).contains(StatxFlags::MNT_ID) {
                return Err(OrchestratorError::unsupported(
                    "in_place_generation",
                    "linux_statx_mount_id_unavailable",
                ));
            }
            Ok(metadata_from_stat(
                &stat,
                MountIdentity::Linux {
                    mount_id: mount.stx_mnt_id,
                },
            ))
        }
        #[cfg(target_os = "macos")]
        {
            match object_type(stat.st_mode) {
                FilesystemObjectType::Directory => {
                    let opened = self.open_dir(name)?;
                    let metadata = opened.metadata()?;
                    if !same_object(
                        &metadata,
                        &metadata_from_stat(&stat, metadata.mount.clone()),
                    ) {
                        return Err(unsafe_path(
                            &self.child_path(name),
                            "directory_identity_changed",
                        ));
                    }
                    Ok(metadata)
                }
                FilesystemObjectType::RegularFile => {
                    let opened = self.open_file(name, GenerationFileOpen::ReadOnly)?;
                    let metadata = opened.metadata()?;
                    if !same_object(
                        &metadata,
                        &metadata_from_stat(&stat, metadata.mount.clone()),
                    ) {
                        return Err(unsafe_path(&self.child_path(name), "file_identity_changed"));
                    }
                    Ok(metadata)
                }
                FilesystemObjectType::Symlink => {
                    let parent = self.metadata()?;
                    Ok(metadata_from_stat(&stat, parent.mount))
                }
                FilesystemObjectType::Other => Err(OrchestratorError::unsupported(
                    "in_place_generation",
                    "entry_mount_identity_unavailable",
                )),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = (name, stat);
            Err(OrchestratorError::unsupported(
                "in_place_generation",
                "mount_identity_unavailable",
            ))
        }
    }

    fn child_path(&self, name: &OsStr) -> PathBuf {
        self.path.join(name)
    }

    fn new_child(
        &self,
        fd: OwnedFd,
        path: PathBuf,
    ) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        let state = self.state()?;
        let handle_id = state.register(&fd)?;
        Ok(Arc::new(Self {
            fd,
            path,
            state: Arc::downgrade(&state),
            backend_id: self.backend_id.clone(),
            handle_id,
        }))
    }

    fn created_directory_identity(
        &self,
        name: &OsStr,
    ) -> std::io::Result<CreatedDirectoryIdentity> {
        let stat = fs::statat(self.fd.as_fd(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(std::io::Error::from)?;
        let metadata = metadata_from_stat(&stat, MountIdentity::Unsupported);
        Ok(CreatedDirectoryIdentity {
            device: metadata.device,
            inode: metadata.inode,
        })
    }

    fn created_directory_identity_from_open(
        &self,
        name: &OsStr,
    ) -> std::io::Result<CreatedDirectoryIdentity> {
        let fd = fs::openat(
            self.fd.as_fd(),
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;
        let stat = fs::fstat(fd.as_fd()).map_err(std::io::Error::from)?;
        let metadata = metadata_from_stat(&stat, MountIdentity::Unsupported);
        Ok(CreatedDirectoryIdentity {
            device: metadata.device,
            inode: metadata.inode,
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl GenerationDirectory for HostDirectory {
    fn path(&self) -> &Path {
        &self.path
    }

    fn backend_id(&self) -> &GenerationBackendId {
        &self.backend_id
    }

    fn handle_id(&self) -> u64 {
        self.handle_id
    }

    fn metadata(&self) -> Result<FilesystemMetadata, OrchestratorError> {
        let stat = fs::fstat(self.fd.as_fd()).map_err(|error| io(&self.path, &error))?;
        #[cfg(target_os = "linux")]
        let mount = linux_mount_from_fd(self.fd.as_fd(), &self.path)?;
        #[cfg(target_os = "macos")]
        let mount = macos_mount_from_fd(self.fd.as_fd(), &self.path)?;
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        let mount = MountIdentity::Unsupported;
        Ok(metadata_from_stat(&stat, mount))
    }

    fn stat(&self, name: &OsStr) -> Result<FilesystemMetadata, OrchestratorError> {
        let stat = fs::statat(self.fd.as_fd(), name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| io(&self.child_path(name), &error))?;
        self.entry_metadata(name, stat)
    }

    fn stat_opt(&self, name: &OsStr) -> Result<Option<FilesystemMetadata>, OrchestratorError> {
        match fs::statat(self.fd.as_fd(), name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => self.entry_metadata(name, stat).map(Some),
            Err(error) if error == rustix::io::Errno::NOENT => Ok(None),
            Err(error) => Err(io(&self.child_path(name), &error)),
        }
    }

    fn try_clone(&self) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        let fd = rustix::io::dup(self.fd.as_fd()).map_err(|error| io(&self.path, &error))?;
        self.new_child(fd, self.path.clone())
    }

    fn open_dir(&self, name: &OsStr) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        let path = self.child_path(name);
        let fd = fs::openat(
            self.fd.as_fd(),
            name,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::empty(),
        )
        .map_err(|error| io(&path, &error))?;
        self.new_child(fd, path)
    }

    fn open_file(
        &self,
        name: &OsStr,
        access: GenerationFileOpen,
    ) -> Result<Box<dyn GenerationFile>, OrchestratorError> {
        let flags = match access {
            GenerationFileOpen::ReadOnly => OFlags::RDONLY,
            GenerationFileOpen::WriteCreateNew => OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL,
        };
        let path = self.child_path(name);
        let fd = fs::openat(
            self.fd.as_fd(),
            name,
            flags | OFlags::CLOEXEC | OFlags::NOFOLLOW,
            Mode::from_raw_mode(0o666),
        )
        .map_err(|error| io(&path, &error))?;
        Ok(Box::new(HostFile {
            file: std::fs::File::from(fd),
            path,
        }))
    }

    fn create_dir(&self, name: &OsStr, mode: u32) -> Result<(), OrchestratorError> {
        let path = self.child_path(name);
        let mode = mode_from_permissions(mode).map_err(|error| io(&path, &error))?;
        fs::mkdirat(self.fd.as_fd(), name, mode).map_err(|error| io(&path, &error))
    }

    fn create_dir_raw(&self, name: &OsStr, mode: u32) -> std::io::Result<CreatedDirectoryIdentity> {
        let mode = mode_from_permissions(mode)?;
        fs::mkdirat(self.fd.as_fd(), name, mode).map_err(std::io::Error::from)?;
        let identity = self.created_directory_identity(name).or_else(|first| {
            self.created_directory_identity_from_open(name)
                .or_else(|open_failure| {
                    self.created_directory_identity(name).map_err(|retry| {
                        std::io::Error::other(format!(
                            "created_directory_identity_unavailable:cleanup_refused:{first}; open_failed:{open_failure}; retry_failed:{retry}"
                        ))
                    })
                })
        })?;
        Ok(identity)
    }

    fn set_mode(&self, mode: u32) -> Result<(), OrchestratorError> {
        let mode = mode_from_permissions(mode).map_err(|error| io(&self.path, &error))?;
        fs::fchmod(self.fd.as_fd(), mode).map_err(|error| io(&self.path, &error))
    }

    fn entries(&self) -> Result<Vec<OsString>, OrchestratorError> {
        let mut dir = Dir::read_from(self.fd.as_fd()).map_err(|error| io(&self.path, &error))?;
        let mut entries = Vec::new();
        while let Some(entry) = dir.read() {
            let entry = entry.map_err(|error| io(&self.path, &error))?;
            let name = entry.file_name();
            if name.to_bytes() != b"." && name.to_bytes() != b".." {
                entries.push(OsString::from_vec(name.to_bytes().to_vec()));
            }
        }
        Ok(entries)
    }

    fn read_link(&self, name: &OsStr) -> Result<OsString, OrchestratorError> {
        let target = fs::readlinkat(self.fd.as_fd(), name, Vec::new())
            .map_err(|error| io(&self.child_path(name), &error))?;
        Ok(OsString::from_vec(target.to_bytes().to_vec()))
    }

    fn symlink(&self, target: &OsStr, name: &OsStr) -> Result<(), OrchestratorError> {
        fs::symlinkat(target, self.fd.as_fd(), name)
            .map_err(|error| io(&self.child_path(name), &error))
    }

    fn remove_file(&self, name: &OsStr) -> Result<(), OrchestratorError> {
        fs::unlinkat(self.fd.as_fd(), name, AtFlags::empty())
            .map_err(|error| io(&self.child_path(name), &error))
    }

    fn remove_dir(&self, name: &OsStr) -> Result<(), OrchestratorError> {
        fs::unlinkat(self.fd.as_fd(), name, AtFlags::REMOVEDIR)
            .map_err(|error| io(&self.child_path(name), &error))
    }

    fn rename_to(
        &self,
        name: &OsStr,
        destination: &dyn GenerationDirectory,
        destination_name: &OsStr,
        mode: GenerationRename,
    ) -> std::io::Result<()> {
        if !self.backend_id.matches(destination.backend_id()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "generation_directory_backend_mismatch",
            ));
        }
        let state = self
            .state
            .upgrade()
            .ok_or_else(|| std::io::Error::other("filesystem_backend_expired"))?;
        let flags = match mode {
            GenerationRename::NoReplace => RenameFlags::NOREPLACE,
            GenerationRename::Exchange => RenameFlags::EXCHANGE,
        };
        state.with_handle(destination.handle_id(), |destination_fd| {
            fs::renameat_with(
                self.fd.as_fd(),
                name,
                destination_fd,
                destination_name,
                flags,
            )
            .map_err(std::io::Error::from)
        })
    }
}
