// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, path::Path};

use super::CleanupObserverBinding;

const TMPFS_MAGIC: i64 = 0x0102_1994;

#[must_use]
pub const fn linux_device(major: u64, minor: u64) -> u64 {
    (minor & 0xff) | ((major & 0xfff) << 8) | ((minor & !0xff) << 12) | ((major & !0xfff) << 32)
}

#[must_use]
pub fn parse_mountinfo_device(value: &str) -> Option<u64> {
    let (major, minor) = value.split_once(':')?;
    Some(linux_device(major.parse().ok()?, minor.parse().ok()?))
}

#[must_use]
pub fn observer_runtime_tmpfs_devices(
    observer_pid: u32,
    observer_namespace: Option<([u32; 2], u64)>,
    binding: &CleanupObserverBinding,
    self_mountinfo: &[u8],
) -> HashSet<u64> {
    let Some(namespace) = observer_namespace else {
        return HashSet::new();
    };
    let protected_namespaces = [
        (
            binding.daemon.namespace_device,
            binding.daemon.namespace_inode,
        ),
        (
            binding.runner.namespace_device,
            binding.runner.namespace_inode,
        ),
        (binding.dind.namespace_device, binding.dind.namespace_inode),
    ];
    if observer_pid == 0
        || observer_pid == binding.daemon.pid
        || observer_pid == binding.runner.pid
        || observer_pid == binding.dind.pid
        || protected_namespaces.contains(&namespace)
    {
        return HashSet::new();
    }
    let Ok(text) = std::str::from_utf8(self_mountinfo) else {
        return HashSet::new();
    };
    let mut devices = HashSet::new();
    for line in text.lines() {
        let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
        let Some(device) = fields
            .get(2)
            .and_then(|value| parse_mountinfo_device(value))
        else {
            continue;
        };
        if is_observer_runtime_tmpfs_mount(&fields) {
            devices.insert(device);
        }
    }
    devices
}

#[must_use]
pub fn mountinfo_line_has_owned_tmpfs<S: std::hash::BuildHasher>(
    fields: &[&str],
    device: u64,
    is_observer_task: bool,
    owned_tmpfs_devices: &HashSet<u64, S>,
    observer_tmpfs_devices: &HashSet<u64, S>,
) -> bool {
    if !owned_tmpfs_devices.contains(&device) {
        return false;
    }
    if is_observer_task && observer_tmpfs_devices.contains(&device) && is_tmpfs_mount(fields) {
        return false;
    }
    true
}

#[must_use]
pub fn descriptor_has_owned_tmpfs<S: std::hash::BuildHasher>(
    is_observer_task: bool,
    filesystem_magic: i64,
    device: [u32; 2],
    _target: &Path,
    owned_tmpfs_devices: &HashSet<u64, S>,
    observer_tmpfs_devices: &HashSet<u64, S>,
) -> bool {
    if filesystem_magic != TMPFS_MAGIC {
        return false;
    }
    let dev = linux_device(u64::from(device[0]), u64::from(device[1]));
    if !owned_tmpfs_devices.contains(&dev) {
        return false;
    }
    if is_observer_task && observer_tmpfs_devices.contains(&dev) {
        return false;
    }
    true
}

fn is_tmpfs_mount(fields: &[&str]) -> bool {
    let fstype = fields
        .iter()
        .position(|field| *field == "-")
        .and_then(|separator| fields.get(separator + 1).copied());
    fstype == Some("tmpfs")
}

fn is_observer_runtime_tmpfs_mount(fields: &[&str]) -> bool {
    is_tmpfs_mount(fields)
        && fields.get(3).copied() == Some("/")
        && matches!(
            fields.get(4).copied(),
            Some("/dev" | "/dev/shm" | "/run" | "/sys/fs/cgroup")
        )
}
