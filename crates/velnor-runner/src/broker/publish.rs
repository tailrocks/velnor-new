// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs::File;
use std::io::Write as _;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use rustix::fs::{AtFlags, Mode, OFlags, RawMode, Stat, fchmod, fstat, fsync, openat, statat};
use rustix::io::Errno;
use velnor_cache_rules::Scope;

use super::access::stamp_access;
use super::budget::make_room;
use super::cache::{
    ACCESSED, CacheOwner, DATA, KEY, MARKER, SCOPE, VERSION, entry_name, live_access,
};
use super::incoming::Upload;
use super::layout::Layout;
use super::proto::Response;
use super::safe_fs::{directory_flags, remove_entry};

const FROZEN_DIRECTORY: RawMode = 0o555;
const FROZEN_FILE: RawMode = 0o444;

#[derive(Clone)]
pub(super) struct Target {
    pub(super) owner: CacheOwner,
    pub(super) scope: Scope,
    pub(super) key: String,
    pub(super) version: String,
}

enum Outcome {
    Published,
    AlreadyExists,
}

fn create_synced(entry: BorrowedFd<'_>, name: &str, bytes: &[u8]) -> Result<()> {
    let fd = openat(
        entry,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o644),
    )
    .with_context(|| format!("create {name} in the cache entry"))?;
    fchmod(&fd, Mode::from_bits_truncate(FROZEN_FILE))
        .with_context(|| format!("open {name} in the cache entry to the step uid"))?;
    let mut file = File::from(fd);
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("write {name} in the cache entry"))
}

fn seal(entry: &OwnedFd, target: &Target, now: u64) -> Result<()> {
    create_synced(entry.as_fd(), KEY, target.key.as_bytes())?;
    create_synced(entry.as_fd(), VERSION, target.version.as_bytes())?;
    create_synced(entry.as_fd(), SCOPE, target.scope.0.as_bytes())?;
    create_synced(entry.as_fd(), ACCESSED, b"")?;
    stamp_access(entry.as_fd(), now)?;
    create_synced(entry.as_fd(), MARKER, b"")?;
    sync_fd(entry, "the staged cache entry")
}

#[cfg(test)]
pub(super) struct Synced {
    pub(super) what: String,
    pub(super) identity: (u64, u64),
    pub(super) mode: RawMode,
}

#[cfg(test)]
pub(super) static SYNCED: std::sync::Mutex<Vec<Synced>> = std::sync::Mutex::new(Vec::new());

#[cfg(test)]
fn record(fd: &impl AsFd, what: &str) {
    if let (Ok(stat), Ok(mut synced)) = (fstat(fd), SYNCED.lock()) {
        synced.push(Synced {
            what: what.to_owned(),
            identity: (
                crate::width::narrow(stat.st_dev).unwrap_or_default(),
                stat.st_ino,
            ),
            mode: stat.st_mode & 0o7777,
        });
    }
}

#[cfg(test)]
pub(super) static FAIL_SYNC: std::sync::Mutex<Option<&'static str>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn injected(what: &str) -> bool {
    FAIL_SYNC.lock().is_ok_and(|mut armed| {
        let hit = *armed == Some(what);
        if hit {
            *armed = None;
        }
        hit
    })
}

pub(super) fn sync_fd(fd: &impl AsFd, what: &str) -> Result<()> {
    #[cfg(test)]
    {
        record(fd, what);
        ensure!(!injected(what), "sync {what}: injected failure");
    }
    fsync(fd).with_context(|| format!("sync {what}"))
}

fn sync_file(file: &File, what: &str) -> Result<()> {
    #[cfg(test)]
    record(file, what);
    file.sync_all().with_context(|| format!("sync {what}"))
}

fn same_file(left: &Stat, right: &Stat) -> bool {
    (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino)
}

