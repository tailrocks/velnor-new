// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use serde_json::Value;

use crate::support::Harness;

pub(super) fn keys(list: &Value) -> Vec<String> {
    list["actions_caches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["key"].as_str().map(str::to_owned))
        .collect()
}

pub(super) fn mode(path: &std::path::Path, mode: u32) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

pub(super) fn archives(harness: &Harness) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = std::fs::read_dir(harness.dir().join("archives"))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    paths.sort();
    paths
}
