// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::error::HostError;

pub(super) fn write(path: &Path, bytes: &[u8]) -> Result<(), HostError> {
    let parent = path.parent().ok_or(HostError::Path)?;
    let staging = parent.join(format!(".stage~{}", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&staging)
        .map_err(|_| HostError::Path)?;
    let published = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&staging, path))
        .and_then(|()| File::open(parent)?.sync_all());
    if published.is_err() {
        let _ = std::fs::remove_file(&staging);
        return Err(HostError::Path);
    }
    Ok(())
}
