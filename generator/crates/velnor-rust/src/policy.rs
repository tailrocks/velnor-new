// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{PolicyTools, RepoPath};

use crate::mise::MISE_CONFIGS;
use crate::roots::is_cargo_config;

pub(crate) const DENY_CONFIG: RepoPath = RepoPath::from_static("deny.toml");

const DEPENDENCY_FILES: [&str; 3] = ["Cargo.toml", "Cargo.lock", "deny.toml"];
const IGNORE_FILES: [&str; 2] = [".gitignore", ".ignore"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyRuns {
    pub deny: bool,
    pub machete: bool,
}

#[must_use]
pub fn policy_runs(
    tools: PolicyTools,
    changed: &BTreeSet<RepoPath>,
    everything: bool,
) -> PolicyRuns {
    let dependency = everything
        || changed.iter().any(|path| {
            let name = path.as_str().rsplit('/').next().unwrap_or_default();
            DEPENDENCY_FILES.contains(&name) || is_cargo_config(path)
        });
    let unused_possible = changed.iter().any(|path| {
        let name = path.as_str().rsplit('/').next().unwrap_or_default();
        is_rust_source(path) || MISE_CONFIGS.contains(path) || IGNORE_FILES.contains(&name)
    });
    PolicyRuns {
        deny: tools.deny && dependency,
        machete: tools.machete && (dependency || unused_possible),
    }
}

fn is_rust_source(path: &RepoPath) -> bool {
    std::path::Path::new(path.as_str())
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("rs"))
}
