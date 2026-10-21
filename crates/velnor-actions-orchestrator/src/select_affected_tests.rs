//! Ownership and closure cases for affected-work selection.

use velnor_actions_rust::{
    CompileDriver, NextestProfile, PackageRecord, ProfileSource, RustExecutionProfile, TestRunner,
    WorkspaceRecord,
};

use super::*;
use crate::discover::PlannedWorkspace;
use crate::select::{ChangeEvidence, classify_changed};

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
        v1_task_owner: velnor_actions_rust::VelnorV1TaskOwner::Project,
    };
    Discovery {
        gradle_roots: Vec::new(),
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        feature_fallbacks: Vec::new(),
        workspaces: vec![PlannedWorkspace {
            record: WorkspaceRecord {
                workspace_root: String::new(),
                members: vec!["a".to_owned(), "b".to_owned()],
                packages: vec![package("a"), package("b")],
                edges: Vec::new(),
                skipped_edges: Vec::new(),
                has_external_dependencies: false,
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
        affected: crate::affected_inputs::AffectedInputs::default(),
        bun: crate::workflow_task_jobs::bun_inventory::BunInventory::default(),
        dbt: crate::workflow_task_jobs::dbt_inventory::DbtInventory::default(),
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
            v1_task_owner: velnor_actions_rust::VelnorV1TaskOwner::Project,
        })
        .collect();
    discovery.workspaces[0].record.members =
        packages.iter().map(|(id, _)| (*id).to_owned()).collect();
    discovery
}

fn set(paths: &[&str]) -> BTreeSet<String> {
    paths.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn root_package_does_not_classify_stray_files() {
    let discovery = discovery_with(&[("root", "Cargo.toml"), ("a", "crates/a/Cargo.toml")]);
    let ownership = Ownership::new(&discovery);
    assert!(
        has_unowned_file(&ownership, &set(&["docs/shared.md"])),
        "root prefix must not mask stray files"
    );
    assert!(
        !has_unowned_file(&ownership, &set(&["crates/a/src/lib.rs"])),
        "nested paths stay classified"
    );
}

#[test]
fn lone_root_package_classifies_everything() {
    let discovery = discovery_with(&[("root", "Cargo.toml")]);
    let ownership = Ownership::new(&discovery);
    let changed = set(&["docs/shared.md"]);
    assert!(
        !has_unowned_file(&ownership, &changed),
        "one package selects itself either way"
    );
    let selected = affected_packages(&ownership, &changed, &BTreeSet::new(), &[]);
    assert_eq!(selected, set(&["root"]));
}

#[test]
fn nested_change_selects_deepest_beside_root() {
    let discovery = discovery_with(&[("root", "Cargo.toml"), ("a", "crates/a/Cargo.toml")]);
    let ownership = Ownership::new(&discovery);
    let selected = affected_packages(
        &ownership,
        &set(&["crates/a/src/lib.rs"]),
        &BTreeSet::new(),
        &[],
    );
    assert_eq!(selected, set(&["a"]));
}

#[test]
fn declared_input_inside_another_package_selects_both() {
    let mut discovery = two_package_discovery();
    discovery.affected.rules = vec![crate::affected_inputs::InputRule {
        globs: vec!["b/proto/**".to_owned()],
        package_ids: set(&["a"]),
        complete: false,
    }];
    let ownership = Ownership::new(&discovery);
    let selected = affected_packages(
        &ownership,
        &set(&["b/proto/x.proto"]),
        &BTreeSet::new(),
        &[],
    );
    assert_eq!(selected, set(&["a", "b"]), "owner and declarer both run");
}

#[test]
fn declared_input_outside_packages_is_owned() {
    let mut discovery = two_package_discovery();
    discovery.affected.rules = vec![crate::affected_inputs::InputRule {
        globs: vec!["protos/**/*.proto".to_owned()],
        package_ids: set(&["a"]),
        complete: false,
    }];
    let ownership = Ownership::new(&discovery);
    let changed = set(&["protos/v1/x.proto"]);
    assert!(!has_unowned_file(&ownership, &changed));
    assert_eq!(
        affected_packages(&ownership, &changed, &BTreeSet::new(), &[]),
        set(&["a"])
    );
    assert!(
        has_unowned_file(&ownership, &set(&["protos/README.md"])),
        "only matching paths are claimed"
    );
}

#[test]
fn union_of_graphs_selects_removed_consumers() {
    let discovery = two_package_discovery();
    let ownership = Ownership::new(&discovery);
    let changed = set(&["b/src/lib.rs"]);
    let base = vec![edge("a", "b")];
    let head: Vec<(String, String)> = Vec::new();
    let selected = affected_packages(&ownership, &changed, &BTreeSet::new(), &[&base, &head]);
    assert_eq!(
        selected,
        set(&["a", "b"]),
        "base-only edge selects its consumer"
    );
    let selected = affected_packages(&ownership, &changed, &BTreeSet::new(), &[&head]);
    assert_eq!(
        selected,
        set(&["b"]),
        "without the edge only the owner runs"
    );
}

#[test]
fn scoped_tool_configs_select_every_member_below() {
    let discovery = discovery_with(&[
        ("outer", "crates/outer/Cargo.toml"),
        ("inner", "crates/outer/inner/Cargo.toml"),
        ("other", "other/Cargo.toml"),
    ]);
    let ownership = Ownership::new(&discovery);
    for path in [
        "crates/outer/clippy.toml",
        "crates/outer/.rustfmt.toml",
        "crates/outer/.cargo/config.toml",
    ] {
        assert_eq!(
            affected_packages(&ownership, &set(&[path]), &BTreeSet::new(), &[]),
            set(&["inner", "outer"]),
            "{path} applies to nested members too"
        );
    }
    assert!(!has_unowned_file(&ownership, &set(&["crates/clippy.toml"])));
    assert_eq!(
        affected_packages(
            &ownership,
            &set(&["crates/clippy.toml"]),
            &BTreeSet::new(),
            &[]
        ),
        set(&["inner", "outer"])
    );
}

#[test]
fn workspace_root_manifest_and_nextest_config_select_the_workspace() {
    let mut discovery = discovery_with(&[
        ("root", "ws/Cargo.toml"),
        ("a", "ws/a/Cargo.toml"),
        ("x", "x/Cargo.toml"),
    ]);
    discovery.workspaces[0].record.workspace_root = "ws".to_owned();
    let ownership = Ownership::new(&discovery);
    for path in ["ws/Cargo.toml", "ws/.config/nextest.toml"] {
        assert_eq!(
            affected_packages(&ownership, &set(&[path]), &BTreeSet::new(), &[]),
            set(&["a", "root", "x"]),
            "{path}"
        );
    }
}

#[test]
fn symlink_targets_select_the_link_owner() {
    let discovery = two_package_discovery();
    let link = crate::select_symlinks::ResolvedLink {
        link: "a/fixtures".to_owned(),
        chain: vec!["a/fixtures".to_owned(), "b/data/link".to_owned()],
        target: "b/data/real".to_owned(),
    };
    let ownership = Ownership::new(&discovery).with_links(vec![link]);
    assert_eq!(
        affected_packages(
            &ownership,
            &set(&["b/data/real/x.sql"]),
            &BTreeSet::new(),
            &[]
        ),
        set(&["a", "b"])
    );
    assert_eq!(
        affected_packages(&ownership, &set(&["b/data/link"]), &BTreeSet::new(), &[]),
        set(&["a", "b"]),
        "retargeting an intermediate hop selects the reader"
    );
    assert_eq!(
        affected_packages(&ownership, &set(&["b/src/lib.rs"]), &BTreeSet::new(), &[]),
        set(&["b"])
    );
}

#[test]
fn resolved_path_packages_own_their_directory() {
    let mut discovery = two_package_discovery();
    discovery.affected.resolves = vec![crate::affected_inputs::WorkspaceResolve {
        workspace_root: String::new(),
        graph: Ok(velnor_actions_rust::ResolvedGraph {
            edges: vec![edge("a", "lapin"), edge("lapin", "patched")],
            path_packages: vec![velnor_actions_rust::ResolvedPathPackage {
                id: "patched".to_owned(),
                dir: "vendor/patched".to_owned(),
            }],
        }),
    }];
    let ownership = Ownership::new(&discovery);
    let changed = set(&["vendor/patched/src/lib.rs"]);
    assert!(!has_unowned_file(&ownership, &changed));
    let graph = vec![edge("a", "lapin"), edge("lapin", "patched")];
    let selected = affected_packages(&ownership, &changed, &BTreeSet::new(), &[&graph]);
    assert!(
        selected.contains("a"),
        "registry edge reaches the app: {selected:?}"
    );
    assert!(!selected.contains("b"));
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
        &mut ChangeEvidence::default(),
    );
    assert_eq!(changed, None, "skipped names broaden to all");
    assert!(
        warnings.iter().any(|w| w.contains("non_utf8_path")),
        "explicit tag: {warnings:?}"
    );
}

#[test]
fn native_parallel_target_helper_changes_select_the_declared_renderer_owner() {
    let config: toml::Value = toml::from_str(include_str!("../../../.velnor/config.toml"))
        .expect("native repository configuration");
    let rust: velnor_actions_contract::RustStackConfig = config["stacks"]["rust"]
        .clone()
        .try_into()
        .expect("typed native Rust input declarations");
    let owner = "velnor-actions-workflow-renderer";
    let mut discovery = discovery_with(&[
        (owner, "crates/velnor-actions-workflow-renderer/Cargo.toml"),
        ("velnor-actions-cli", "crates/velnor-actions-cli/Cargo.toml"),
        ("unrelated", "crates/unrelated/Cargo.toml"),
    ]);
    discovery.affected.rules =
        crate::affected_inputs::resolve_input_rules(Some(&rust), &discovery.workspaces)
            .expect("configured input packages resolve");
    let ownership = Ownership::new(&discovery);
    for file in [
        "scripts/parallel-targets/seed.py",
        "scripts/parallel-targets/test_seed.py",
    ] {
        let changed = set(&[file]);
        assert!(
            !has_unowned_file(&ownership, &changed),
            "declared native input: {file}"
        );
        assert_eq!(
            affected_packages(&ownership, &changed, &BTreeSet::new(), &[]),
            set(&[owner])
        );
    }
    assert!(
        has_unowned_file(&ownership, &set(&["scripts/unrelated.py"])),
        "no broad scripts ownership or ignore filter"
    );
}
