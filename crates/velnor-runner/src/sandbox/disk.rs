// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use super::tree;

const BLOCK_BYTES: u64 = 512;
const GIB: u64 = 1 << 30;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Usage {
    pub(super) bytes: u64,
    pub(super) foreign: bool,
}

pub(super) const MAX_ENTRIES: u64 = 4_000_000;

#[derive(Debug, Clone, Copy)]
pub(super) struct Bounds {
    pub(super) depth: usize,
    pub(super) entries: u64,
}

pub(super) const BOUNDS: Bounds = Bounds {
    depth: tree::MAX_DEPTH,
    entries: MAX_ENTRIES,
};

pub(super) fn used(directories: &[&Path], stop: &AtomicBool) -> Usage {
    used_within(directories, BOUNDS, stop)
}

pub(super) fn used_within(directories: &[&Path], bounds: Bounds, stop: &AtomicBool) -> Usage {
    let mut seen: HashSet<tree::Identity> = HashSet::new();
    let mut usage = Usage::default();
    let mut visited = 0_u64;
    for directory in directories {
        let mut device = None;
        let mut root = true;
        tree::walk(directory, bounds.depth, |stat| {
            let identity = tree::identity(stat);
            if *device.get_or_insert(identity.0) != identity.0 {
                usage.foreign = true;
                return false;
            }
            visited += 1;
            let is_root = std::mem::take(&mut root);
            let new = if is_root || stat.st_nlink > 1 {
                seen.insert(identity)
            } else {
                true
            };
            if is_root && !new {
                return false;
            }
            if new {
                let blocks = u64::try_from(stat.st_blocks).unwrap_or(0);
                usage.bytes = usage
                    .bytes
                    .saturating_add(blocks.saturating_mul(BLOCK_BYTES));
            }
            visited <= bounds.entries && !stop.load(Ordering::Relaxed)
        });
    }
    usage
}

pub(super) fn limit_bytes(limit_gib: u64) -> u64 {
    limit_gib.saturating_mul(GIB)
}

pub(super) fn overage(usage: Usage, limit_gib: u64) -> Option<String> {
    (usage.bytes > limit_bytes(limit_gib)).then(|| {
        format!(
            "velnor: job used {} GiB of disk; the limit is {limit_gib} GiB",
            usage.bytes / GIB
        )
    })
}
