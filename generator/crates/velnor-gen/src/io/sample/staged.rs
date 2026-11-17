// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::SampleWriteOutcome;
use crate::io::Error;

pub(super) struct StagedSample {
    pub(super) path: PathBuf,
    cleanup: bool,
}

impl StagedSample {
    pub(super) fn new(samples: &Path) -> Result<Self, Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let next = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = samples.join(format!(".velnor-gen-{}-{next}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    return Ok(Self {
                        path,
                        cleanup: true,
                    });
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
                Err(error) => return Err(Error::File(path, error)),
            }
        }
    }

    pub(super) fn publish(
        mut self,
        destination: &Path,
        exists: bool,
    ) -> Result<SampleWriteOutcome, Error> {
        let previous = self.path.join("previous");
        if exists {
            fs::rename(destination, &previous)
                .map_err(|error| Error::File(destination.into(), error))?;
        }
        if let Err(error) = fs::rename(self.path.join("new"), destination) {
            if exists && let Err(rollback) = fs::rename(&previous, destination) {
                self.cleanup = false;
                return Err(Error::Repository(format!(
                    "could not publish sample: {error}; could not restore it: {rollback}; original sample remains at {}",
                    previous.display()
                )));
            }
            return Err(Error::File(destination.into(), error));
        }
        self.cleanup = false;
        match remove_staging(&self.path) {
            Ok(()) => Ok(SampleWriteOutcome::Recorded),
            Err(error) => Ok(SampleWriteOutcome::RecordedWithCleanupFailure {
                staging_path: self.path.clone(),
                error: error.to_string(),
            }),
        }
    }
}

impl Drop for StagedSample {
    fn drop(&mut self) {
        if self.cleanup
            && let Err(error) = remove_staging(&self.path)
        {
            writeln!(
                std::io::stderr(),
                "velnor-gen: could not remove sample staging directory {}: {error}",
                self.path.display()
            )
            .ok();
        }
    }
}

fn remove_staging(path: &Path) -> std::io::Result<()> {
    writable_tree(path)?;
    fs::remove_dir_all(path)
}

fn writable_tree(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_symlink() {
        return Ok(());
    }
    #[cfg(unix)]
    if metadata.is_dir() {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = metadata.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        fs::set_permissions(path, permissions)?;
    }
    #[cfg(windows)]
    if metadata.permissions().readonly() {
        let mut permissions = metadata.permissions();
        permissions.set_readonly(false);
        fs::set_permissions(path, permissions)?;
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            writable_tree(&entry?.path())?;
        }
    }
    Ok(())
}
