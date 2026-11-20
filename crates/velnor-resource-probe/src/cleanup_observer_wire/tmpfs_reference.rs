// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::HashSet, path::Path};

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
pub fn observer_outside_pair_namespaces(
    observer_namespace: Option<([u32; 2], u64)>,
    protected_namespaces: &[([u32; 2], u64)],
) -> Option<([u32; 2], u64)> {
    observer_namespace.filter(|namespace| !protected_namespaces.contains(namespace))
}

#[must_use]
pub fn observer_runtime_tmpfs_devices(
    observer_namespace: Option<([u32; 2], u64)>,
    self_mountinfo: &[u8],
) -> HashSet<u64> {
    if observer_namespace.is_none() {
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
        if is_observer_runtime_mount(&fields) {
            devices.insert(device);
        }
    }
    devices
}

#[must_use]
pub fn mountinfo_line_has_owned_tmpfs<S: std::hash::BuildHasher>(
    device: u64,
    owned_tmpfs_devices: &HashSet<u64, S>,
    observer_tmpfs_devices: &HashSet<u64, S>,
) -> bool {
    if !owned_tmpfs_devices.contains(&device) {
        return false;
    }
    if observer_tmpfs_devices.contains(&device) {
        return false;
    }
    true
}

#[must_use]
pub fn descriptor_has_owned_tmpfs<S: std::hash::BuildHasher>(
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
    if observer_tmpfs_devices.contains(&dev) {
        return false;
    }
    true
}

fn is_observer_runtime_mount(fields: &[&str]) -> bool {
    let fstype = fields
        .iter()
        .position(|field| *field == "-")
        .and_then(|separator| fields.get(separator + 1).copied());
    fields.get(3).copied() == Some("/")
        && matches!(
            (fstype, fields.get(4).copied()),
            (
                Some("tmpfs"),
                Some("/dev" | "/dev/shm" | "/run" | "/sys/fs/cgroup")
            ) | (Some("mqueue"), Some("/dev/mqueue"))
                | (Some("devpts"), Some("/dev/pts"))
                | (Some("proc"), Some("/proc"))
                | (Some("sysfs"), Some("/sys"))
                | (Some("cgroup2" | "cgroup"), Some("/sys/fs/cgroup"))
                | (Some("overlay"), Some("/"))
        )
}
