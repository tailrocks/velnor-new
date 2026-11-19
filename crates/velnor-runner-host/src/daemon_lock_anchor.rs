// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;

use super::filesystem::ensure_regular_or_missing;
use super::{Anchor, MAX_ANCHOR_BYTES};
use crate::error::HostError;

pub(super) fn read_anchor(path: &Path) -> Result<Option<Anchor>, HostError> {
    ensure_regular_or_missing(path)?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(HostError::Journal),
    };
    if metadata.len() > MAX_ANCHOR_BYTES {
        return Err(HostError::Journal);
    }
    let bytes = fs::read(path).map_err(|_| HostError::Journal)?;
    let anchor = serde_json::from_slice(&bytes).map_err(|_| HostError::Journal)?;
    Ok(Some(anchor))
}

/// Removes leftover `.{anchor}.tmp-*` files. Callers hold the lineage lock and the
/// state mutex, and no anchor is published, so every such file is an unpublished
/// remnant of a writer that failed or died before `rename`. `remove_file` unlinks a
/// symlink itself and never follows it; anything that is not a file or symlink is
/// refused rather than deleted.
pub(super) fn remove_stale_anchor_temporaries(path: &Path) -> Result<(), HostError> {
    let parent = path.parent().ok_or(HostError::Path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(HostError::Path)?;
    let prefix = format!(".{name}.tmp-");
    for entry in fs::read_dir(parent).map_err(|_| HostError::Journal)? {
        let entry = entry.map_err(|_| HostError::Journal)?;
        if !entry
            .file_name()
            .to_str()
            .is_some_and(|candidate| candidate.starts_with(&prefix))
        {
            continue;
        }
        let stale = entry.path();
        let metadata = fs::symlink_metadata(&stale).map_err(|_| HostError::Journal)?;
        if metadata.is_dir() {
            return Err(HostError::Journal);
        }
        fs::remove_file(&stale).map_err(|_| HostError::Journal)?;
    }
    Ok(())
}

pub(super) fn write_anchor(path: &Path, anchor: &Anchor) -> Result<(), HostError> {
    write_anchor_with(path, anchor, |temporary, anchor| {
        fs::rename(temporary, anchor)
    })
}

pub(super) fn write_anchor_with(
    path: &Path,
    anchor: &Anchor,
    publish: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), HostError> {
    ensure_regular_or_missing(path)?;
    let bytes = serde_json::to_vec(anchor).map_err(|_| HostError::Journal)?;
    let parent = path.parent().ok_or(HostError::Path)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(HostError::Path)?;
    let temporary = parent.join(format!(".{name}.tmp-{}", uuid::Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary).map_err(|_| HostError::Journal)?;
    let published = file
        .write_all(&bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| {
            drop(file);
            publish(&temporary, path)
        });
    if published.is_err() {
        fs::remove_file(&temporary).ok();
        return Err(HostError::Journal);
    }
    sync_parent(path)
}

pub(super) fn sync_parent(path: &Path) -> Result<(), HostError> {
    let parent = path.parent().ok_or(HostError::Path)?;
    #[cfg(test)]
    if super::tests::take_directory_sync_failure() {
        return Err(HostError::Journal);
    }
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| HostError::Journal)
}
