// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use velnor_model::{PolicyTools, RepoPath};

pub(crate) const DENY_CONFIG: RepoPath = RepoPath::from_static("deny.toml");

const DEPENDENCY_FILES: [&str; 3] = ["Cargo.toml", "Cargo.lock", "deny.toml"];

#[must_use]
pub fn policy_needed(changed: &BTreeSet<RepoPath>, everything: bool, tools: PolicyTools) -> bool {
    everything
        || changed.iter().any(|path| {
            let name = path.as_str().rsplit('/').next().unwrap_or_default();
            DEPENDENCY_FILES.contains(&name)
                || (tools.machete && rust_source(path))
                || (tools.deny && license_text(name))
        })
}

fn rust_source(path: &RepoPath) -> bool {
    Path::new(path.as_str())
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}

fn license_text(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    name.starts_with("LICENSE") || name.starts_with("LICENCE")
}
