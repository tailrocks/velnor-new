// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::btree_map::Entry as Slot;
use std::collections::{BTreeMap, HashMap};
use std::ffi::{OsStr, OsString};
use std::io::Read as _;
use std::os::fd::{AsFd, OwnedFd};

use anyhow::{Context, Result};
use rustix::fs::{AtFlags, Mode, openat, statat};
use rustix::io::Errno;
use velnor_cache_rules::{Entry, EntryId, Scope, Timestamp, evict};

use super::cache::{ACCESSED, DATA, KEY, MARKER, SCOPE, VERSION};
use super::layout::Layout;
use super::publish::{Target, sync_fd};
use super::safe_fs::{directory_flags, is_regular_mode, names, open_regular, remove_entry};

const MAX_TEXT: u64 = 4096;

fn seconds(value: i64) -> Timestamp {
    Timestamp(u64::try_from(value).unwrap_or_default())
}

fn regular(entry: &OwnedFd, name: &str) -> Option<rustix::fs::Stat> {
    statat(entry, name, AtFlags::SYMLINK_NOFOLLOW)
        .ok()
        .filter(|stat| is_regular_mode(stat.st_mode))
}

fn text(entry: &OwnedFd, name: &str) -> Option<String> {
    let mut text = String::new();
    open_regular(entry.as_fd(), name)
        .ok()?
        .take(MAX_TEXT)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

struct Placed {
    scope: OsString,
    entry: OsString,
}

struct Scan {
    entries: Vec<Entry>,
    placed: HashMap<EntryId, Placed>,
    empty: Vec<OsString>,
}

fn read_entry(scope: &OwnedFd, scope_name: &OsStr, name: &OsStr) -> Option<(Entry, Placed)> {
    let entry = openat(scope, name, directory_flags(), Mode::empty()).ok()?;
    let marker = regular(&entry, MARKER)?;
    let data = regular(&entry, DATA)?;
    let accessed = regular(&entry, ACCESSED).map_or(marker.st_mtime, |stat| stat.st_mtime);
    let parsed = Entry {
        scope: Scope(
            text(&entry, SCOPE).unwrap_or_else(|| scope_name.to_string_lossy().into_owned()),
        ),
        key: text(&entry, KEY).unwrap_or_else(|| name.to_string_lossy().into_owned()),
        version: text(&entry, VERSION).unwrap_or_default(),
        size_bytes: u64::try_from(data.st_size).unwrap_or_default(),
        created: seconds(marker.st_mtime),
        last_access: seconds(accessed),
    };
    let placed = Placed {
        scope: scope_name.to_owned(),
        entry: name.to_owned(),
    };
    Some((parsed, placed))
}

fn scan(owner: &OwnedFd) -> Result<Scan> {
    let mut found = Scan {
        entries: Vec::new(),
        placed: HashMap::new(),
        empty: Vec::new(),
    };
    for scope_name in names(owner)? {
        let Ok(scope) = openat(owner, &scope_name, directory_flags(), Mode::empty()) else {
            continue;
        };
        let members = names(&scope)?;
        if members.is_empty() {
            found.empty.push(scope_name.clone());
        }
        for name in members {
            if let Some((entry, placed)) = read_entry(&scope, &scope_name, &name) {
                found.placed.insert(entry.id(), placed);
                found.entries.push(entry);
            }
        }
    }
    Ok(found)
}

pub(super) fn make_room(layout: &Layout, target: &Target, incoming: u64) -> Result<()> {
    let now = Timestamp(layout.now());
    let owner = layout.open_owner(&target.owner)?;
    let mut found = scan(&owner).context("total the cache entries of this repository")?;
    let evicted = evict(&found.entries, now, layout.budget.saturating_sub(incoming));
    if evicted.is_empty() && found.empty.is_empty() {
        return Ok(());
    }
    let aside = layout.open_aside()?;
    let mut changed = BTreeMap::new();
    for id in evicted {
        let Some(place) = found.placed.get(&id) else {
            continue;
        };
        let scope = match changed.entry(place.scope.clone()) {
            Slot::Occupied(open) => open.into_mut(),
            Slot::Vacant(slot) => {
                match openat(&owner, slot.key(), directory_flags(), Mode::empty()) {
                    Ok(scope) => slot.insert(scope),
                    Err(Errno::NOENT) => continue,
                    Err(error) => return Err(error).context("open the scope of an evicted entry"),
                }
            }
        };
        remove_entry(scope.as_fd(), &place.entry, aside.as_fd()).context("evict a cache entry")?;
    }
    for scope in changed.values() {
        sync_fd(scope, "the cache scope of an evicted entry")?;
    }
    found.empty.extend(changed.into_keys());
    for scope in &found.empty {
        layout.retire_scope(&owner, scope)?;
    }
    Ok(())
}
