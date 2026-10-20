//! Regression tests for changes in a pull request.

use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

use super::{ChangeEvidence, classify_changed};
use crate::discover::{Discovery, PlannedWorkspace};
use velnor_actions_contract::WorkflowEvent;
use velnor_actions_rust::{
    CompileDriver, DepKind, LocalEdge, NextestProfile, PackageRecord, ProfileSource,
    RustExecutionProfile, TestRunner, VelnorV1TaskOwner, WorkspaceRecord,
};

#[test]
fn pull_request_keeps_earlier_changes_and_transitive_dependents() {
    let repo = tempfile::tempdir().expect("temporary repository");
    initialize_repo(repo.path());
    write_workspace(repo.path());
    commit(repo.path(), "baseline", "Make the crate graph.");
    let base = rev(repo.path(), "HEAD");

    write_source(repo.path(), "a", "pub fn changed() {}\n");
    commit(repo.path(), "change-a", "Change crate a.");
    write_source(repo.path(), "b", "pub fn changed() {}\n");
    commit(repo.path(), "change-b", "Change crate b.");
    let head = rev(repo.path(), "HEAD");

    let mut warnings = Vec::new();
    let selected = classify_changed(
        repo.path(),
        WorkflowEvent::PullRequest,
        Some(&base),
        &head,
        &discovery(),
        &mut warnings,
        &mut ChangeEvidence::default(),
    )
    .expect("valid pull request range");

    assert_eq!(selected, ids(&["a", "b", "c", "d"]));
    assert!(!selected.contains("e"), "The plan must not select crate e.");
    assert!(warnings.is_empty(), "The test has warnings: {warnings:?}");
}

fn initialize_repo(root: &Path) {
    let status = crate::git_fixture::sterile_command()
        .arg("init")
        .arg(root)
        .status()
        .expect("Make the test repository.");
    assert!(status.success());
}

fn write_workspace(root: &Path) {
    let members = ["a", "b", "c", "d", "e"];
    let paths = members
        .iter()
        .map(|name| format!("\"crates/{name}\""))
        .collect::<Vec<_>>()
        .join(", ");
    write(
        root,
        "Cargo.toml",
        &format!("[workspace]\nmembers = [{paths}]\n"),
    );
    for name in members {
        write_package(root, name);
    }
    add_path_dependency(root, "c", "a", "../a");
    add_path_dependency(root, "d", "c", "../c");
    for name in members {
        write_source(root, name, "pub fn stable() {}\n");
    }
}

fn write_package(root: &Path, name: &str) {
    let directory = format!("crates/{name}");
    write(
        root,
        &format!("{directory}/Cargo.toml"),
        &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
    );
}

fn add_path_dependency(root: &Path, package: &str, dependency: &str, path: &str) {
    write(
        root,
        &format!("crates/{package}/Cargo.toml"),
        &format!(
            "[package]\nname = \"{package}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{dependency} = {{ path = \"{path}\" }}\n"
        ),
    );
}

fn write_source(root: &Path, package: &str, source: &str) {
    write(root, &format!("crates/{package}/src/lib.rs"), source);
}

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent directory")).expect("create parent");
    std::fs::write(path, contents).expect("write fixture file");
}

fn commit(root: &Path, path: &str, message: &str) {
    let add = git(root).args(["add", "--all"]).status().expect("git add");
    assert!(add.success());
    let commit = git(root)
        .args(["commit", "-m", message])
        .status()
        .expect("git commit");
    assert!(commit.success(), "fixture commit failed: {path}");
}

fn rev(root: &Path, reference: &str) -> String {
    let output = git(root)
        .args(["rev-parse", reference])
        .output()
        .expect("git rev-parse");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("UTF-8 revision")
        .trim()
        .to_owned()
}

fn git(root: &Path) -> Command {
    crate::git_fixture::command(root).expect("fixture Git authority")
}

fn discovery() -> Discovery {
    let packages = ["a", "b", "c", "d", "e"].into_iter().map(package).collect();
    let edge = |from: &str, to: &str| LocalEdge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind: DepKind::Normal,
        optional: false,
        target: None,
    };
    Discovery {
        affected: crate::affected_inputs::AffectedInputs::default(),
        gradle_roots: Vec::new(),
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: vec![PlannedWorkspace {
            record: WorkspaceRecord {
                workspace_root: String::new(),
                members: ["a", "b", "c", "d", "e"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect(),
                packages,
                edges: vec![edge("c", "a"), edge("d", "c")],
                skipped_edges: Vec::new(),
                has_external_dependencies: false,
            },
            profile: RustExecutionProfile {
                compile_driver: CompileDriver::Cargo,
                test_runner: TestRunner::CargoNextest,
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
        feature_fallbacks: Vec::new(),
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
        bun: crate::workflow_task_jobs::bun_inventory::BunInventory::default(),
    }
}

fn package(name: &str) -> PackageRecord {
    PackageRecord {
        id: name.to_owned(),
        name: name.to_owned(),
        version: "0.1.0".to_owned(),
        manifest: format!("crates/{name}/Cargo.toml"),
        external: false,
        in_workspace: true,
        targets: Vec::new(),
        features: Vec::new(),
        has_build_script: false,
        v1_task_owner: VelnorV1TaskOwner::Project,
    }
}

fn ids(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}
