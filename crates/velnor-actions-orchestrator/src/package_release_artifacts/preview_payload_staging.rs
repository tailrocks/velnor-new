// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, invalid};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(super) struct StagingGuard {
    owned: Option<(PathBuf, fs::File)>,
}

impl StagingGuard {
    pub(super) fn create(&mut self, path: &Path) -> Result<(), OrchestratorError> {
        super::inventory::create_fresh_staging(path)?;
        let descriptor = rustix::fs::open(
            path,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| invalid("preview_payload_staging_identity"))?;
        self.owned = Some((path.to_owned(), fs::File::from(descriptor)));
        Ok(())
    }

    pub(super) fn disarm(&mut self) {
        self.owned = None;
    }

    pub(super) fn cleanup(&mut self) -> Result<(), OrchestratorError> {
        let Some((path, handle)) = self.owned.take() else {
            return Ok(());
        };
        let owned = handle
            .metadata()
            .map_err(|_| invalid("preview_payload_staging_identity"))?;
        match fs::symlink_metadata(&path) {
            Ok(current)
                if current.is_dir()
                    && !current.file_type().is_symlink()
                    && current.dev() == owned.dev()
                    && current.ino() == owned.ino() =>
            {
                fs::remove_dir_all(&path).map_err(|error| {
                    OrchestratorError::io(path.display().to_string(), error.to_string())
                })?;
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(invalid("preview_payload_staging_identity")),
        }
        Ok(())
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.cleanup().is_err() {
            eprintln!("preview_payload_staging_cleanup_failed");
        }
    }
}
