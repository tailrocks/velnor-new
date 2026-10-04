//! One daemon. The second fails before a session exists.

use std::fs::{File, OpenOptions};
use std::path::Path;

use crate::error::HostError;

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

    /// The lock is held while the advisory file is still open.
    #[must_use]
    pub fn is_held(&self) -> bool {
        self.file.metadata().is_ok()
    }
}
