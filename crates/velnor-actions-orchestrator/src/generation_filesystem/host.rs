// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::collections::BTreeMap;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::os::fd::{AsFd, OwnedFd};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::path::Path;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::sync::{Arc, Mutex};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use rustix::fs::{self, Mode, OFlags};

use super::{
    GenerationBackendId, GenerationDirectory, GenerationFilesystem, OrchestratorError, io,
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use self::directory::HostDirectory;

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod directory;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod file;
#[cfg(any(target_os = "linux", target_os = "macos"))]
mod metadata;

#[derive(Debug)]
pub struct HostGenerationFilesystem {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    state: Arc<HostState>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug)]
struct HostState {
    id: GenerationBackendId,
    next_handle: AtomicU64,
    handles: Mutex<BTreeMap<u64, OwnedFd>>,
}

impl HostGenerationFilesystem {
    #[must_use]
    pub fn new() -> Self {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            Self {
                state: Arc::new(HostState {
                    id: GenerationBackendId::new(),
                    next_handle: AtomicU64::new(1),
                    handles: Mutex::new(BTreeMap::new()),
                }),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            Self {}
        }
    }
}

impl Default for HostGenerationFilesystem {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
impl HostState {
    fn register(&self, fd: impl AsFd) -> Result<u64, OrchestratorError> {
        let copy = rustix::io::dup(fd.as_fd())
            .map_err(|error| OrchestratorError::io("<directory descriptor>", error.to_string()))?;
        let id = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.handles
            .lock()
            .map_err(|_| OrchestratorError::Internal {
                problem: "filesystem_handle_registry_poisoned".to_owned(),
            })?
            .insert(id, copy);
        Ok(id)
    }

    fn remove(&self, id: u64) {
        if let Ok(mut handles) = self.handles.lock() {
            handles.remove(&id);
        }
    }

    fn with_handle<T>(
        &self,
        id: u64,
        operation: impl FnOnce(rustix::fd::BorrowedFd<'_>) -> std::io::Result<T>,
    ) -> std::io::Result<T> {
        let handles = self
            .handles
            .lock()
            .map_err(|_| std::io::Error::other("filesystem_handle_registry_poisoned"))?;
        let fd = handles
            .get(&id)
            .ok_or_else(|| std::io::Error::other("filesystem_handle_expired"))?;
        operation(fd.as_fd())
    }
}

impl GenerationFilesystem for HostGenerationFilesystem {
    fn open_root(&self, path: &Path) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let fd = fs::openat(
                fs::CWD,
                path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC | OFlags::NOFOLLOW,
                Mode::empty(),
            )
            .map_err(|error| io(path, &error))?;
            let handle_id = self.state.register(&fd)?;
            Ok(Arc::new(HostDirectory {
                fd,
                path: path.to_path_buf(),
                state: Arc::downgrade(&self.state),
                backend_id: self.state.id.clone(),
                handle_id,
            }))
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos")))]
        {
            let _ = path;
            Err(OrchestratorError::unsupported(
                "in_place_generation",
                "descriptor_relative_generation_unavailable",
            ))
        }
    }
}
