// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::fd::{AsFd, BorrowedFd};

use anyhow::{Context, Result, ensure};
use rustix::fs::{
    AtFlags, Mode, OFlags, Timespec, Timestamps, UTIME_OMIT, fstat, futimens, openat, statat,
};
use velnor_cache_rules::Scope;

use super::cache::{ACCESSED, CacheOwner, MARKER, check_request, entry_name, scope_parts};
use super::facts::TrustedFacts;
use super::layout::Layout;
use super::publish::sync_fd;
use super::safe_fs::{directory_flags, is_regular_mode, open_path};

pub(super) fn stamp_access(entry: BorrowedFd<'_>, now: u64) -> Result<()> {
    let file = openat(
        entry,
        ACCESSED,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .context("open the access stamp of the cache entry")?;
    ensure!(
        is_regular_mode(fstat(&file)?.st_mode),
        "the access stamp of the cache entry is not a regular file"
    );
    futimens(
        &file,
        &Timestamps {
            last_access: Timespec {
                tv_sec: 0,
                tv_nsec: UTIME_OMIT,
            },
            last_modification: Timespec {
                tv_sec: i64::try_from(now).context("convert the access time")?,
                tv_nsec: 0,
            },
        },
    )
    .context("set the access stamp of the cache entry")?;
    sync_fd(&file, "the access stamp of the cache entry")
}

pub(super) fn touch(
    layout: &Layout,
    facts: &TrustedFacts,
    scope: &str,
    key: &str,
    version: &str,
) -> Result<()> {
    check_request(key, version)?;
    let scope = Scope(scope.to_owned());
    ensure!(
        facts.cache_access().read.contains(&scope),
        "the cache scope {} cannot be read by this job",
        scope.0
    );
    let mut directory = open_path(&layout.root).context("open the cache store")?;
    for part in scope_parts(&CacheOwner::of(facts), &scope) {
        directory = openat(&directory, part.as_str(), directory_flags(), Mode::empty())
            .with_context(|| format!("open the cache directory {part}"))?;
    }
    let entry = openat(
        &directory,
        entry_name(version, key).as_str(),
        directory_flags(),
        Mode::empty(),
    )
    .context("open the cache entry")?;
    let marker = statat(entry.as_fd(), MARKER, AtFlags::SYMLINK_NOFOLLOW)
        .context("inspect the cache entry marker")?;
    ensure!(
        is_regular_mode(marker.st_mode),
        "the cache entry is not complete"
    );
    stamp_access(entry.as_fd(), layout.now())
}
