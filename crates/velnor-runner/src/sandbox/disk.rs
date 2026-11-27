// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::tree;

const BLOCK_BYTES: u64 = 512;
const GIB: u64 = 1 << 30;
const HELD_DIRECTORIES: [&str; 2] = ["fd", "map_files"];

type Seen = HashSet<tree::Identity>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Usage {
    pub(super) bytes: u64,
    pub(super) hidden: bool,
    pub(super) foreign: bool,
}

struct Tally<'a> {
    seen: Seen,
    usage: Usage,
    stop: &'a AtomicBool,
}

impl Tally<'_> {
    fn count(&mut self, identity: tree::Identity, blocks: u64) {
        if self.seen.insert(identity) {
            self.usage.bytes = self.usage.bytes.saturating_add(blocks * BLOCK_BYTES);
        }
    }

    fn tree(&mut self, root: &Path) {
        let stop = self.stop;
        let mut device = None;
        let outcome = tree::walk(root, false, |stat| {
            if *device.get_or_insert(tree::identity(stat).0) != tree::identity(stat).0 {
                self.usage.foreign = true;
                return false;
            }
            self.count(
                tree::identity(stat),
                u64::try_from(stat.st_blocks).unwrap_or(0),
            );
            !stop.load(Ordering::Relaxed)
        });
        if outcome.is_err() {
            self.usage.hidden = true;
        }
    }

    fn held(&mut self, process: &Path) {
        for directory in HELD_DIRECTORIES {
            let Ok(entries) = std::fs::read_dir(process.join(directory)) else {
                continue;
            };
            for entry in entries.flatten() {
                let Ok(target) = std::fs::read_link(entry.path()) else {
                    continue;
                };
                if !target.to_string_lossy().ends_with(" (deleted)") {
                    continue;
                }
                match std::fs::metadata(entry.path()) {
                    Ok(metadata) if metadata.is_file() && metadata.nlink() == 0 => {
                        self.count(
                            (i128::from(metadata.dev()), i128::from(metadata.ino())),
                            metadata.blocks(),
                        );
                    }
                    Err(_)
                        if target.is_absolute()
                            && !target.to_string_lossy().starts_with("/memfd:") =>
                    {
                        self.usage.hidden = true;
                    }
                    _ => {}
                }
            }
        }
    }

    fn deleted_and_open(&mut self, cgroup: &Path) {
        let Ok(members) = std::fs::read_to_string(cgroup.join("cgroup.procs")) else {
            return;
        };
        for pid in members.lines() {
            self.held(&Path::new("/proc").join(pid));
        }
    }
}

pub(super) fn used(directories: &[&Path], cgroup: &Path, stop: &AtomicBool) -> Usage {
    let mut tally = Tally {
        seen: Seen::new(),
        usage: Usage::default(),
        stop,
    };
    for directory in directories {
        tally.tree(directory);
    }
    tally.deleted_and_open(cgroup);
    tally.usage
}

pub(super) fn limit_bytes(limit_gib: u64) -> u64 {
    limit_gib.saturating_mul(GIB)
}

pub(super) fn overage(usage: Usage, limit_gib: u64) -> Option<String> {
    let gib = usage.bytes / GIB;
    if usage.bytes > limit_bytes(limit_gib) {
        Some(format!(
            "velnor: job used {gib} GiB of disk; the limit is {limit_gib} GiB"
        ))
    } else {
        usage.hidden.then(|| {
            format!(
                "velnor: job used {gib} GiB of disk and hid files from its accounting; the limit is {limit_gib} GiB"
            )
        })
    }
}
