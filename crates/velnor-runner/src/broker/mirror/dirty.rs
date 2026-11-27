// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use anyhow::{Context, Result};
use rustix::fs::OFlags;

use super::remove_entry;

pub(super) const NAME: &str = "sweep-needed";

pub(super) async fn pending(mirror: &Path) -> bool {
    tokio::fs::symlink_metadata(mirror.join(NAME)).await.is_ok()
}

pub(super) async fn set(mirror: &Path) -> Result<()> {
    tokio::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(mirror)
        .await
        .with_context(|| format!("create {}", mirror.display()))?;
    let path = mirror.join(NAME);
    if tokio::fs::symlink_metadata(&path)
        .await
        .is_ok_and(|meta| !meta.is_file())
    {
        remove_entry(&path).await?;
    }
    tokio::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .custom_flags(
            (OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC)
                .bits()
                .cast_signed(),
        )
        .open(&path)
        .await
        .with_context(|| format!("mark {}", path.display()))?;
    Ok(())
}

pub(super) async fn clear(mirror: &Path) {
    let _ = tokio::fs::remove_file(mirror.join(NAME)).await;
}
