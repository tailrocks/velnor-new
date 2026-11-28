// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::GEN_ROOT;

const BTRFS_SYSFS: &str = "/sys/fs/btrfs";
const MOUNTINFO: &str = "/proc/self/mountinfo";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Mount {
    pub(super) device: String,
    pub(super) subvolume: u64,
}

pub(super) fn mount_of(mountinfo: &str, path: &Path) -> Option<Mount> {
    mountinfo
        .lines()
        .filter_map(|line| {
            let (before, after) = line.split_once(" - ")?;
            let mount_point = before.split_whitespace().nth(4)?;
            let mut tail = after.split_whitespace();
            let (kind, device, options) = (tail.next()?, tail.next()?, tail.next()?);
            let subvolume = options
                .split(',')
                .find_map(|option| option.strip_prefix("subvolid="))?
                .parse()
                .ok()?;
            (kind == "btrfs" && path.starts_with(mount_point)).then(|| {
                (
                    mount_point.len(),
                    Mount {
                        device: device.to_owned(),
                        subvolume,
                    },
                )
            })
        })
        .max_by_key(|(length, _)| *length)
        .map(|(_, mount)| mount)
}

pub(super) fn free_bytes_of(
    referenced: &str,
    reserved_data: &str,
    max_referenced: &str,
) -> Option<u64> {
    let referenced: u64 = referenced.trim().parse().ok()?;
    let reserved: u64 = reserved_data.trim().parse().ok()?;
    let limit: u64 = max_referenced
        .trim()
        .parse()
        .ok()
        .filter(|limit| *limit > 0)?;
    Some(limit.saturating_sub(referenced.saturating_add(reserved)))
}

fn filesystem_of(device: &str) -> Option<String> {
    let name = Path::new(device).file_name()?.to_str()?.to_owned();
    std::fs::read_dir(BTRFS_SYSFS)
        .ok()?
        .flatten()
        .find(|entry| entry.path().join("devices").join(&name).exists())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
}

pub(super) fn free_bytes() -> Option<u64> {
    let mountinfo = std::fs::read_to_string(MOUNTINFO).ok()?;
    let mount = mount_of(&mountinfo, Path::new(GEN_ROOT))?;
    let group = Path::new(BTRFS_SYSFS)
        .join(filesystem_of(&mount.device)?)
        .join("qgroups")
        .join(format!("0_{}", mount.subvolume));
    free_bytes_of(
        &std::fs::read_to_string(group.join("referenced")).ok()?,
        &std::fs::read_to_string(group.join("rsv_data")).ok()?,
        &std::fs::read_to_string(group.join("max_referenced")).ok()?,
    )
}
