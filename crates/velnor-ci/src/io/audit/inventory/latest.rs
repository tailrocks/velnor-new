// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::handle::{self, Observed, Origin};
use super::{Entries, Scan};

pub(in crate::io::audit) fn newest(
    scan: &Scan,
    entries: &Entries,
) -> io::Result<Option<SystemTime>> {
    let mut newest = SystemTime::UNIX_EPOCH;
    for (directory, children) in entries {
        let Some(listing) = reopen(scan, directory)? else {
            return Ok(None);
        };
        for (name, kind) in children {
            let path = directory.join(name);
            match handle::observe(&listing, name.as_os_str(), &path)? {
                Some(Observed {
                    kind: current,
                    modified,
                }) if current == *kind => {
                    if let Some(time) = modified {
                        newest = newest.max(time);
                    }
                }
                _ => return Ok(None),
            }
        }
    }
    Ok(Some(newest))
}

fn reopen(scan: &Scan, directory: &Path) -> io::Result<Option<nix::dir::Dir>> {
    let mut listing = match handle::open(&Origin::Path, directory) {
        Ok(listing) => listing,
        Err(error) if handle::replaced(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let listed: BTreeSet<PathBuf> = handle::names(&mut listing, |_| Ok(()))?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let walked = scan
        .entries
        .get(directory)
        .is_some_and(|children| children.keys().cloned().collect::<BTreeSet<_>>() == listed);
    let same = scan.identities.get(directory) == Some(&handle::identity(&listing)?);
    Ok((walked && same).then_some(listing))
}
