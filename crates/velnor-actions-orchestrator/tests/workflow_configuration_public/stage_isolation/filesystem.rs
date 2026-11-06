// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use velnor_actions_orchestrator::generation_filesystem::CreatedDirectoryIdentity;
use velnor_actions_orchestrator::{
    FilesystemMetadata, GenerateOptions, GenerationDirectory, GenerationFile, GenerationFileOpen,
    GenerationFilesystem, GenerationRename, HostGenerationFilesystem, MountIdentity,
    OrchestratorError, generate, prepare,
};

use super::support::GENERATE_LOCK_NAME;

#[path = "filesystem/forward_file.rs"]
mod forward_file;
use forward_file::ForwardFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum BoundaryPoint {
    Rename(GenerationRename),
    RemoveDirectory(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreationTarget {
    Lock,
    Stage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CreationFault {
    OpenDirectory,
    SetMode,
    Metadata,
    MountValidation,
}

#[derive(Debug)]
struct FilesystemFault {
    target: CreationTarget,
    fault: CreationFault,
    fired: AtomicBool,
    metadata_calls: std::sync::atomic::AtomicUsize,
}

impl FilesystemFault {
    fn matches(&self, path: &Path) -> bool {
        let Some(name) = path.file_name() else {
            return false;
        };
        match self.target {
            CreationTarget::Lock => name == GENERATE_LOCK_NAME,
            CreationTarget::Stage => name.to_string_lossy().starts_with(".github.velnor-stage-"),
        }
    }

    fn fail(&self, path: &Path) -> bool {
        self.matches(path) && !self.fired.swap(true, Ordering::AcqRel)
    }

    fn metadata_error(&self, path: &Path) -> bool {
        self.fault == CreationFault::Metadata && self.fail(path)
    }

    fn mismount(&self, path: &Path) -> bool {
        if self.fault != CreationFault::MountValidation || !self.matches(path) {
            return false;
        }
        let call = self.metadata_calls.fetch_add(1, Ordering::AcqRel) + 1;
        let validation_call = 2;
        call == validation_call && !self.fired.swap(true, Ordering::AcqRel)
    }
}

#[derive(Debug)]
struct BoundaryGate {
    point: BoundaryPoint,
    reached: mpsc::SyncSender<()>,
    release: Mutex<mpsc::Receiver<()>>,
    entered: AtomicBool,
}

impl BoundaryGate {
    fn pause_if(&self, point: &BoundaryPoint) -> std::io::Result<()> {
        if &self.point != point || self.entered.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.reached
            .send(())
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        self.release
            .lock()
            .map_err(|_| std::io::Error::other("filesystem_barrier_poisoned"))?
            .recv_timeout(Duration::from_secs(30))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }
}

#[derive(Debug)]
struct ForwardFilesystem {
    inner: Arc<dyn GenerationFilesystem>,
    gate: Option<Arc<BoundaryGate>>,
    fault: Option<Arc<FilesystemFault>>,
}

impl GenerationFilesystem for ForwardFilesystem {
    fn open_root(&self, path: &Path) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        let inner = self.inner.open_root(path)?;
        Ok(Arc::new(ForwardDirectory {
            inner,
            gate: self.gate.clone(),
            fault: self.fault.clone(),
        }))
    }
}

#[derive(Debug)]
struct ForwardDirectory {
    inner: Arc<dyn GenerationDirectory>,
    gate: Option<Arc<BoundaryGate>>,
    fault: Option<Arc<FilesystemFault>>,
}

impl ForwardDirectory {
    fn wrap(&self, inner: Arc<dyn GenerationDirectory>) -> Arc<dyn GenerationDirectory> {
        Arc::new(Self {
            inner,
            gate: self.gate.clone(),
            fault: self.fault.clone(),
        })
    }
}

impl GenerationDirectory for ForwardDirectory {
    fn path(&self) -> &Path {
        self.inner.path()
    }

    fn backend_id(&self) -> &velnor_actions_orchestrator::GenerationBackendId {
        self.inner.backend_id()
    }

    fn handle_id(&self) -> u64 {
        self.inner.handle_id()
    }

    fn metadata(&self) -> Result<FilesystemMetadata, OrchestratorError> {
        if let Some(fault) = &self.fault
            && fault.metadata_error(self.inner.path())
        {
            return Err(OrchestratorError::io(
                self.inner.path().display().to_string(),
                "injected_generation_metadata_failure",
            ));
        }
        let mut metadata = self.inner.metadata()?;
        if let Some(fault) = &self.fault
            && fault.mismount(self.inner.path())
        {
            metadata.mount = match metadata.mount {
                MountIdentity::Linux { mount_id } => MountIdentity::Linux {
                    mount_id: mount_id.wrapping_add(1),
                },
                MountIdentity::Macos {
                    mount_point,
                    filesystem_id,
                } => MountIdentity::Macos {
                    mount_point,
                    filesystem_id: filesystem_id.wrapping_add(1),
                },
                MountIdentity::Unsupported => MountIdentity::Unsupported,
            };
        }
        Ok(metadata)
    }

    fn stat(&self, name: &std::ffi::OsStr) -> Result<FilesystemMetadata, OrchestratorError> {
        self.inner.stat(name)
    }

    fn stat_opt(
        &self,
        name: &std::ffi::OsStr,
    ) -> Result<Option<FilesystemMetadata>, OrchestratorError> {
        self.inner.stat_opt(name)
    }

    fn try_clone(&self) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        Ok(self.wrap(self.inner.try_clone()?))
    }

    fn open_dir(
        &self,
        name: &std::ffi::OsStr,
    ) -> Result<Arc<dyn GenerationDirectory>, OrchestratorError> {
        if let Some(fault) = &self.fault
            && fault.fault == CreationFault::OpenDirectory
            && (match fault.target {
                CreationTarget::Lock => name == std::ffi::OsStr::new(GENERATE_LOCK_NAME),
                CreationTarget::Stage => {
                    name.to_string_lossy().starts_with(".github.velnor-stage-")
                }
            })
            && fault.fail(&self.inner.path().join(name))
        {
            return Err(OrchestratorError::io(
                self.inner.path().join(name).display().to_string(),
                "injected_generation_open_failure",
            ));
        }
        Ok(self.wrap(self.inner.open_dir(name)?))
    }

    fn open_file(
        &self,
        name: &std::ffi::OsStr,
        access: GenerationFileOpen,
    ) -> Result<Box<dyn GenerationFile>, OrchestratorError> {
        Ok(Box::new(ForwardFile::new(
            self.inner.open_file(name, access)?,
        )))
    }

    fn create_dir(&self, name: &std::ffi::OsStr, mode: u32) -> Result<(), OrchestratorError> {
        self.inner.create_dir(name, mode)
    }

    fn create_dir_raw(
        &self,
        name: &std::ffi::OsStr,
        mode: u32,
    ) -> std::io::Result<CreatedDirectoryIdentity> {
        self.inner.create_dir_raw(name, mode)
    }

    fn set_mode(&self, mode: u32) -> Result<(), OrchestratorError> {
        if let Some(fault) = &self.fault
            && fault.fault == CreationFault::SetMode
            && fault.fail(self.inner.path())
        {
            return Err(OrchestratorError::io(
                self.inner.path().display().to_string(),
                "injected_generation_mode_failure",
            ));
        }
        self.inner.set_mode(mode)
    }

    fn entries(&self) -> Result<Vec<std::ffi::OsString>, OrchestratorError> {
        self.inner.entries()
    }

    fn read_link(&self, name: &std::ffi::OsStr) -> Result<std::ffi::OsString, OrchestratorError> {
        self.inner.read_link(name)
    }

    fn symlink(
        &self,
        target: &std::ffi::OsStr,
        name: &std::ffi::OsStr,
    ) -> Result<(), OrchestratorError> {
        self.inner.symlink(target, name)
    }

    fn remove_file(&self, name: &std::ffi::OsStr) -> Result<(), OrchestratorError> {
        self.inner.remove_file(name)
    }

    fn remove_dir(&self, name: &std::ffi::OsStr) -> Result<(), OrchestratorError> {
        if name == std::ffi::OsStr::new(GENERATE_LOCK_NAME)
            && let Some(gate) = &self.gate
        {
            gate.pause_if(&BoundaryPoint::RemoveDirectory(GENERATE_LOCK_NAME))
                .map_err(|error| {
                    OrchestratorError::io(
                        self.inner.path().display().to_string(),
                        error.to_string(),
                    )
                })?;
        }
        self.inner.remove_dir(name)
    }

    fn rename_to(
        &self,
        name: &std::ffi::OsStr,
        destination: &dyn GenerationDirectory,
        destination_name: &std::ffi::OsStr,
        mode: GenerationRename,
    ) -> std::io::Result<()> {
        if let Some(gate) = &self.gate {
            gate.pause_if(&BoundaryPoint::Rename(mode))?;
        }
        self.inner
            .rename_to(name, destination, destination_name, mode)
    }
}

pub(super) struct RunningGeneration {
    release: Option<mpsc::SyncSender<()>>,
    join: Option<JoinHandle<Result<(), String>>>,
}

impl RunningGeneration {
    pub(super) fn resume_and_join(&mut self) -> Result<(), String> {
        if let Some(release) = self.release.take() {
            release.send(()).map_err(|error| error.to_string())?;
        }
        self.join
            .take()
            .ok_or_else(|| "generation_thread_already_joined".to_owned())?
            .join()
            .map_err(|_| "generation_thread_panicked".to_owned())?
    }
}

impl Drop for RunningGeneration {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            release.send(()).unwrap_or(());
        }
        if let Some(join) = self.join.take() {
            drop(join.join().unwrap_or(Ok(())));
        }
    }
}

