//! Affected-member claims (hard decision 3 for the Bun stack).
//!
//! A member is selected when a changed path lies in its directory, in a
//! file every member of its workspace shares (root `package.json`, `bun.lock`,
//! `bunfig.toml`, root `tsconfig*.json`, `patches/**`), or in a declared
//! input; every member that depends on a selected one through `workspace:`
//! is selected with it.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::config::BunInputRule;
use velnor_actions_contract::matches_glob;

use crate::paths::{is_tsconfig, is_under};
use crate::workspace::{Member, Workspace};

/// True for a file whose change affects every member of the workspace at `root`.
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

/// Names of the members affected by `changed` (repository-relative paths).
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

/// The deepest member directory that contains `path`.
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

/// Members that must always run: their configs read outside their directory
/// and no rule naming them declares those reads complete.
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
