//! Package ownership and reverse-closure selection over base/head graphs.

use std::collections::BTreeSet;

use velnor_actions_contract_planning::reverse_closure;

use velnor_actions_orchestrator_discovery::discover::Discovery;
use velnor_actions_orchestrator_discovery::select_edges::manifest_dir;

/// Package IDs owning changed files plus their reverse closure.
///
/// The closure runs over the union of the base and head graphs, so edges
/// removed or renamed at head still select their consumers. Graphs arrive
/// as neutral `(from, to)` pairs converted at the owning adapter boundary.
pub(crate) fn affected_packages(
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    base_edges: &[(String, String)],
    head_edges: &[(String, String)],
) -> BTreeSet<String> {
    let mut owners: Vec<(String, String)> = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                owners.push((manifest_dir(&package.manifest), package.id.clone()));
            }
        }
    }
    let mut owned = BTreeSet::new();
    for path in changed {
        if let Some(id) = deepest_owner(&owners, path) {
            owned.insert(id);
        }
        owned.extend(declared_owners(discovery, path));
    }
    let mut selected = owned.clone();
    selected.extend(reverse_closure(base_edges, head_edges, &owned));
    selected
}

/// Packages whose tasks declare `path` as an input (PAR-4.11).
///
/// Every declarer is affected: returning only the first would silently
/// drop cross-package consumers of one shared input.
fn declared_owners(discovery: &Discovery, path: &str) -> Vec<String> {
    discovery
        .proposals
        .iter()
        .filter(|task| {
            task.identity
                .declared_inputs
                .iter()
                .any(|input| input == path)
        })
        .map(|task| task.identity.unit_id.clone())
        .collect()
}

/// True when any changed file has no owning package.
///
/// Only nested manifest directories classify: the root package (empty
/// directory) owns every path by prefix, so counting it would mask
/// stray files and narrow selection to the root instead of broadening.
/// With no nested packages every path is trivially classified —
/// broadening and narrowing select the same single package.
pub(crate) fn has_unowned_file(discovery: &Discovery, changed: &BTreeSet<String>) -> bool {
    let mut dirs = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                let dir = manifest_dir(&package.manifest);
                if !dir.is_empty() {
                    dirs.push(dir);
                }
            }
        }
    }
    if dirs.is_empty() {
        return false;
    }
    changed
        .iter()
        .any(|path| !dirs.iter().any(|dir| owns(dir, path)))
}

/// True when manifest directory `dir` owns `path`.
fn owns(dir: &str, path: &str) -> bool {
    dir.is_empty() || *path == *dir || path.starts_with(&format!("{dir}/"))
}

/// Deepest manifest directory owning `path`; the root package owns the rest.
fn deepest_owner(owners: &[(String, String)], path: &str) -> Option<String> {
    let mut best: Option<&(String, String)> = None;
    for owner in owners {
        if owns(&owner.0, path) && best.is_none_or(|current| owner.0.len() > current.0.len()) {
            best = Some(owner);
        }
    }
    best.map(|owner| owner.1.clone())
}

#[cfg(test)]
mod tests;
