//! Mount identity checks for in-place output and cleanup walks.

use std::path::Path;

use crate::OrchestratorError;

#[cfg(target_os = "macos")]
use std::os::raw::c_char;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MountIdentity {
    #[cfg(target_os = "linux")]
    mount_id: u64,
    #[cfg(target_os = "macos")]
    mount_point: [c_char; 1024],
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    unavailable: (),
}

impl MountIdentity {
    /// Capture the mount containing a real directory without following its leaf.
    pub(super) fn capture(path: &Path) -> Result<Self, OrchestratorError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            use rustix::fs::{CWD, Mode, OFlags, openat};

            let directory = openat(
                CWD,
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|error| io(path, &error))?;
            Self::from_directory(path, &directory)
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Err(OrchestratorError::unsupported(
                "in_place_generation",
                "mount_identity_unavailable",
            ))
        }
    }

    #[cfg(target_os = "linux")]
    fn from_directory(
        path: &Path,
        directory: &impl rustix::fd::AsFd,
    ) -> Result<Self, OrchestratorError> {
        use rustix::fs::{AtFlags, StatxFlags, statx};

        let status = statx(directory, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID)
            .map_err(|error| io(path, &error))?;
        Self::from_linux_fields(
            path,
            rustix::fs::StatxFlags::from_bits_retain(status.stx_mask),
            status.stx_mnt_id,
        )
    }

    #[cfg(target_os = "linux")]
    fn from_linux_fields(
        _path: &Path,
        mask: rustix::fs::StatxFlags,
        mount_id: u64,
    ) -> Result<Self, OrchestratorError> {
        if !mask.contains(rustix::fs::StatxFlags::MNT_ID) {
            return Err(OrchestratorError::unsupported(
                "in_place_generation",
                "linux_statx_mount_id_unavailable",
            ));
        }
        Ok(Self { mount_id })
    }

    #[cfg(target_os = "macos")]
    fn from_directory(
        path: &Path,
        directory: &impl rustix::fd::AsFd,
    ) -> Result<Self, OrchestratorError> {
        let status = rustix::fs::fstatfs(directory).map_err(|error| io(path, &error))?;
        Ok(Self {
            mount_point: status.f_mntonname,
        })
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn from_directory(
        path: &Path,
        _directory: &impl rustix::fd::AsFd,
    ) -> Result<Self, OrchestratorError> {
        Err(OrchestratorError::unsupported(
            "in_place_generation",
            "mount_identity_unavailable",
        ))
    }

    #[cfg(test)]
    pub(super) fn different_for_test(&self) -> Self {
        #[cfg(target_os = "linux")]
        {
            Self {
                mount_id: self.mount_id.wrapping_add(1),
            }
        }
        #[cfg(target_os = "macos")]
        {
            let mut mount_point = self.mount_point;
            mount_point[0] = i8::from(mount_point[0] == 0);
            Self { mount_point }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            self
        }
    }
}

/// Require the directory to remain on the expected mount.
pub(super) fn require_mount(
    path: &Path,
    expected: &MountIdentity,
) -> Result<(), OrchestratorError> {
    if &MountIdentity::capture(path)? != expected {
        return Err(unsafe_path(path, "cross_mount_boundary"));
    }
    Ok(())
}

fn io(path: &Path, error: &impl std::fmt::Display) -> OrchestratorError {
    OrchestratorError::io(path.display().to_string(), error.to_string())
}

fn unsafe_path(path: &Path, reason: &str) -> OrchestratorError {
    OrchestratorError::UnsafePath {
        path: path.display().to_string(),
        reason: reason.to_owned(),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::MountIdentity;

    #[test]
    fn linux_mount_id_requires_the_kernel_to_report_its_mask() {
        let error = MountIdentity::from_linux_fields(
            std::path::Path::new("directory"),
            rustix::fs::StatxFlags::empty(),
            1,
        )
        .expect_err("missing STATX_MNT_ID must fail closed");
        assert!(
            error
                .to_string()
                .contains("linux_statx_mount_id_unavailable")
        );
    }
}
