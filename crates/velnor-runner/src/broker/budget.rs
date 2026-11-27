// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::collections::btree_map::Entry as Slot;
use std::ffi::OsStr;
use std::os::fd::{AsFd, OwnedFd};

use anyhow::{Context, Result};
use rustix::fs::{AtFlags, Mode, openat, statat};
use rustix::io::Errno;
use velnor_cache_rules::{Entry, Scope, Timestamp, evict};

use super::cache::{ACCESSED, DATA, MARKER};
use super::layout::Layout;
use super::publish::{Target, sync_fd};
use super::safe_fs::{directory_flags, is_regular_mode, names, remove_entry};

fn seconds(value: i64) -> Timestamp {
    Timestamp(u64::try_from(value).unwrap_or_default())
}

fn regular(entry: &OwnedFd, name: &str) -> Option<rustix::fs::Stat> {
    statat(entry, name, AtFlags::SYMLINK_NOFOLLOW)
        .ok()
        .filter(|stat| is_regular_mode(stat.st_mode))
}

fn read_entry(scope: &OwnedFd, scope_name: &OsStr, name: &OsStr) -> Option<Entry> {
    let entry = openat(scope, name, directory_flags(), Mode::empty()).ok()?;
    let marker = regular(&entry, MARKER)?;
    let data = regular(&entry, DATA)?;
    let accessed = regular(&entry, ACCESSED).map_or(marker.st_mtime, |stat| stat.st_mtime);
    Some(Entry {
        scope: Scope(scope_name.to_string_lossy().into_owned()),
        key: name.to_string_lossy().into_owned(),
        version: String::new(),
        size_bytes: u64::try_from(data.st_size).unwrap_or_default(),
        created: seconds(marker.st_mtime),
        last_access: seconds(accessed),
    })
}

fn scan(owner: &OwnedFd) -> Result<Vec<Entry>> {
    let mut found = Vec::new();
    for scope_name in names(owner)? {
        let Ok(scope) = openat(owner, &scope_name, directory_flags(), Mode::empty()) else {
            continue;
        };
        for name in names(&scope)? {
            found.extend(read_entry(&scope, &scope_name, &name));
        }
    }
    Ok(found)
}

pub(super) fn make_room(layout: &Layout, target: &Target, incoming: u64) -> Result<()> {
    let now = Timestamp(layout.now());
    let owner = layout.open_owner(&target.owner)?;
    let others = scan(&owner).context("total the cache entries of this repository")?;
    let evicted = evict(&others, now, layout.budget.saturating_sub(incoming));
    if evicted.is_empty() {
        return Ok(());
    }
    let aside = layout.open_aside()?;
    let mut changed = BTreeMap::new();
    for id in evicted {
        let scope = match changed.entry(id.scope.0) {
            Slot::Occupied(open) => open.into_mut(),
            Slot::Vacant(slot) => {
                match openat(
                    &owner,
                    slot.key().as_str(),
                    directory_flags(),
                    Mode::empty(),
                ) {
                    Ok(scope) => slot.insert(scope),
                    Err(Errno::NOENT) => continue,
                    Err(error) => return Err(error).context("open the scope of an evicted entry"),
                }
            }
        };
        remove_entry(scope.as_fd(), OsStr::new(&id.key), aside.as_fd())
            .context("evict a cache entry")?;
    }
    for scope in changed.values() {
        sync_fd(scope, "the cache scope of an evicted entry")?;
    }
    Ok(())
}
