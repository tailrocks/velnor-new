// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use velnor_model::{PolicyTools, RepoPath};

use crate::mise::MISE_CONFIGS;
use crate::roots::is_cargo_config;

const DEPENDENCY_FILES: [&str; 2] = ["Cargo.toml", "Cargo.lock"];

pub(crate) const DENY_CONFIG_NAMES: [&str; 4] = [
    "deny.toml",
    ".deny.toml",
    ".cargo/deny.toml",
    ".config/deny.toml",
];

pub(crate) fn is_deny_config(path: &RepoPath) -> bool {
    matches!(
        path.as_str().rsplit('/').next(),
        Some("deny.toml" | ".deny.toml")
    )
}
const DENY_EXCEPTION_FILES: [&str; 2] = ["deny.exceptions.toml", ".deny.exceptions.toml"];
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
    selected: bool,
) -> PolicyRuns {
    let dependency = everything
        || changed.iter().any(|path| {
            let name = path.as_str().rsplit('/').next().unwrap_or_default();
            DEPENDENCY_FILES.contains(&name) || is_deny_config(path) || is_cargo_config(path)
        });
    let deny_only = changed.iter().any(|path| {
        let name = path.as_str().rsplit('/').next().unwrap_or_default();
        license_text(name) || DENY_EXCEPTION_FILES.contains(&name)
    });
    let unused_possible = changed.iter().any(|path| {
        let name = path.as_str().rsplit('/').next().unwrap_or_default();
        (selected && rust_source(path))
            || MISE_CONFIGS.contains(path)
            || IGNORE_FILES.contains(&name)
    });
    PolicyRuns {
        deny: (tools.deny || changed.iter().any(is_deny_config)) && (dependency || deny_only),
        machete: tools.machete && (dependency || unused_possible),
    }
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
