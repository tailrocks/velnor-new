//! Retained, validated state-directory capability for pre-lock startup checks.

use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;

use rustix::fs::{Mode, OFlags, openat};

use crate::HostError;

use super::DiagnosticsStore;
use super::filesystem::{
    effective_uid, open_private_directory_at, open_trusted_directory, validate_protected_parent,
};

/// Opaque descriptor for an existing protected state directory.
///
/// The descriptor is opened without following symlinks and retained so later lock and
/// diagnostics operations do not resolve the path again.
#[derive(Debug, Clone)]
pub struct ProtectedStateDirectory {
    directory: Arc<File>,
    owner: u32,
}

/// Stable device/inode identity for matching later path-based state opens to the retained FD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtectedStateDirectoryIdentity {
    device: u64,
    inode: u64,
}

impl ProtectedStateDirectoryIdentity {
    /// Device number observed from the retained state-directory descriptor.
    #[must_use]
    pub const fn device(self) -> u64 {
        self.device
    }

    /// Inode number observed from the retained state-directory descriptor.
    #[must_use]
    pub const fn inode(self) -> u64 {
        self.inode
    }
}

/// Validate and retain an existing protected state directory without creating anything.
///
/// The path must be absolute, its ancestors must pass the diagnostics store's existing trusted
/// owner/write-policy checks, and the leaf must be service-owned with owner access, no group or
/// other write permissions, no other permissions, and no special mode bits. Every component is opened descriptor-relatively
/// with `NOFOLLOW`; the returned capability keeps the validated leaf open.
///
/// # Errors
///
/// Returns [`HostError::Path`] if the path is absent, writable by an untrusted owner, symlinked,
/// or otherwise outside the existing protected-state policy.
pub fn validate_protected_state_directory(
    path: &Path,
) -> Result<ProtectedStateDirectory, HostError> {
    let owner = effective_uid();
    let directory = open_trusted_directory(path, owner)?;
    validate_protected_parent(&directory, owner)?;
    Ok(ProtectedStateDirectory {
        directory: Arc::new(directory),
        owner,
    })
}

impl ProtectedStateDirectory {
    /// Return the identity observed from the retained state-directory descriptor.
    ///
    /// A later path-based state open can compare its opened parent descriptor with this value.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] if the retained descriptor cannot be inspected.
    pub fn identity(&self) -> Result<ProtectedStateDirectoryIdentity, HostError> {
        let metadata = self.directory.metadata().map_err(|_| HostError::Path)?;
        Ok(ProtectedStateDirectoryIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// Open or create the mode-0700 diagnostics child relative to this retained directory.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Path`] if the child cannot be safely opened or created.
    pub fn open_diagnostics_store(&self) -> Result<DiagnosticsStore, HostError> {
        let root_directory =
            open_private_directory_at(&self.directory, "diagnostics", self.owner, true)?
                .ok_or(HostError::Path)?;
        Ok(DiagnosticsStore {
            root_directory: Arc::new(root_directory),
            owner: self.owner,
        })
    }

    pub(crate) fn open_daemon_lock_file(&self) -> Result<File, HostError> {
        let fd = openat(
            &self.directory,
            "daemon.lock",
            OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
            Mode::from_bits_truncate(0o600),
        )
        .map_err(|_| HostError::Path)?;
        let file = File::from(fd);
        let metadata = file.metadata().map_err(|_| HostError::Path)?;
        if !metadata.is_file()
            || metadata.uid() != self.owner
            || metadata.nlink() != 1
            || metadata.mode() & 0o022 != 0
            || metadata.mode() & 0o7000 != 0
        {
            return Err(HostError::Path);
        }
        Ok(file)
    }
}
