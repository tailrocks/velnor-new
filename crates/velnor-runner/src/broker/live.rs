// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;

use anyhow::{Context, Result};
use rustix::fs::{
    AtFlags, FlockOperation, Mode, OFlags, flock, fstat, openat, renameat, statat, unlinkat,
};

use super::layout::{PRIVATE, ensure_private};
use super::safe_fs::{directory_flags, enter_directory_as, names, open_path, purge};

pub(super) const LIVE: &str = "live";
const TREES: [&str; 2] = ["incoming", "aside"];
const CLAIM_ATTEMPTS: usize = 8;

pub(super) fn open_live(root: &Path) -> Result<OwnedFd> {
    std::fs::create_dir_all(root).with_context(|| format!("create {}", root.display()))?;
    let root = open_path(root).with_context(|| format!("open {}", root.display()))?;
    let (live, _) = enter_directory_as(root.as_fd(), OsStr::new(LIVE), PRIVATE)
        .context("open the liveness directory")?;
    ensure_private(&live, "the liveness directory")?;
    Ok(live)
}

pub(super) struct Claiming {
    live: OwnedFd,
    file: OwnedFd,
    temp: String,
    id: String,
}

pub(super) fn prepare(live: &OwnedFd, id: &str) -> Result<Claiming> {
    let temp = format!("{id}.{}", uuid::Uuid::new_v4().simple());
    let file = openat(
        live,
        &temp,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )
    .context("create the liveness file")?;
    flock(&file, FlockOperation::NonBlockingLockExclusive).context("lock the liveness file")?;
    Ok(Claiming {
        live: live.try_clone()?,
        file,
        temp,
        id: id.to_owned(),
    })
}

impl Claiming {
    pub(super) fn complete(self) -> Result<OwnedFd> {
        renameat(&self.live, &self.temp, &self.live, &self.id)
            .context("publish the liveness file")?;
        Ok(self.file)
    }
}

pub(super) fn claim(root: &Path, id: &str) -> Result<OwnedFd> {
    let live = open_live(root)?;
    let mut attempts = 0;
    loop {
        match prepare(&live, id).and_then(Claiming::complete) {
            Ok(file) => return Ok(file),
            Err(error) => {
                attempts += 1;
                if attempts == CLAIM_ATTEMPTS {
                    return Err(error);
                }
            }
        }
    }
}

pub(super) enum Holder {
    Gone,
    Dead(OwnedFd),
    Alive,
}

pub(super) fn holder(live: &OwnedFd, id: &OsStr) -> Holder {
    match openat(
        live,
        id,
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    ) {
        Err(rustix::io::Errno::NOENT) => Holder::Gone,
        Err(_) => Holder::Alive,
        Ok(file) => match flock(&file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => Holder::Dead(file),
            Err(_) => Holder::Alive,
        },
    }
}

fn names_the(lock: &OwnedFd, live: &OwnedFd, id: &OsStr) -> bool {
    match (fstat(lock), statat(live, id, AtFlags::SYMLINK_NOFOLLOW)) {
        (Ok(held), Ok(named)) => (held.st_dev, held.st_ino) == (named.st_dev, named.st_ino),
        _ => false,
    }
}

fn purge_trees(trees: &[OwnedFd], id: &OsStr) {
    for tree in trees {
        let _ = purge(tree.as_fd(), id);
    }
}

pub(super) fn retire(live: &OwnedFd, trees: &[OwnedFd], id: &OsStr, lock: &OwnedFd) -> bool {
    if !names_the(lock, live, id) {
        return false;
    }
    purge_trees(trees, id);
    unlinkat(live, id, AtFlags::empty()).is_ok()
}

pub(super) struct Scan {
    live: OwnedFd,
    trees: Vec<OwnedFd>,
    ids: BTreeSet<OsString>,
}

impl Scan {
    pub(super) fn of(root: &Path) -> Option<Self> {
        let root = open_path(root).ok()?;
        let directory = |name: &str| openat(&root, name, directory_flags(), Mode::empty()).ok();
        let live = directory(LIVE)?;
        let trees: Vec<_> = TREES.iter().filter_map(|name| directory(name)).collect();
        let ids = std::iter::once(&live)
            .chain(&trees)
            .filter_map(|directory| names(directory).ok())
            .flatten()
            .collect();
        Some(Self { live, trees, ids })
    }

    pub(super) fn reap(self, own: &str) {
        for id in self.ids.iter().filter(|id| *id != OsStr::new(own)) {
            match holder(&self.live, id) {
                Holder::Dead(lock) => {
                    retire(&self.live, &self.trees, id, &lock);
                }
                Holder::Gone => purge_trees(&self.trees, id),
                Holder::Alive => {}
            }
        }
    }
}

pub(super) fn reclaim(root: &Path, own: &str) {
    if let Some(scan) = Scan::of(root) {
        scan.reap(own);
    }
}
