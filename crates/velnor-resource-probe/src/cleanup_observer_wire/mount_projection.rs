// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashSet;

use super::{CleanupObserverBinding, tmpfs_reference::parse_mountinfo_device};

#[must_use]
pub fn owned_mountinfo_projection(input: &[u8], binding: &CleanupObserverBinding) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(input) else {
        return input.to_vec();
    };
    let devices = owned_devices(binding);
    let namespaces = [
        binding.daemon.namespace_inode,
        binding.runner.namespace_inode,
        binding.dind.namespace_inode,
    ];
    let mut projection = Vec::new();
    for line in text.lines() {
        if line_is_owned(line, &devices, &namespaces) {
            projection.extend_from_slice(line.as_bytes());
            projection.push(b'\n');
        }
    }
    projection
}

fn owned_devices(binding: &CleanupObserverBinding) -> HashSet<u64> {
    binding
        .volume_mountpoints
        .iter()
        .map(|volume| volume.mount_device)
        .chain(binding.owned_tmpfs_devices)
        .map(|device| {
            super::tmpfs_reference::linux_device(u64::from(device[0]), u64::from(device[1]))
        })
        .collect()
}

fn line_is_owned(line: &str, devices: &HashSet<u64>, namespaces: &[u64; 3]) -> bool {
    let fields = line.split_ascii_whitespace().collect::<Vec<_>>();
    let Some(device) = fields
        .get(2)
        .and_then(|value| parse_mountinfo_device(value))
    else {
        return true;
    };
    if devices.contains(&device) {
        return true;
    }
    let is_nsfs = fields
        .iter()
        .position(|field| *field == "-")
        .and_then(|separator| fields.get(separator + 1).copied())
        == Some("nsfs");
    is_nsfs
        && fields
            .get(3)
            .and_then(|root| {
                root.split_once('[')?
                    .1
                    .strip_suffix(']')?
                    .parse::<u64>()
                    .ok()
            })
            .is_none_or(|inode| namespaces.contains(&inode))
}
