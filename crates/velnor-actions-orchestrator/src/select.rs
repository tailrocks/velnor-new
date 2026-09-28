//! Event-time affected-work selection for `plan-v1`.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::WorkflowEvent;
use velnor_actions_mise::GitRequest;
use velnor_actions_rust::{DepKind, LocalEdge, TaskGroup, reverse_closure};

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
            warnings.push("comparison_unavailable:missing_base:selecting_all".to_owned());
        }
        return all;
    }
    let base = base.unwrap_or_default();
    let changed = match changed_files(root, base, head) {
        Ok(files) => files,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
            return all;
        }
    };
    if changed.is_empty() {
        warnings.push("no_affected_files".to_owned());
        return Vec::new();
    }
    if changed.iter().any(|path| path == "Cargo.lock") {
        warnings.push("cargo_lock_changed:selecting_all".to_owned());
        return all;
    }
    if changed.iter().any(|path| is_root_config(path)) {
        warnings.push("root_config_changed:selecting_all".to_owned());
        return all;
    }
    if has_unowned_file(discovery, &changed) {
        warnings.push("unclassified_files:selecting_all".to_owned());
        return all;
    }
    let head_edges = head_edges(discovery);
    let base_edges = match base_edges(root, base, discovery) {
        Ok(edges) => edges,
        Err(problem) => {
            warnings.push(format!("comparison_unavailable:{problem}:selecting_all"));
            return all;
        }
    };
    let selected_ids = affected_packages(discovery, &changed, &base_edges, &head_edges);
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

/// True for root Cargo config paths: root manifest or cargo config.
fn is_root_config(path: &str) -> bool {
    path == "Cargo.toml" || path == ".cargo/config.toml" || path == ".cargo/config"
}

/// Head local-path edges from discovery records.
fn head_edges(discovery: &Discovery) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for workspace in &discovery.workspaces {
        edges.extend(workspace.record.edges.iter().cloned());
    }
    edges
}

/// Base local-path edges from base-revision manifests, in head-id space.
///
/// Each head package manifest is read at `base`; path dependencies resolve
/// to the head package owning the target directory, so removed or renamed
/// edges still select their head consumers. Manifests absent at base are
/// new and contribute nothing; fetch or parse failures are errors.
fn base_edges(root: &Path, base: &str, discovery: &Discovery) -> Result<Vec<LocalEdge>, String> {
    let mut packages: Vec<(String, String)> = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if package.in_workspace && !package.external {
                packages.push((manifest_dir(&package.manifest), package.id.clone()));
            }
        }
    }
    let mut edges = Vec::new();
    for workspace in &discovery.workspaces {
        for package in &workspace.record.packages {
            if !package.in_workspace || package.external {
                continue;
            }
            let Some(text) = base_manifest(root, base, &package.manifest)? else {
                continue;
            };
            edges.extend(manifest_edges(
                &text,
                &package.id,
                &manifest_dir(&package.manifest),
                &packages,
            )?);
        }
    }
    Ok(edges)
}

/// Base content of one manifest; `None` when absent at base.
fn base_manifest(root: &Path, base: &str, manifest: &str) -> Result<Option<String>, String> {
    let spec = format!("{base}:{manifest}");
    let output = GitRequest::show(vec![OsString::from(spec)])
        .run_in(root)
        .map_err(|err| err.to_string())?;
    if let Err(err) = output.require_success("git") {
        let problem = err.to_string();
        if problem.contains("does not exist") {
            return Ok(None);
        }
        return Err(problem);
    }
    output
        .stdout_text("git")
        .map(Some)
        .map_err(|err| err.to_string())
}

/// Path edges of one base manifest, resolved to head package IDs.
fn manifest_edges(
    text: &str,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
) -> Result<Vec<LocalEdge>, String> {
    let document: toml::Table = toml::from_str(text).map_err(|err| err.to_string())?;
    let mut edges = Vec::new();
    for (table, kind) in sections() {
        if let Some(deps) = document.get(table).and_then(toml::Value::as_table) {
            edges.extend(dep_edges(deps, from, dir, packages, kind, None));
        }
    }
    if let Some(targets) = document.get("target").and_then(toml::Value::as_table) {
        for (name, target) in targets {
            let Some(target) = target.as_table() else {
                continue;
            };
            for (table, kind) in sections() {
                if let Some(deps) = target.get(table).and_then(toml::Value::as_table) {
                    edges.extend(dep_edges(deps, from, dir, packages, kind, Some(name)));
                }
            }
        }
    }
    Ok(edges)
}

