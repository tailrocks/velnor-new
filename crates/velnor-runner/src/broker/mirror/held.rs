// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, TryLockError};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rustix::fs::OFlags;

#[derive(Clone)]
pub(super) struct Held(Arc<File>);

impl Held {
    pub(super) async fn acquire(path: &Path) -> Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .custom_flags(
                (OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC)
                    .bits()
                    .cast_signed(),
            )
            .open(path)
            .await
            .with_context(|| format!("open {}", path.display()))?;
        let meta = file
            .metadata()
            .await
            .with_context(|| format!("inspect {}", path.display()))?;
        if !meta.is_file() {
            bail!("mirror lock {} is not a regular file", path.display());
        }
        let file = file.into_std().await;
        let mut backoff = Duration::from_millis(5);
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self(Arc::new(file))),
                Err(TryLockError::WouldBlock) => {
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_millis(100));
                }
                Err(TryLockError::Error(error)) => {
                    return Err(error).with_context(|| format!("lock {}", path.display()));
                }
            }
        }
    }

    pub(super) fn file(&self) -> Arc<File> {
        Arc::clone(&self.0)
    }

    pub(super) async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<T> {
        let held = self.clone();
        tokio::task::spawn_blocking(move || {
            let done = work();
            drop(held);
            done
        })
        .await
        .context("wait for the mirror cleanup")
    }
}

#[cfg(test)]
mod tests;
