// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;
use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

const HASHED_FILE_LIMIT: u64 = 1024 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Dir,
    File { size: u64, digest: Option<[u8; 32]> },
    Link { target: PathBuf },
    Other,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) kind: Kind,
    pub(crate) mode: u32,
}

pub(crate) type Tree = BTreeMap<PathBuf, Entry>;

pub(crate) fn files_under(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
            } else {
                found.push(entry.path());
            }
        }
    }
    Ok(found)
}

fn digest_of(path: &Path, size: u64) -> Option<[u8; 32]> {
    if size > HASHED_FILE_LIMIT {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .ok()?;
    Some(Sha256::digest(&bytes).into())
}

fn entry_of(path: &Path) -> Result<Entry, Box<dyn Error>> {
    let meta = std::fs::symlink_metadata(path)?;
    let kind = if meta.is_dir() {
        Kind::Dir
    } else if meta.is_file() {
        Kind::File {
            size: meta.len(),
            digest: digest_of(path, meta.len()),
        }
    } else if meta.file_type().is_symlink() {
        Kind::Link {
            target: std::fs::read_link(path)?,
        }
    } else {
        Kind::Other
    };
    Ok(Entry {
        kind,
        mode: meta.permissions().mode() & 0o7777,
    })
}

pub(crate) fn tree_of(root: &Path) -> Result<Tree, Box<dyn Error>> {
    let mut found = Tree::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(listing) = std::fs::read_dir(&dir) else {
            continue;
        };
        for item in listing {
            let path = item?.path();
            let entry = entry_of(&path)?;
            if entry.kind == Kind::Dir {
                pending.push(path.clone());
            }
            found.insert(path, entry);
        }
    }
    Ok(found)
}

pub(crate) fn unlock_tree(root: &Path) {
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let mut permissions = meta.permissions();
        permissions.set_mode(permissions.mode() | 0o700);
        drop(std::fs::set_permissions(&path, permissions));
        if let Ok(listing) = std::fs::read_dir(&path) {
            pending.extend(listing.flatten().map(|item| item.path()));
        }
    }
}

pub(crate) fn expect_denied(dir: &Path) -> Result<(), Box<dyn Error>> {
    let probe = dir.join("mode-probe");
    std::fs::create_dir_all(&probe)?;
    std::fs::write(probe.join("child"), "x")?;
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o644))?;
    let denied = std::fs::metadata(probe.join("child"))
        .err()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied);
    std::fs::set_permissions(&probe, std::fs::Permissions::from_mode(0o755))?;
    std::fs::remove_dir_all(&probe)?;
    if denied {
        Ok(())
    } else {
        Err(std::io::Error::other("file modes are not enforced for this user").into())
    }
}
