// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use bollard::Docker;
use zeroize::Zeroizing;

use super::{AdmittedImages, NativeArtifactProvider};
use crate::error::HostError;

#[derive(PartialEq, Eq)]
struct Key {
    state_directory: PathBuf,
}

pub(crate) struct BootstrapSlot {
    key: Key,
    images: Mutex<Option<Arc<AdmittedImages>>>,
    running: AtomicBool,
    retry_at: Mutex<Instant>,
}

static SLOT: OnceLock<Mutex<Option<Arc<BootstrapSlot>>>> = OnceLock::new();

struct RunningGuard(Arc<BootstrapSlot>);

impl Drop for RunningGuard {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::Release);
    }
}

impl BootstrapSlot {
    pub(crate) fn images(&self) -> Option<Arc<AdmittedImages>> {
        self.images.lock().ok()?.clone()
    }

    pub(crate) fn retry(self: &Arc<Self>, pat: &str, docker: &Docker) -> Result<(), HostError> {
        schedule(self, pat, docker)
    }

    pub(crate) fn invalidate(&self) {
        if let Ok(mut images) = self.images.lock() {
            *images = None;
        }
    }
}

pub(crate) fn start(
    pat: &str,
    state_directory: &Path,
    docker: &Docker,
) -> Result<Arc<BootstrapSlot>, HostError> {
    if crate::compile_identity::compiled_release_identity().is_none()
        || !state_directory.is_absolute()
    {
        return Err(HostError::Identity);
    }
    let key = Key {
        state_directory: state_directory.to_owned(),
    };
    let mut global = SLOT
        .get_or_init(|| Mutex::new(None))
        .lock()
        .map_err(|_| HostError::Identity)?;
    let slot = if let Some(slot) = global.as_ref().filter(|slot| slot.key == key) {
        Arc::clone(slot)
    } else {
        let slot = Arc::new(BootstrapSlot {
            key,
            images: Mutex::new(None),
            running: AtomicBool::new(false),
            retry_at: Mutex::new(Instant::now()),
        });
        *global = Some(Arc::clone(&slot));
        slot
    };
    drop(global);
    if schedule(&slot, pat, docker).is_err()
        && let Ok(mut retry_at) = slot.retry_at.lock()
    {
        *retry_at = Instant::now() + Duration::from_secs(30);
    }
    Ok(slot)
}

fn schedule(slot: &Arc<BootstrapSlot>, pat: &str, docker: &Docker) -> Result<(), HostError> {
    if slot.images().is_some()
        || Instant::now() < *slot.retry_at.lock().map_err(|_| HostError::Identity)?
    {
        return Ok(());
    }
    if slot
        .running
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Ok(());
    }
    if let Ok(mut retry_at) = slot.retry_at.lock() {
        *retry_at = Instant::now() + Duration::from_secs(30);
    }
    let worker_slot = Arc::clone(slot);
    let pat = Zeroizing::new(pat.to_owned());
    let docker = docker.clone();
    let spawn = std::thread::Builder::new()
        .name("velnor-image-admission".to_owned())
        .spawn(move || {
            let _running = RunningGuard(Arc::clone(&worker_slot));
            let result = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| HostError::Identity)
                .and_then(|runtime| {
                    runtime.block_on(async {
                        let provider = NativeArtifactProvider::from_compiled_release_identity(
                            &pat,
                            &worker_slot.key.state_directory,
                        )?;
                        provider.bootstrap(&docker).await
                    })
                });
            if let Ok(images) = result {
                if let Ok(mut current) = worker_slot.images.lock() {
                    *current = Some(Arc::new(images));
                }
            } else if let Ok(mut retry_at) = worker_slot.retry_at.lock() {
                *retry_at = Instant::now() + Duration::from_secs(30);
            }
        });
    if spawn.is_err() {
        slot.running.store(false, Ordering::Release);
        return Err(HostError::Identity);
    }
    Ok(())
}
