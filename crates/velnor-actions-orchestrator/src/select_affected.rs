//! Package ownership and reverse-closure selection over base/head graphs.

use std::collections::BTreeSet;

use velnor_actions_contract::reverse_closure;

use crate::discover::Discovery;

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

/// Directory of a manifest path; empty for the repository root.
pub(crate) fn manifest_dir(manifest: &str) -> String {
    manifest
        .rsplit_once('/')
        .map_or_else(String::new, |(dir, _)| dir.to_owned())
}

#[cfg(test)]
mod tests {
    use velnor_actions_rust::{
        CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile,
        TestRunner, WorkspaceRecord,
    };

    use super::*;
    use crate::discover::PlannedWorkspace;
    use crate::select::classify_changed;

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
            feature_fallbacks: Vec::new(),
            workspaces: vec![PlannedWorkspace {
                record: WorkspaceRecord {
                    workspace_root: String::new(),
                    members: vec!["a".to_owned(), "b".to_owned()],
                    packages: vec![package("a"), package("b")],
                    edges: Vec::new(),
                    skipped_edges: Vec::new(),
                },
                profile: RustExecutionProfile {
                    compile_driver: CompileDriver::Cargo,
                    test_runner: TestRunner::CargoTest,
                    evidence: Vec::new(),
                    driver_source: ProfileSource::Detected,
                    runner_source: ProfileSource::Detected,
                    nextest_profile: NextestProfile::Default,
                    nextest_config: None,
                    run_ignored: None,
                },
                recommendations: Vec::new(),
                findings: Vec::new(),
            }],
            proposals: Vec::new(),
            tool_checks: Vec::new(),
            clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
                groups: Vec::new(),
                barriers: 0,
            },
            recommendations: Vec::new(),
            consumer_manifest_json: None,
            consumer_manifest_stand_in: false,
            skipped_non_utf8: false,
            tofu_note: None,
            tofu_units: Vec::new(),
        }
    }

    fn edge(from: &str, to: &str) -> (String, String) {
        (from.to_owned(), to.to_owned())
    }

    /// Discovery with the given `(id, manifest)` packages in one workspace.
    fn discovery_with(packages: &[(&str, &str)]) -> Discovery {
        let mut discovery = two_package_discovery();
        discovery.workspaces[0].record.packages = packages
            .iter()
            .map(|(id, manifest)| PackageRecord {
                id: (*id).to_owned(),
                name: (*id).to_owned(),
                version: "0.1.0".to_owned(),
                manifest: (*manifest).to_owned(),
                external: false,
                in_workspace: true,
                targets: Vec::new(),
                features: Vec::new(),
                has_build_script: false,
            })
            .collect();
        discovery.workspaces[0].record.members =
            packages.iter().map(|(id, _)| (*id).to_owned()).collect();
        discovery
    }

    #[test]
    fn root_package_does_not_classify_stray_files() {
        let discovery = discovery_with(&[("root", "Cargo.toml"), ("a", "crates/a/Cargo.toml")]);
        let stray: BTreeSet<String> = ["docs/shared.md".to_owned()].into_iter().collect();
        assert!(
            has_unowned_file(&discovery, &stray),
            "root prefix must not mask stray files"
        );
        let nested: BTreeSet<String> = ["crates/a/src/lib.rs".to_owned()].into_iter().collect();
        assert!(
            !has_unowned_file(&discovery, &nested),
            "nested paths stay classified"
        );
    }

    #[test]
    fn lone_root_package_classifies_everything() {
        let discovery = discovery_with(&[("root", "Cargo.toml")]);
        let changed: BTreeSet<String> = ["docs/shared.md".to_owned()].into_iter().collect();
        assert!(
            !has_unowned_file(&discovery, &changed),
            "one package selects itself either way"
        );
        let selected = affected_packages(&discovery, &changed, &[], &[]);
        assert_eq!(selected, ["root".to_owned()].into_iter().collect());
    }

    #[test]
    fn nested_change_selects_deepest_beside_root() {
        let discovery = discovery_with(&[("root", "Cargo.toml"), ("a", "crates/a/Cargo.toml")]);
        let changed: BTreeSet<String> = ["crates/a/src/lib.rs".to_owned()].into_iter().collect();
        let selected = affected_packages(&discovery, &changed, &[], &[]);
        assert_eq!(selected, ["a".to_owned()].into_iter().collect());
    }

    #[test]
    fn shared_declared_input_selects_every_declarer() {
        use velnor_actions_rust::{TaskGroup, TaskKind};
        let mut discovery = two_package_discovery();
        let group = |package: &str| TaskGroup {
            task_id: format!("stack/rust/{package}/clippy/default"),
            package_id: package.to_owned(),
            package_name: package.to_owned(),
            manifest_key: package.to_owned(),
            kind: TaskKind::Clippy,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            declared_inputs: vec!["docs/shared.md".to_owned()],
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
            run_ignored: None,
            nextest_profile: NextestProfile::Default,
        };
        let mut proposals = Vec::new();
        for package in ["a", "b"] {
            let task =
                velnor_actions_rust::propose_task(&group(package)).expect("fixture proposes");
            task.validate().expect("fixture valid");
            proposals.push(task);
        }
        discovery.proposals = proposals;
        let changed: BTreeSet<String> = ["docs/shared.md".to_owned()].into_iter().collect();
        let selected = affected_packages(&discovery, &changed, &[], &[]);
        assert!(
            selected.contains("a") && selected.contains("b"),
            "both declarers affected: {selected:?}"
        );
    }

    #[test]
    fn union_of_base_and_head_graphs_selects_removed_consumers() {
        let discovery = two_package_discovery();
        let changed: BTreeSet<String> = ["b/src/lib.rs".to_owned()].into_iter().collect();
        let base = vec![edge("a", "b")];
        let head: Vec<(String, String)> = Vec::new();
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

    #[test]
    fn declared_inputs_select_their_package() {
        use velnor_actions_rust::{CompileDriver, NextestProfile, TaskGroup, TaskKind, TestRunner};
        let mut discovery = two_package_discovery();
        let group = TaskGroup {
            task_id: "stack/rust/a/clippy/default".to_owned(),
            package_id: "a".to_owned(),
            package_name: "a".to_owned(),
            manifest_key: "a".to_owned(),
            kind: TaskKind::Clippy,
            configuration: "default".to_owned(),
            features: Vec::new(),
            target: "host".to_owned(),
            gated_by: Vec::new(),
            depends_on: Vec::new(),
            target_flags: Vec::new(),
            no_test_targets: false,
            package_arg: None,
            compile_driver: CompileDriver::Cargo,
            test_runner: TestRunner::CargoTest,
            declared_inputs: vec!["docs/spec.md".to_owned()],
            undeclared_reads: false,
            uses_network: false,
            uses_clock: false,
            uses_random: false,
            run_ignored: None,
            nextest_profile: NextestProfile::Default,
        };
        let task = velnor_actions_rust::propose_task(&group).expect("fixture proposes");
        task.validate().expect("fixture valid");
        discovery.proposals = vec![task];
        let changed: BTreeSet<String> = ["docs/spec.md".to_owned()].into_iter().collect();
        let selected = affected_packages(&discovery, &changed, &[], &[]);
        assert!(
            selected.contains("a"),
            "declared input selects package a: {selected:?}"
        );
    }

    #[test]
    fn skipped_index_names_broaden_with_explicit_tag() {
        use velnor_actions_contract::WorkflowEvent;
        let mut discovery = two_package_discovery();
        discovery.skipped_non_utf8 = true;
        let mut warnings = Vec::new();
        let changed = classify_changed(
            std::path::Path::new("/nonexistent"),
            WorkflowEvent::PullRequest,
            Some("base"),
            "head",
            &discovery,
            &mut warnings,
        );
        assert_eq!(changed, None, "skipped names broaden to all");
        assert!(
            warnings.iter().any(|w| w.contains("non_utf8_path")),
            "explicit tag: {warnings:?}"
        );
    }
}