pub(super) fn start_generation_at(
    root: &Path,
    point: BoundaryPoint,
) -> (RunningGeneration, mpsc::Receiver<()>) {
    let (reached_tx, reached_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let gate = Arc::new(BoundaryGate {
        point,
        reached: reached_tx,
        release: Mutex::new(release_rx),
        entered: AtomicBool::new(false),
    });
    let filesystem = Arc::new(ForwardFilesystem {
        inner: Arc::new(HostGenerationFilesystem::new()),
        gate: Some(gate),
        fault: None,
    });
    let options = GenerateOptions {
        output_dir: None,
        filesystem,
    };
    let root = root.to_path_buf();
    let join = std::thread::spawn(move || {
        prepare(&root)
            .and_then(|preparation| generate(&preparation, &options))
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    (
        RunningGeneration {
            release: Some(release_tx),
            join: Some(join),
        },
        reached_rx,
    )
}

pub(super) fn generate_with_creation_fault(
    root: &Path,
    target: CreationTarget,
    fault: CreationFault,
) -> Result<(), String> {
    let filesystem = Arc::new(ForwardFilesystem {
        inner: Arc::new(HostGenerationFilesystem::new()),
        gate: None,
        fault: Some(Arc::new(FilesystemFault {
            target,
            fault,
            fired: AtomicBool::new(false),
            metadata_calls: std::sync::atomic::AtomicUsize::new(0),
        })),
    });
    let options = GenerateOptions {
        output_dir: None,
        filesystem,
    };
    let preparation = prepare(root).map_err(|error| error.to_string())?;
    generate(&preparation, &options)
        .map(|_| ())
        .map_err(|error| error.to_string())
}
