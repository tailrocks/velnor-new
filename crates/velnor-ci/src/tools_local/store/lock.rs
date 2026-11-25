// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use nix::sys::stat::{Mode, mkdirat};

use super::{LOCK_PREFIX, STAGING_PREFIX, Step, expired, step};
use crate::mbx_store::random_u64;
use crate::tools_local::lease::{open_entry, published};
use crate::tree_removal::remove_tree;

pub(super) struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        fs::remove_dir(&self.0).ok();
    }
}

pub(super) enum Claim {
    Won(Lock),
    Held,
    Broken,
}

pub(super) fn claim(tools: &Path, key: &str) -> Claim {
    let path = tools.join(format!("{LOCK_PREFIX}{key}"));
    for _ in 0..2 {
        match fs::create_dir(&path) {
            Ok(()) => return Claim::Won(Lock(path)),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Claim::Broken,
        }
        if !take_over(tools, &path, key) {
            return Claim::Held;
        }
    }
    Claim::Held
}

fn take_over(tools: &Path, path: &Path, key: &str) -> bool {
    let Ok(held) = open_entry(path) else {
        return false;
    };
    let Ok(seen) = held.metadata() else {
        return false;
    };
    if !expired(&seen) {
        return false;
    }
    step(Step::Breaking);
    let marker = format!(".taken-{}.{:09}", seen.mtime(), seen.mtime_nsec());
    if mkdirat(&held, marker.as_str(), Mode::S_IRWXU).is_err() || !published(&held, path) {
        return false;
    }
    step(Step::Taken);
    let grave = tools.join(format!("{STAGING_PREFIX}{key}-{:016x}", random_u64()));
    if fs::rename(path, &grave).is_err() {
        return false;
    }
    remove_tree(&grave).ok();
    true
}
