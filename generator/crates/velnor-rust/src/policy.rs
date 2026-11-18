// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use velnor_model::{PolicyTools, RepoPath, RepoView, RustRoot};

use crate::error::{DetectError, Reason};
use crate::mise::MISE_CONFIGS;
use crate::os_pins::pinned_for_runner;
use crate::roots::is_cargo_config;

const DEPENDENCY_FILES: [&str; 2] = ["Cargo.toml", "Cargo.lock"];
const DENY_TOOL: &str = "cargo-deny";

const DENY_CONFIG_NAMES: [&str; 4] = [
    "deny.toml",
    ".deny.toml",
    ".cargo/deny.toml",
    ".config/deny.toml",
];

pub(crate) fn deny_configs(view: &RepoView, roots: &[RustRoot]) -> BTreeMap<RepoPath, RepoPath> {
    roots
        .iter()
        .filter_map(|root| {
            let mut folder = root.manifest.parent();
            loop {
                for name in DENY_CONFIG_NAMES {
                    let path = folder
                        .as_ref()
                        .map_or_else(
                            || RepoPath::new(name),
                            |folder| RepoPath::new(format!("{folder}/{name}")),
                        )
                        .ok()?;
                    if view.tracked_paths.contains(&path) {
                        return Some((root.manifest.clone(), path));
                    }
                }
                folder = match folder {
                    Some(current) => current.parent(),
                    None => break,
                };
            }
            None
        })
        .collect()
}

pub(crate) fn is_deny_config(path: &RepoPath) -> bool {
    matches!(
        path.as_str().rsplit('/').next(),
        Some("deny.toml" | ".deny.toml")
    )
}

pub(crate) fn policy_tools(
    view: &RepoView,
    deny_configs: &BTreeMap<RepoPath, RepoPath>,
    runner: &str,
) -> Result<PolicyTools, DetectError> {
    let pinned = |tool: &str| pinned_for_runner(&view.files, |name| is_tool(name, tool), runner);
    if let Some(path) = deny_configs.values().next()
        && !pinned(DENY_TOOL)
    {
        return Err(DetectError::invalid(path, Reason::UnpinnedTool(DENY_TOOL)));
    }
    Ok(PolicyTools {
        deny: !deny_configs.is_empty(),
        machete: pinned("cargo-machete"),
    })
}

fn is_tool(name: &str, tool: &str) -> bool {
    name.rsplit(['/', ':']).next() == Some(tool)
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
