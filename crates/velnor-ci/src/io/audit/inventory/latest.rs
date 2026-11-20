// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;
use std::time::SystemTime;

use super::handle::{self, Observed, Origin};
use super::{Entries, Scan};

/// The newest modification time of the files in a scanned snapshot, read again
/// now from each directory the walk listed, so a write that landed after the
/// walk is counted. A directory counts only if it is the very directory the walk
/// listed. `None` means an entry vanished, changed type, or sits in a directory
/// that was replaced, so the snapshot no longer matches the tree and the inputs
/// are stale.
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
            match handle::kind_at(&listing, name.as_os_str(), &path)? {
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
    let listing = match handle::open(&Origin::Path, directory) {
        Ok(listing) => listing,
        Err(error) if handle::replaced(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let same = scan.identities.get(directory) == Some(&handle::identity(&listing)?);
    Ok(same.then_some(listing))
}
