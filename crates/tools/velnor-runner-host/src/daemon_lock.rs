//! One daemon. The second fails before a session exists.

use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::HostError;
use crate::worker::ProtectedStateDirectory;

/// Held advisory lock. Dropping the file releases it.
#[derive(Debug)]
pub struct DaemonLock {
    file: File,
}

impl DaemonLock {
    /// Acquire the lock or fail closed.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] when the file cannot be created or is held.
    pub fn try_acquire(path: &Path) -> Result<Self, HostError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| HostError::Lock)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| HostError::Lock)?;
        file.try_lock().map_err(|_| HostError::Lock)?;
        Ok(Self { file })
    }

    /// Acquire the daemon lock relative to an already validated state-directory descriptor.
    ///
    /// This method does not create a directory and cannot be redirected by replacing the
    /// validated directory's pathname after validation.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::Lock`] when the lock file is unsafe or already held.
    pub fn try_acquire_in(directory: &ProtectedStateDirectory) -> Result<Self, HostError> {
        let file = directory
            .open_daemon_lock_file()
            .map_err(|_| HostError::Lock)?;
        file.try_lock().map_err(|_| HostError::Lock)?;
        Ok(Self { file })
    }

    /// The lock is held while the advisory file is still open.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.file.metadata().is_ok()
    }
}
