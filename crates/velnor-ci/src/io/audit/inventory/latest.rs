// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::handle::{self, Observed, Origin};
use super::{Entries, Kind, Scan};

pub(super) struct Hooks<'a> {
    pub(super) listed: &'a dyn Fn(&OsStr),
    pub(super) observed: &'a dyn Fn(&Path),
    pub(super) resolved: &'a dyn Fn(&Path),
}

impl Default for Hooks<'_> {
    fn default() -> Self {
        Self {
            listed: &|_| {},
            observed: &|_| {},
            resolved: &|_| {},
        }
    }
}

pub(in crate::io::audit) fn newest(
    scan: &Scan,
    entries: &Entries,
    floor: Option<SystemTime>,
) -> io::Result<Option<SystemTime>> {
    newest_with(scan, entries, floor, &Hooks::default())
}

pub(super) fn newest_with(
    scan: &Scan,
    entries: &Entries,
    floor: Option<SystemTime>,
    hooks: &Hooks<'_>,
) -> io::Result<Option<SystemTime>> {
    let mut newest = SystemTime::UNIX_EPOCH;
    for (directory, children) in entries {
        let Some(listing) = reopen(scan, directory, hooks)? else {
            return Ok(None);
        };
        for (name, kind) in children {
            let path = directory.join(name);
            match handle::observe(&listing, name.as_os_str(), &path, floor, &|| {
                (hooks.resolved)(&path);
            })? {
                Some(Observed {
                    kind: current,
                    changed,
                    link,
                }) if current == *kind && followed_type_kept(scan, kind, changed) => {
                    for time in changed.into_iter().chain(link) {
                        newest = newest.max(time);
                    }
                    (hooks.observed)(&path);
                }
                _ => return Ok(None),
            }
        }
        let stamp = handle::stamp(&listing)?;
        if scan.identities.get(directory) != Some(&stamp) {
            return Ok(None);
        }
        if scan
            .entries
            .get(directory)
            .is_some_and(|walked| walked.len() == children.len())
        {
            newest = newest.max(stamp.latest(floor));
        }
    }
    Ok(Some(newest))
}

fn followed_type_kept(scan: &Scan, kind: &Kind, changed: Option<SystemTime>) -> bool {
    match kind {
        Kind::Link {
            resolved: Some(resolved),
            ..
        } => scan.entries.contains_key(resolved) == changed.is_none(),
        Kind::File | Kind::Directory | Kind::Link { .. } | Kind::Other => true,
    }
}

fn reopen(scan: &Scan, directory: &Path, hooks: &Hooks<'_>) -> io::Result<Option<nix::dir::Dir>> {
    let Some(walked) = scan.entries.get(directory) else {
        return Ok(None);
    };
    let mut listing = match handle::open(&Origin::Path, directory) {
        Ok(listing) => listing,
        Err(error) if handle::replaced(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut diverged = false;
    let listed = handle::names(&mut listing, |name| {
        (hooks.listed)(name);
        if walked.contains_key(Path::new(name)) {
            Ok(())
        } else {
            diverged = true;
            Err(io::Error::other("directory gained an entry"))
        }
    });
    match listed {
        Ok(names) => Ok((names.len() == walked.len()).then_some(listing)),
        Err(_) if diverged => Ok(None),
        Err(error) => Err(error),
    }
}
