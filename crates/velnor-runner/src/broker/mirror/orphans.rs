// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};

use super::remove_entry;

pub(super) async fn remove_orphan_packs(mirror: &Path) -> Result<()> {
    let directory = mirror.join("objects/pack");
    let mut entries = match tokio::fs::read_dir(&directory).await {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).with_context(|| format!("read {}", directory.display())),
    };
    let mut names = BTreeSet::new();
    while let Some(entry) = entries.next_entry().await? {
        names.insert(entry.file_name().to_string_lossy().into_owned());
    }
    for name in &names {
        let Some(stem) = name.strip_suffix(".pack") else {
            continue;
        };
        if !names.contains(&format!("{stem}.idx")) {
            remove_entry(&directory.join(name)).await?;
        }
    }
    Ok(())
}
