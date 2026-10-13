//! Event-time affected-work selection for `plan-v1`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{TaskGroup, reverse_closure};

use crate::discover::Discovery;

/// Select task groups: affected subset on PRs with a base, else all.
pub(crate) fn select_groups<'a>(
    root: &Path,
    event: WorkflowEvent,
    base: Option<&str>,
    head: &str,
    discovery: &'a Discovery,
    warnings: &mut Vec<String>,
) -> Vec<&'a TaskGroup> {
    let all: Vec<&TaskGroup> = discovery.task_groups.iter().collect();
    let narrow = event == WorkflowEvent::PullRequest && base.is_some();
    if !narrow {
        if event == WorkflowEvent::PullRequest {
            warnings.push("missing_base:selecting_all".to_owned());
        }
        return all;
    }
    let changed = match changed_files(root, base.unwrap_or_default(), head) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("affected_diff_failed:{problem}:selecting_all"));
            return all;
        }
    };
    if changed.is_empty() {
        warnings.push("no_affected_files".to_owned());
        return Vec::new();
    }
    if has_unowned_file(discovery, &changed) {
        warnings.push("unclassified_files:selecting_all".to_owned());
        return all;
    }
    let selected_ids = affected_packages(discovery, &changed);
    let mut keys = BTreeSet::new();
    for group in &all {
        if selected_ids.contains(&group.package_id) {
            keys.insert(group.manifest_key.clone());
        }
    }
    all.into_iter()
        .filter(|group| {
            selected_ids.contains(&group.package_id)
                || (group.package_id.is_empty() && keys.contains(&group.manifest_key))
        })
        .collect()
}

/// Files changed between base and head via the allowlisted `diff` verb.
fn changed_files(root: &Path, base: &str, head: &str) -> Result<BTreeSet<String>, String> {
    let range = format!("{base}...{head}");
    let output = GitRequest::diff(vec![OsString::from("--name-only"), OsString::from(range)])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    output
        .require_success("git")
        .map_err(|err| err.to_string())?;
    let text = output.stdout_text("git").map_err(|err| err.to_string())?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

/// Package IDs owning changed files plus their reverse closure.
fn affected_packages(discovery: &Discovery, changed: &BTreeSet<String>) -> BTreeSet<String> {
    let mut owned = BTreeSet::new();
    let mut edges = Vec::new();
    let mut owners: Vec<(String, String)> = Vec::new();
    for workspace in &discovery.workspaces {
        edges.extend(workspace.record.edges.iter().cloned());
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                owners.push((manifest_dir(&package.manifest), package.id.clone()));
            }
        }
    }
    for path in changed {
        if let Some(id) = deepest_owner(&owners, path) {
            owned.insert(id);
        }
    }
    let mut selected = owned.clone();
    selected.extend(reverse_closure(&edges, &edges, &owned));
    selected
}

/// True when any changed file has no owning package.
fn has_unowned_file(discovery: &Discovery, changed: &BTreeSet<String>) -> bool {
    let mut dirs = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                dirs.push(manifest_dir(&package.manifest));
            }
        }
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

/// Directory of a manifest path; empty for the repository root.
fn manifest_dir(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}