/// Dependency tables with their edge kinds.
fn sections() -> [(&'static str, DepKind); 3] {
    [
        ("dependencies", DepKind::Normal),
        ("build-dependencies", DepKind::Build),
        ("dev-dependencies", DepKind::Dev),
    ]
}

/// Path-dep edges of one dependency table.
fn dep_edges(
    deps: &toml::Table,
    from: &str,
    dir: &str,
    packages: &[(String, String)],
    kind: DepKind,
    target: Option<&str>,
) -> Vec<LocalEdge> {
    let mut edges = Vec::new();
    for spec in deps.values() {
        let Some(spec) = spec.as_table() else {
            continue;
        };
        let Some(path) = spec.get("path").and_then(toml::Value::as_str) else {
            continue;
        };
        let joined = join_dir(dir, path);
        let Some(to) = packages
            .iter()
            .find(|(owned, _)| *owned == joined)
            .map(|(_, id)| id)
        else {
            continue;
        };
        let optional = spec
            .get("optional")
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        edges.push(LocalEdge {
            from: from.to_owned(),
            to: to.clone(),
            kind,
            optional,
            target: target.map(str::to_owned),
        });
    }
    edges
}

/// Join a manifest directory with a dep path, resolving `.` and `..`.
fn join_dir(dir: &str, path: &str) -> String {
    let mut parts: Vec<&str> = dir.split('/').filter(|seg| !seg.is_empty()).collect();
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
        } else {
            parts.push(seg);
        }
    }
    parts.join("/")
}

/// Package IDs owning changed files plus their reverse closure.
///
/// The closure runs over the union of the base and head graphs, so edges
/// removed or renamed at head still select their consumers.
fn affected_packages(
    discovery: &Discovery,
    changed: &BTreeSet<String>,
    base_edges: &[LocalEdge],
    head_edges: &[LocalEdge],
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
    }
    let mut selected = owned.clone();
    selected.extend(reverse_closure(base_edges, head_edges, &owned));
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

#[cfg(test)]
mod tests {
    use velnor_actions_rust::{
        CompileDriver, PackageRecord, RustExecutionProfile, TestRunner, WorkspaceRecord,
    };

    use super::*;
    use crate::discover::PlannedWorkspace;

    /// Discovery with packages `a` and `b` under `a/` and `b/`.
    fn two_package_discovery() -> Discovery {
        let package = |id: &str| PackageRecord {
            id: id.to_owned(),
            name: id.to_owned(),
            version: "0.1.0".to_owned(),
            manifest: format!("{id}/Cargo.toml"),
            external: false,
            in_workspace: true,
            targets: Vec::new(),
            features: Vec::new(),
            has_build_script: false,
        };
        Discovery {
            statuses: Vec::new(),
            workspaces: vec![PlannedWorkspace {
                record: WorkspaceRecord {
                    workspace_root: String::new(),
                    members: vec!["a".to_owned(), "b".to_owned()],
                    packages: vec![package("a"), package("b")],
                    edges: Vec::new(),
                },
                profile: RustExecutionProfile {
                    compile_driver: CompileDriver::Cargo,
                    test_runner: TestRunner::CargoTest,
                    evidence: Vec::new(),
                },
                recommendations: Vec::new(),
            }],
            task_groups: Vec::new(),
            recommendations: Vec::new(),
        }
    }

    fn edge(from: &str, to: &str) -> LocalEdge {
        LocalEdge {
            from: from.to_owned(),
            to: to.to_owned(),
            kind: DepKind::Normal,
            optional: false,
            target: None,
        }
    }

    #[test]
    fn union_of_base_and_head_graphs_selects_removed_consumers() {
        let discovery = two_package_discovery();
        let changed: BTreeSet<String> = ["b/src/lib.rs".to_owned()].into_iter().collect();
        let base = vec![edge("a", "b")];
        let head: Vec<LocalEdge> = Vec::new();
        let selected = affected_packages(&discovery, &changed, &base, &head);
        assert_eq!(
            selected,
            ["a".to_owned(), "b".to_owned()].into_iter().collect(),
            "base-only edge selects its consumer"
        );
        let selected = affected_packages(&discovery, &changed, &[], &head);
        assert_eq!(
            selected,
            ["b".to_owned()].into_iter().collect(),
            "without the base edge only the owner is selected"
        );
    }
}