fn is_streamed(published: &OwnedFd, upload: &Upload, data: &File) -> Result<()> {
    ensure!(
        same_file(&fstat(published)?, &fstat(&upload.dir)?),
        "the published cache entry is not the directory this broker streamed into"
    );
    let inside = statat(published, DATA, AtFlags::SYMLINK_NOFOLLOW)
        .context("inspect the published cache data")?;
    ensure!(
        same_file(&inside, &fstat(data)?),
        "the published cache data is not the file this broker streamed"
    );
    Ok(())
}

fn is_complete(scope: BorrowedFd<'_>, name: &OsString, now: u64) -> bool {
    openat(scope, name, directory_flags(), Mode::empty())
        .is_ok_and(|entry| live_access(entry.as_fd(), now).is_some())
}

fn not_abandoned(abandoned: &AtomicBool, step: &str) -> Result<()> {
    ensure!(
        !abandoned.load(Ordering::Relaxed),
        "the publish was abandoned {step}"
    );
    Ok(())
}

fn withdraw(layout: &Layout, scope: &OwnedFd, entry: &OsString) -> Result<()> {
    let aside = layout.open_aside()?;
    remove_entry(scope.as_fd(), entry, aside.as_fd()).context("withdraw the publication")?;
    sync_fd(scope, "the cache scope")
}

fn attempt(
    layout: &Layout,
    target: &Target,
    upload: &Upload,
    data: &File,
    abandoned: &AtomicBool,
) -> Result<Outcome> {
    fchmod(data, Mode::from_bits_truncate(FROZEN_FILE)).context("freeze the cache data")?;
    sync_file(data, "the cache data")?;
    let _lock = layout.lock(target, abandoned)?;
    let scope = layout.open_scope(target)?;
    not_abandoned(abandoned, "before it was sealed")?;
    let entry = OsString::from(entry_name(&target.version, &target.key));
    if is_complete(scope.as_fd(), &entry, layout.now()) {
        return Ok(Outcome::AlreadyExists);
    }
    let _store = layout.lock_store(abandoned)?;
    not_abandoned(abandoned, "before it evicted anything")?;
    let size = u64::try_from(fstat(data)?.st_size).unwrap_or_default();
    make_room(layout, target, size)?;
    let leftover = match statat(scope.as_fd(), &entry, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => true,
        Err(Errno::NOENT) => false,
        Err(error) => return Err(error).context("inspect the cache entry"),
    };
    if leftover {
        let aside = layout.open_aside()?;
        remove_entry(scope.as_fd(), &entry, aside.as_fd()).context("remove the leftover")?;
    }
    seal(&upload.dir, target, layout.now())?;
    not_abandoned(abandoned, "before it was committed")?;
    upload
        .rename_into(scope.as_fd(), &entry)
        .context("publish the cache entry")?;
    let committed = openat(scope.as_fd(), &entry, directory_flags(), Mode::empty())
        .context("reopen the published cache entry")
        .and_then(|published| {
            is_streamed(&published, upload, data)?;
            fchmod(&published, Mode::from_bits_truncate(FROZEN_DIRECTORY))
                .context("freeze the cache entry")?;
            sync_fd(&published, "the cache entry")
        })
        .and_then(|()| sync_fd(&scope, "the cache scope"));
    if let Err(error) = committed {
        return Err(match withdraw(layout, &scope, &entry) {
            Ok(()) => error,
            Err(left) => error.context(format!(
                "the failed publication was not fully withdrawn: {left:#}"
            )),
        });
    }
    Ok(Outcome::Published)
}

pub(super) fn publish(
    layout: &Layout,
    target: &Target,
    upload: &Upload,
    data: &File,
    abandoned: &AtomicBool,
) -> Response {
    let response = match attempt(layout, target, upload, data, abandoned) {
        Ok(Outcome::Published) => Response::Published {
            scope: target.scope.0.clone(),
        },
        Ok(Outcome::AlreadyExists) => Response::AlreadyExists,
        Err(error) => Response::Failed {
            message: format!("{error:#}"),
        },
    };
    if !matches!(response, Response::Published { .. }) {
        upload.discard();
    }
    response
}
