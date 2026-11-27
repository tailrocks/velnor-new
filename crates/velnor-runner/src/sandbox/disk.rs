// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const BLOCK_BYTES: u64 = 512;
const GIB: u64 = 1 << 30;

type Seen = HashSet<(u64, u64)>;

fn counted(metadata: &Metadata, seen: &mut Seen) -> u64 {
    if metadata.nlink() > 1 && !seen.insert((metadata.dev(), metadata.ino())) {
        return 0;
    }
    metadata.blocks() * BLOCK_BYTES
}

fn tree(root: &Path, seen: &mut Seen) -> u64 {
    let Ok(metadata) = std::fs::symlink_metadata(root) else {
        return 0;
    };
    let mut total = counted(&metadata, seen);
    let mut pending: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            total += counted(&metadata, seen);
            if metadata.is_dir() {
                pending.push(entry.path());
            }
        }
    }
    total
}

fn deleted_and_open(cgroup: &Path, seen: &mut Seen) -> u64 {
    let Ok(members) = std::fs::read_to_string(cgroup.join("cgroup.procs")) else {
        return 0;
    };
    let mut total = 0;
    for pid in members.lines() {
        let descriptors = Path::new("/proc").join(pid).join("fd");
        let Ok(entries) = std::fs::read_dir(&descriptors) else {
            continue;
        };
        for entry in entries.flatten() {
            let held_deleted = std::fs::read_link(entry.path())
                .is_ok_and(|target| target.to_string_lossy().ends_with(" (deleted)"));
            if !held_deleted {
                continue;
            }
            if let Ok(metadata) = std::fs::metadata(entry.path())
                && metadata.is_file()
                && metadata.nlink() == 0
            {
                total += counted(&metadata, seen);
            }
        }
    }
    total
}

pub(super) fn used(directories: &[&Path], cgroup: &Path) -> u64 {
    let mut seen = Seen::new();
    directories
        .iter()
        .map(|directory| tree(directory, &mut seen))
        .sum::<u64>()
        + deleted_and_open(cgroup, &mut seen)
}

pub(super) fn limit_bytes(limit_gib: u64) -> u64 {
    limit_gib.saturating_mul(GIB)
}

pub(super) fn overage(used: u64, limit_gib: u64) -> Option<String> {
    (used > limit_bytes(limit_gib)).then(|| {
        format!(
            "velnor: job used {} GiB of disk; the limit is {limit_gib} GiB",
            used / GIB
        )
    })
}
