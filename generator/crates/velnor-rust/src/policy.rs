// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::RepoPath;

pub(crate) const DENY_CONFIG: RepoPath = RepoPath::from_static("deny.toml");

const DEPENDENCY_FILES: [&str; 3] = ["Cargo.toml", "Cargo.lock", "deny.toml"];

#[must_use]
pub fn policy_needed(changed: &BTreeSet<RepoPath>, everything: bool) -> bool {
    everything
        || changed.iter().any(|path| {
            let name = path.as_str().rsplit('/').next().unwrap_or_default();
            DEPENDENCY_FILES.contains(&name)
        })
}
