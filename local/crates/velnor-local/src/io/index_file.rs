// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, OpenOptions};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::fs;

use crate::store::Index;

static WRITES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug)]
pub struct IndexLock {
    _file: File,
}

pub async fn lock_index(dir: &Path) -> std::io::Result<IndexLock> {
    let path = dir.join("index.lock");
    tokio::task::spawn_blocking(move || {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        file.lock()?;
        Ok(IndexLock { _file: file })
    })
    .await
    .map_err(std::io::Error::other)?
}

pub(super) async fn read_index(dir: &Path) -> std::io::Result<Index> {
    match fs::read(dir.join("index.json")).await {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(std::io::Error::other),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Index::default()),
        Err(error) => Err(error),
    }
}

pub(super) async fn write_index(dir: &Path, index: &Index) -> std::io::Result<()> {
    let bytes = serde_json::to_vec_pretty(index).map_err(std::io::Error::other)?;
    let staged = dir.join(format!(
        "index.json.{}.{}.tmp",
        std::process::id(),
        WRITES.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&staged, bytes).await?;
    fs::rename(&staged, dir.join("index.json")).await
}

pub(super) fn remove_if_present(result: std::io::Result<()>) -> std::io::Result<()> {
    match result {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
