// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Which packages a path belongs to: the package directories, the workspace-wide files, and the
//! `stacks.rust.inputs` rules. A package is named by its directory.

use std::collections::{BTreeMap, BTreeSet};

use super::config::InputRule;
use super::units::Unit;

const UPWARD_CONFIGS: [&str; 4] = [
    "clippy.toml",
    ".clippy.toml",
    "rustfmt.toml",
    ".rustfmt.toml",
];

#[derive(Debug)]
pub(super) struct Ownership<'a> {
    owners: Vec<(String, String)>,
    members: Vec<(String, String)>,
    workspaces: Vec<(String, Vec<String>)>,
    rules: Vec<(&'a InputRule, BTreeSet<String>)>,
}

impl<'a> Ownership<'a> {
    /// A rule that names a package no route covers is a configuration the plan cannot follow.
    pub(super) fn new(units: &[Unit], rules: &'a [InputRule]) -> Result<Self, String> {
        let mut owners = Vec::new();
        let mut roots: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for unit in units {
            owners.push((unit.dir.clone(), unit.dir.clone()));
            roots
                .entry(unit.workspace.clone())
                .or_default()
                .push(unit.dir.clone());
        }
        let mut resolved = Vec::new();
        for rule in rules {
            let mut ids = BTreeSet::new();
            for name in &rule.packages {
                let found: Vec<&Unit> = units.iter().filter(|unit| unit.name == *name).collect();
                if found.is_empty() {
                    return Err(format!("unknown_package:{name}"));
                }
                ids.extend(found.into_iter().map(|unit| unit.dir.clone()));
            }
            resolved.push((rule, ids));
        }
        Ok(Self {
            members: owners.clone(),
            owners,
            workspaces: roots.into_iter().collect(),
            rules: resolved,
        })
    }

    pub(super) fn package_of(&self, path: &str) -> Option<&(String, String)> {
        deepest(&self.owners, path)
    }

    pub(super) fn has_claims_below(&self, dir: &str) -> bool {
        let prefix = format!("{dir}/");
        let below = |path: &str| dir.is_empty() || path.starts_with(&prefix);
        dir.is_empty()
            || self.owners.iter().any(|(owner, _)| below(owner))
            || self.workspaces.iter().any(|(root, _)| below(root))
            || self.rules.iter().any(|(rule, _)| {
                rule.globs
                    .iter()
                    .any(|glob| below(glob) || glob.starts_with(['*', '?', '[', '{']))
            })
    }

    pub(super) fn declares_complete_inputs(&self, id: &str) -> bool {
        self.rules
            .iter()
            .any(|(rule, ids)| rule.complete && ids.contains(id))
    }

    pub(super) fn is_owned(&self, path: &str) -> bool {
        if self.owners.iter().all(|(dir, _)| dir.is_empty()) {
            return true;
        }
        self.owners
            .iter()
            .any(|(dir, _)| !dir.is_empty() && owns(dir, path))
            || !self.claim_without_owners(path).is_empty()
    }

    pub(super) fn claim(&self, path: &str) -> BTreeSet<String> {
        let mut claim = self.claim_without_owners(path);
        if let Some((_, id)) = deepest(&self.owners, path) {
            claim.insert(id.clone());
        }
        claim
    }

    fn claim_without_owners(&self, path: &str) -> BTreeSet<String> {
        let mut claim = BTreeSet::new();
        if let Some(scope) = upward_config_scope(path) {
            for (dir, id) in &self.members {
                if scope.is_empty() || owns(scope, dir) {
                    claim.insert(id.clone());
                }
            }
        }
        for (root, ids) in &self.workspaces {
            if is_workspace_scoped(root, path) {
                claim.extend(ids.iter().cloned());
            }
        }
        for (rule, ids) in &self.rules {
            if rule.matches(path) {
                claim.extend(ids.iter().cloned());
            }
        }
        claim
    }
}

pub(super) fn affected(
    ownership: &Ownership<'_>,
    changed: &BTreeSet<String>,
    forced: &BTreeSet<String>,
    edges: &[(String, String)],
) -> BTreeSet<String> {
    let mut selected = forced.clone();
    for path in changed {
        selected.extend(ownership.claim(path));
    }
    let mut reverse: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (from, to) in edges {
        reverse.entry(to).or_default().push(from);
    }
    let mut stack: Vec<String> = selected.iter().cloned().collect();
    while let Some(id) = stack.pop() {
        for dependent in reverse.get(id.as_str()).into_iter().flatten() {
            if selected.insert((*dependent).to_owned()) {
                stack.push((*dependent).to_owned());
            }
        }
    }
    selected
}

pub(super) fn has_unowned_file(ownership: &Ownership<'_>, changed: &BTreeSet<String>) -> bool {
    changed.iter().any(|path| !ownership.is_owned(path))
}

fn upward_config_scope(path: &str) -> Option<&str> {
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    if UPWARD_CONFIGS.contains(&name) {
        return Some(dir);
    }
    if name == "config" || name == "config.toml" {
        let (parent, cargo) = dir.rsplit_once('/').unwrap_or(("", dir));
        if cargo == ".cargo" {
            return Some(parent);
        }
    }
    None
}

fn is_workspace_scoped(root: &str, path: &str) -> bool {
    let prefix = if root.is_empty() {
        String::new()
    } else {
        format!("{root}/")
    };
    path.strip_prefix(&prefix)
        .is_some_and(|rest| rest == "Cargo.toml" || rest == ".config/nextest.toml")
}

fn owns(dir: &str, path: &str) -> bool {
    dir.is_empty() || *path == *dir || path.starts_with(&format!("{dir}/"))
}

fn deepest<'o>(owners: &'o [(String, String)], path: &str) -> Option<&'o (String, String)> {
    let mut best: Option<&(String, String)> = None;
    for owner in owners {
        if owns(&owner.0, path) && best.is_none_or(|current| owner.0.len() > current.0.len()) {
            best = Some(owner);
        }
    }
    best
}
