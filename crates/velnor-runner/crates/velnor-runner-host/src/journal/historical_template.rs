// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Test-only cache of journals migrated to a historical schema version.
//!
//! Replaying every migration and validation up to a historical version costs tens of CPU seconds
//! in a debug build, and nextest runs each test in its own process, so an in-process cache would
//! never be hit. The migrated file is built once per test binary build, under a file lock, and
//! every test starts from a byte copy of it, including the write-ahead log that holds the
//! unflushed tail of the migrated state.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::error::HostError;

use super::Journal;

fn cache_directory() -> Result<PathBuf, HostError> {
    let exe = std::env::current_exe().map_err(|_| HostError::Path)?;
    let metadata = fs::metadata(&exe).map_err(|_| HostError::Path)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |age| age.as_nanos());
    let name = format!(
        "velnor-journal-templates-{}-{modified}-{}",
        metadata.len(),
        exe.file_name().and_then(|name| name.to_str()).unwrap_or("")
    );
    Ok(std::env::temp_dir().join(name))
}

async fn template(
    version: u32,
    build: impl AsyncFnOnce(&Path) -> Result<Journal, HostError>,
) -> Result<PathBuf, HostError> {
    let directory = cache_directory()?;
    fs::create_dir_all(&directory).map_err(|_| HostError::Path)?;
    let lock =
        File::create(directory.join(format!("v{version}.lock"))).map_err(|_| HostError::Path)?;
    let lock = tokio::task::spawn_blocking(move || lock.lock().map(|()| lock))
        .await
        .map_err(|_| HostError::Lock)?
        .map_err(|_| HostError::Lock)?;
    let ready = directory.join(format!("v{version}"));
    if !ready.is_dir() {
        let scratch = directory.join(format!("v{version}.building-{}", std::process::id()));
        fs::create_dir_all(&scratch).map_err(|_| HostError::Path)?;
        let database = scratch.join("launch.db");
        drop(build(&database).await?);
        fs::rename(&scratch, &ready).map_err(|_| HostError::Path)?;
    }
    drop(lock);
    Ok(ready)
}

/// Reopening a journal that already holds data must replay the bootstrap, not overwrite the file.
pub(super) fn has_content(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0)
}

/// Copies the migrated database and any write-ahead sidecar files next to `path`.
pub(super) async fn seed(
    version: u32,
    path: &Path,
    build: impl AsyncFnOnce(&Path) -> Result<Journal, HostError>,
) -> Result<(), HostError> {
    let ready = template(version, build).await?;
    let target = path.to_str().ok_or(HostError::Path)?;
    for entry in fs::read_dir(&ready).map_err(|_| HostError::Path)? {
        let entry = entry.map_err(|_| HostError::Path)?;
        let name = entry.file_name();
        let suffix = name
            .to_str()
            .and_then(|name| name.strip_prefix("launch.db"))
            .ok_or(HostError::Path)?;
        fs::copy(entry.path(), format!("{target}{suffix}")).map_err(|_| HostError::Path)?;
    }
    Ok(())
}
