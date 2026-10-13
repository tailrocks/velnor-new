//! Package ownership and reverse-closure selection over base/head graphs.

use std::collections::BTreeSet;

use velnor_actions_rust::{LocalEdge, reverse_closure};

use crate::discover::Discovery;

/// Package IDs owning changed files plus their reverse closure.
///
/// The closure runs over the union of the base and head graphs, so edges
/// removed or renamed at head still select their consumers.
pub(crate) fn affected_packages(
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
pub(crate) fn has_unowned_file(discovery: &Discovery, changed: &BTreeSet<String>) -> bool {
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
pub(crate) fn manifest_dir(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}

#[cfg(test)]
mod tests {
    use velnor_actions_rust::{
        CompileDriver, DepKind, PackageRecord, RustExecutionProfile, TestRunner, WorkspaceRecord,
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
            consumer_manifest_json: None,
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
