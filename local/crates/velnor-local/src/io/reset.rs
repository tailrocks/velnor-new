// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use tokio::fs;

use super::RESTORES;
use super::env::{Engine, Environment};

const CACHE_DIRECTORY: &str = "cache";
const RUN_STATE: [&str; 5] = [CACHE_DIRECTORY, "api", "events", "logs", "metrics"];

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
        }
        Ok(())
    }
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
