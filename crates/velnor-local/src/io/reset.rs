// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use tokio::fs;

use super::RESTORES;
use super::env::{Engine, Environment};
use super::index_file::{lock_index, read_index, write_index};

const CACHE_DIRECTORY: &str = "cache";
const RUN_STATE: [&str; 6] = [CACHE_DIRECTORY, "api", "events", "logs", "metrics", "runs"];

impl<E: Engine> Environment<E> {
    pub async fn reset(&self, keep_cache: bool) -> io::Result<()> {
        for directory in RUN_STATE {
            let path = self.dir.join(directory);
            if keep_cache && directory == CACHE_DIRECTORY {
                remove_file(&path.join(RESTORES)).await?;
            } else {
                remove_dir(&path).await?;
            }
            fs::create_dir_all(&path).await?;
            if keep_cache && directory == CACHE_DIRECTORY {
                end_run(&path).await?;
            }
        }
        Ok(())
    }
}

/// Keeps the cache entries and their archives, and forgets the index's
/// per-run state (see `Index::end_run`) together with the partial uploads.
async fn end_run(cache: &Path) -> io::Result<()> {
    if !fs::try_exists(cache.join("index.json")).await? {
        return Ok(());
    }
    let _lock = lock_index(cache).await?;
    let mut index = read_index(cache).await?;
    let abandoned = index.end_run();
    write_index(cache, &index).await?;
    for record in abandoned {
        remove_file(&cache.join("archives").join(&record.blob)).await?;
    }
    remove_dir(&cache.join("blocks")).await
}

async fn remove_dir(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path).await {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

async fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path).await {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
