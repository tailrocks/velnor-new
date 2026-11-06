// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

pub(super) fn release_pr_write_paths(
    record: &velnor_actions_rust::metadata::WorkspaceRecord,
) -> Vec<String> {
    let join = |dir: &str, file: &str| {
        if dir.is_empty() {
            file.to_owned()
        } else {
            format!("{dir}/{file}")
        }
    };
    let mut paths = BTreeSet::new();
    for name in ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"] {
        paths.insert(join(&record.workspace_root, name));
    }
    for package in record
        .packages
        .iter()
        .filter(|package| package.in_workspace && !package.external)
    {
        paths.insert(package.manifest.clone());
        let dir = package.manifest.rsplit_once('/').map_or("", |(dir, _)| dir);
        paths.insert(join(dir, "CHANGELOG.md"));
    }
    paths.into_iter().collect()
}
