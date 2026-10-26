// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::config::BunInputRule;
use velnor_actions_contract::matches_glob;

use crate::paths::{is_tsconfig, is_under};
use crate::workspace::{Member, Workspace};

fn is_shared(root: &str, path: &str) -> bool {
    let rel = if root.is_empty() {
        Some(path)
    } else {
        path.strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
    };
    rel.is_some_and(|rel| {
        matches!(rel, "package.json" | "bun.lock" | "bunfig.toml")
            || (is_tsconfig(rel) && !rel.contains('/'))
            || rel.starts_with("patches/")
    })
}

#[must_use]
pub fn affected_members(
    workspaces: &[Workspace],
    changed: &BTreeSet<String>,
    rules: &[BunInputRule],
) -> BTreeSet<String> {
    let mut selected = BTreeSet::new();
    for workspace in workspaces {
        if changed.iter().any(|path| is_shared(&workspace.root, path)) {
            selected.extend(workspace.members.iter().map(|member| member.name.clone()));
        }
    }
    let all: Vec<&Member> = workspaces.iter().flat_map(|w| w.members.iter()).collect();
    for path in changed {
        if let Some(owner) = owner_of(&all, path) {
            selected.insert(owner.name.clone());
        }
        for rule in rules {
            if rule.globs.iter().any(|glob| matches_glob(glob, path)) {
                selected.extend(rule.members.iter().cloned());
            }
        }
    }
    close_over_dependents(&all, selected)
}

fn owner_of<'a>(members: &[&'a Member], path: &str) -> Option<&'a Member> {
    members
        .iter()
        .filter(|member| is_under(path, &member.dir))
        .max_by_key(|member| member.dir.len())
        .copied()
}

fn close_over_dependents(members: &[&Member], mut selected: BTreeSet<String>) -> BTreeSet<String> {
    loop {
        let before = selected.len();
        for member in members {
            if member.depends_on.iter().any(|dep| selected.contains(dep)) {
                selected.insert(member.name.clone());
            }
        }
        if selected.len() == before {
            return selected;
        }
    }
}

#[must_use]
pub fn forced_members(
    reads: &BTreeMap<String, Vec<String>>,
    rules: &[BunInputRule],
) -> BTreeSet<String> {
    reads
        .iter()
        .filter(|(_, outside)| !outside.is_empty())
        .filter(|(name, _)| {
            !rules
                .iter()
                .any(|rule| rule.complete && rule.members.contains(*name))
        })
        .map(|(name, _)| name.clone())
        .collect()
}
