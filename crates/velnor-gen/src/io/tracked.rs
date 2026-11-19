// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::Path;

use velnor_model::{RepoPath, RepoView};

use super::{Error, program};

const SYMLINK_MODE: &[u8] = b"120000";

pub(super) fn tracked(root: &Path) -> Result<(BTreeSet<RepoPath>, BTreeSet<RepoPath>), Error> {
    let listing = program(
        root,
        "git",
        &[OsStr::new("ls-files"), OsStr::new("-s"), OsStr::new("-z")],
    )?;
    let mut paths = BTreeSet::new();
    let mut symlinks = BTreeSet::new();
    for entry in listing
        .split(|byte| *byte == 0)
        .filter(|entry| !entry.is_empty())
    {
        let (meta, name) = entry
            .split_at_checked(entry.iter().position(|byte| *byte == b'\t').unwrap_or(0))
            .ok_or_else(|| Error::Repository("unreadable git index entry".to_owned()))?;
        let text = std::str::from_utf8(name.get(1..).unwrap_or_default()).map_err(|_| {
            Error::Repository(format!(
                "tracked path {:?} is not UTF-8",
                String::from_utf8_lossy(name)
            ))
        })?;
        let path = RepoPath::new(text).map_err(|e| Error::Repository(e.to_string()))?;
        if meta.starts_with(SYMLINK_MODE) {
            symlinks.insert(path.clone());
        }
        paths.insert(path);
    }
    Ok((paths, symlinks))
}

pub(super) fn tracked_sizes(root: &Path, view: &RepoView) -> BTreeMap<RepoPath, u64> {
    view.tracked_paths
        .iter()
        .filter_map(|path| {
            let metadata = std::fs::symlink_metadata(root.join(path.as_str())).ok()?;
            metadata.is_file().then(|| (path.clone(), metadata.len()))
        })
        .collect()
}
