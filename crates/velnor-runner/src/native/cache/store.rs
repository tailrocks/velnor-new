// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/executor.rs

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rustix::fs::CWD;
use rustix::io::Errno;
use velnor_cache_rules::{Access, Entry, Scope, Timestamp, lookup};

use super::archive::checkpoint;
use crate::broker::cache::{
    CacheOwner, DATA, KEY, MARKER, VERSION, entry_dir, live_access, scope_dir,
};
use crate::broker::safe_fs::{open_path, open_regular};
use crate::exec::JobCancel;

const MAX_TEXT: u64 = 4096;

#[derive(Debug)]
pub(super) struct CacheEntry {
    pub(super) scope: Scope,
    pub(super) version: String,
    pub(super) key: String,
    pub(super) dir: PathBuf,
}

impl CacheEntry {
    pub(super) fn data(&self) -> PathBuf {
        self.dir.join(DATA)
    }
}

fn hidden(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::InvalidInput
    ) || matches!(
        Errno::from_io_error(error),
        Some(Errno::LOOP | Errno::NOTDIR)
    )
}

fn open_member(dir: &Path, name: &str) -> Result<Option<File>> {
    match open_regular(CWD, dir.join(name)) {
        Ok(file) => Ok(Some(file)),
        Err(error) if hidden(&error) => Ok(None),
        Err(error) => Err(error).with_context(|| format!("open {name} of {}", dir.display())),
    }
}

fn read_text(dir: &Path, name: &str) -> Result<Option<String>> {
    let Some(file) = open_member(dir, name)? else {
        return Ok(None);
    };
    let mut text = String::new();
    match file.take(MAX_TEXT).read_to_string(&mut text) {
        Ok(_) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read {name} of {}", dir.display())),
    }
}

fn seconds(time: SystemTime) -> Result<u64> {
    Ok(time.duration_since(UNIX_EPOCH)?.as_secs())
}

fn read_entry(dir: &Path, scope: &Scope, now: u64) -> Result<Option<Entry>> {
    let Some(marker) = open_member(dir, MARKER)? else {
        return Ok(None);
    };
    let completed = marker.metadata()?.modified()?;
    let nanos = u64::try_from(completed.duration_since(UNIX_EPOCH)?.as_nanos())?;
    let Some(accessed) = open_path(dir)
        .ok()
        .and_then(|entry| live_access(entry.as_fd(), now))
    else {
        return Ok(None);
    };
    let (Some(key), Some(version)) = (read_text(dir, KEY)?, read_text(dir, VERSION)?) else {
        return Ok(None);
    };
    let Ok(data) = fs::symlink_metadata(dir.join(DATA)) else {
        return Ok(None);
    };
    Ok(Some(Entry {
        scope: scope.clone(),
        key,
        version,
        size_bytes: data.len(),
        created: Timestamp(nanos),
        last_access: Timestamp(accessed),
    }))
}

fn read_scope(root: &Path, owner: &CacheOwner, scope: &Scope, now: u64) -> Result<Vec<Entry>> {
    let directory = scope_dir(root, owner, scope);
    let items = match fs::read_dir(&directory) {
        Ok(items) => items,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).with_context(|| format!("read {}", directory.display())),
    };
    let mut entries = Vec::new();
    for item in items {
        let item = item.with_context(|| format!("read {}", directory.display()))?;
        entries.extend(read_entry(&item.path(), scope, now)?);
    }
    Ok(entries)
}

pub(super) fn exists(
    root: &Path,
    owner: &CacheOwner,
    scope: &Scope,
    version: &str,
    key: &str,
) -> bool {
    let Ok(now) = seconds(SystemTime::now()) else {
        return false;
    };
    open_path(&entry_dir(root, owner, scope, version, key))
        .is_ok_and(|entry| live_access(entry.as_fd(), now).is_some())
}

pub(super) fn find(
    root: &Path,
    owner: &CacheOwner,
    access: &Access,
    key: &str,
    restore_keys: &[String],
    version: &str,
    cancel: Option<&JobCancel>,
) -> Result<Option<CacheEntry>> {
    let now = seconds(SystemTime::now())?;
    let mut entries = Vec::new();
    for scope in &access.read {
        entries.extend(read_scope(root, owner, scope, now)?);
    }
    checkpoint(cancel)?;
    Ok(
        lookup(&entries, access, key, restore_keys, version).map(|hit| CacheEntry {
            scope: hit.scope.clone(),
            version: hit.version.clone(),
            key: hit.key.clone(),
            dir: entry_dir(root, owner, &hit.scope, &hit.version, &hit.key),
        }),
    )
}
