//! Edge and path primitives over discovery output.

use velnor_actions_contract_planning::{DetectedProject, DetectionStatus};
use velnor_actions_orchestrator_discovery::discover::{workspace_lock, workspace_manifest};
use velnor_actions_orchestrator_discovery::select_edges::manifest_dir;
use velnor_actions_orchestrator_discovery::select_tofu::tofu_selected_roots;

#[test]
fn manifest_dir_strips_file_segment() {
    assert_eq!(manifest_dir("crates/foo/Cargo.toml"), "crates/foo");
}

#[test]
fn manifest_dir_of_root_manifest_is_empty() {
    assert_eq!(manifest_dir("Cargo.toml"), "");
}

#[test]
fn manifest_dir_of_bare_filename_is_empty() {
    assert_eq!(manifest_dir("deny.toml"), "");
}

#[test]
fn workspace_manifest_joins_root() {
    assert_eq!(workspace_manifest("crates/foo"), "crates/foo/Cargo.toml");
    assert_eq!(workspace_manifest(""), "Cargo.toml");
}

#[test]
fn workspace_lock_joins_root() {
    assert_eq!(workspace_lock("crates/foo"), "crates/foo/Cargo.lock");
    assert_eq!(workspace_lock(""), "Cargo.lock");
}

fn project(stack: &str, root: &str) -> DetectedProject {
    DetectedProject {
        stack_id: stack.to_owned(),
        project_root: root.to_owned(),
        manifest: format!("{root}/main.tf"),
    }
}

#[test]
fn selected_roots_keep_tofu_only_sorted_deduped() {
    let statuses = vec![
        DetectionStatus::Selected(project("tofu", "stacks/b")),
        DetectionStatus::Selected(project("rust", "")),
        DetectionStatus::Selected(project("tofu", "stacks/a")),
        DetectionStatus::Selected(project("tofu", "stacks/a")),
        DetectionStatus::Ignored {
            project: project("tofu", "stacks/c"),
            reason: "test".to_owned(),
        },
    ];
    assert_eq!(tofu_selected_roots(&statuses), ["stacks/a", "stacks/b"]);
}

#[test]
fn selected_roots_empty_without_selections() {
    assert!(tofu_selected_roots(&[]).is_empty());
    assert!(tofu_selected_roots(&[DetectionStatus::Selected(project("rust", ""))]).is_empty());
}

#[test]
fn selected_roots_keep_repo_root() {
    assert_eq!(
        tofu_selected_roots(&[DetectionStatus::Selected(project("tofu", ""))]),
        [String::new()]
    );
}
