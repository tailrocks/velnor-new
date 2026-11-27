// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result};

use super::{install, is_dir, remove_entry};
use crate::broker::proto::Window;

const TTL: Duration = Duration::from_hours(24);
const DIRECTORY: &str = "sound";

fn name(sha: &str, window: Window) -> String {
    match window {
        Window::All => format!("{sha}-all"),
        Window::Last(count) => format!("{sha}-last-{count}"),
    }
}

fn expired(modified: SystemTime) -> bool {
    SystemTime::now()
        .duration_since(modified)
        .map_or(true, |age| age >= TTL)
}

pub(super) async fn is_fresh(mirror: &Path, sha: &str, window: Window) -> bool {
    let path = mirror.join(DIRECTORY).join(name(sha, window));
    tokio::fs::symlink_metadata(&path)
        .await
        .ok()
        .filter(std::fs::Metadata::is_file)
        .and_then(|meta| meta.modified().ok())
        .is_some_and(|modified| !expired(modified))
}

async fn purge_expired(directory: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(directory)
        .await
        .with_context(|| format!("read {}", directory.display()))?;
    while let Some(entry) = entries.next_entry().await? {
        let modified = entry.metadata().await.and_then(|meta| meta.modified());
        if modified.map_or(true, expired) {
            remove_entry(&entry.path()).await?;
        }
    }
    Ok(())
}

pub(super) async fn record(mirror: &Path, sha: &str, window: Window) -> Result<()> {
    let directory: PathBuf = mirror.join(DIRECTORY);
    if !is_dir(&directory).await {
        remove_entry(&directory).await?;
        tokio::fs::create_dir(&directory)
            .await
            .with_context(|| format!("create {}", directory.display()))?;
    }
    purge_expired(&directory).await?;
    install(mirror, &format!("{DIRECTORY}/{}", name(sha, window)), "").await
}
