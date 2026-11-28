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

pub(super) fn used(directories: &[&Path], stop: &AtomicBool) -> Usage {
    used_within(directories, tree::MAX_DEPTH, stop)
}

pub(super) fn used_within(directories: &[&Path], depth: usize, stop: &AtomicBool) -> Usage {
    let mut seen: HashSet<tree::Identity> = HashSet::new();
    let mut usage = Usage::default();
    for directory in directories {
        let mut device = None;
        tree::walk(directory, depth, |stat| {
            let identity = tree::identity(stat);
            if *device.get_or_insert(identity.0) != identity.0 {
                usage.foreign = true;
                return false;
            }
            if seen.insert(identity) {
                let blocks = u64::try_from(stat.st_blocks).unwrap_or(0);
                usage.bytes = usage
                    .bytes
                    .saturating_add(blocks.saturating_mul(BLOCK_BYTES));
            }
            !stop.load(Ordering::Relaxed)
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
