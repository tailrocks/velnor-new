// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::WorkflowPolicy;
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::{assert_required_owner_routes, owner_feature_repo};

#[test]
fn prepare_and_render_apply_clippy_all_features_only_to_clippy_tasks() -> Result<(), Box<dyn Error>>
{
    let repo = feature_toggle_repo(false, false)?;
    let root = repo.path();
    let default = prepare(root)?;
    let default_rendered = render_staged_tree(&default)?;
    assert_feature_toggle_render_routes(&default, &default_rendered)?;
    assert_cargo_test_profile(&default);

    write_feature_toggle_config(root, true, false)?;
    let enabled = prepare(root)?;
    let enabled_rendered = render_staged_tree(&enabled)?;
    assert_feature_toggle_render_routes(&enabled, &enabled_rendered)?;
    assert_cargo_test_profile(&enabled);
    assert_clippy_all_features_transition(&default, &enabled)?;
    Ok(())
}

fn assert_clippy_all_features_transition(
    default: &GenerationPreparation,
    enabled: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    assert_clippy_feature_toggle_config(default, false)?;
    assert_clippy_feature_toggle_config(enabled, true)?;
    let default_tasks = feature_toggle_tasks(default);
    let enabled_tasks = feature_toggle_tasks(enabled);
    assert_eq!(
        default_tasks.keys().collect::<BTreeSet<_>>(),
        enabled_tasks.keys().collect()
    );
    let mut clippy_count = 0;
    for (task_id, enabled_task) in enabled_tasks {
        let default_task = default_tasks
            .get(task_id)
            .ok_or("default Rust task is missing")?;
        assert_eq!(enabled_task.task_kind, default_task.task_kind);
        assert_eq!(
            enabled_task.identity.unit_path,
            default_task.identity.unit_path
        );
        if enabled_task.task_kind == "clippy" {
            clippy_count += 1;
            assert_eq!(feature_flag_count(&enabled_task.payload), 1, "{task_id}");
            assert_eq!(feature_flag_count(&default_task.payload), 0, "{task_id}");
            let mut enabled_payload = enabled_task.payload.clone();
            enabled_payload.retain(|argument| argument != "--all-features");
            assert_eq!(enabled_payload, default_task.payload, "{task_id}");
            let mut enabled_identity = enabled_task.identity.clone();
            assert_eq!(feature_flag_count_strings(&enabled_identity.flags), 1);
            enabled_identity
                .flags
                .retain(|flag| flag != "--all-features");
            assert_eq!(enabled_identity, default_task.identity, "{task_id}");
        } else {
            assert_eq!(enabled_task.identity, default_task.identity, "{task_id}");
            assert_eq!(enabled_task.payload, default_task.payload, "{task_id}");
            assert_eq!(enabled_task, *default_task, "{task_id}");
        }
    }
    assert!(clippy_count > 0);
    assert_package_task_kind_count(&feature_toggle_tasks(enabled), "clippy", 1);
    assert_package_task_kind_count(&feature_toggle_tasks(enabled), "fmt", 1);
    Ok(())
}

fn assert_clippy_feature_toggle_config(
    prepared: &GenerationPreparation,
    expected_clippy_all_features: bool,
) -> Result<(), Box<dyn Error>> {
    let rust = prepared
        .config
        .stacks
        .rust
        .as_ref()
        .ok_or("Rust config missing")?;
    assert_eq!(rust.clippy_all_features, expected_clippy_all_features);
    assert_eq!(rust.configurations.len(), 1);
    let configuration = &rust.configurations[0];
    assert_eq!(configuration.name, "feature-toggle");
    assert_eq!(configuration.features, vec!["default".to_owned()]);
    assert!(!configuration.all_features);
    Ok(())
}

#[test]
fn prepare_and_render_select_each_package_feature_set_when_all_features()
-> Result<(), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, true)?;
    let prepared = prepare(repo.path())?;
    let rendered = render_staged_tree(&prepared)?;
    assert_feature_toggle_render_routes(&prepared, &rendered)?;
    assert_cargo_test_profile(&prepared);
    assert_all_features_package_proposals(&prepared)?;
    Ok(())
}

fn assert_all_features_package_proposals(
    prepared: &GenerationPreparation,
) -> Result<(), Box<dyn Error>> {
    let rust = prepared
        .config
        .stacks
        .rust
        .as_ref()
        .ok_or("Rust config missing")?;
    assert_eq!(rust.configurations.len(), 1);
    let configuration = &rust.configurations[0];
    assert!(configuration.all_features);
    assert_eq!(configuration.features, Vec::<String>::new());
    assert_eq!(configuration.name, "feature-toggle");
    assert!(!rust.clippy_all_features);
    let workspace = prepared
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root.is_empty())
        .ok_or("root Cargo workspace missing")?;
    let expected = BTreeMap::from([
        (
            "Cargo.toml",
            vec!["default".to_owned(), "demo-only".to_owned()],
        ),
        (
            "crates/maintenance/Cargo.toml",
            vec!["default".to_owned(), "maintenance-only".to_owned()],
        ),
    ]);
    for (manifest, features) in &expected {
        let package = workspace
            .record
            .packages
            .iter()
            .find(|package| package.manifest == *manifest)
            .ok_or("expected Cargo package missing")?;
        let expected_metadata_features =
            features.iter().map(String::as_str).collect::<BTreeSet<_>>();
        assert_eq!(
            package
                .features
                .iter()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            expected_metadata_features
        );
    }
    let mut observed = BTreeSet::new();
    for task in &prepared.discovery.proposals {
        if task.stack_id != "rust" || task.identity.unit_id.is_empty() {
            continue;
        }
        let manifest = task.identity.unit_path.as_str();
        let expected_features = expected.get(manifest).ok_or("unknown Cargo manifest")?;
        assert_eq!(task.identity.features, *expected_features, "{manifest}");
        assert_eq!(task.configuration, configuration.name, "{manifest}");
        assert!(
            task.task_id.ends_with("/feature-toggle"),
            "{manifest}: {}",
            task.task_id
        );
        assert_eq!(
            feature_flag_count_strings(&task.identity.flags),
            1,
            "{manifest}"
        );
        let expected_payload_flags = usize::from(task.task_kind != "fmt");
        assert_eq!(
            feature_flag_count(&task.payload),
            expected_payload_flags,
            "{} {manifest}",
            task.task_kind
        );
        observed.insert(manifest);
    }
    assert_eq!(observed, expected.keys().copied().collect());
    assert_package_task_kind_count(&feature_toggle_tasks(prepared), "clippy", 1);
    assert_package_task_kind_count(&feature_toggle_tasks(prepared), "fmt", 1);
    Ok(())
}

fn feature_toggle_repo(
    clippy_all_features: bool,
    all_features: bool,
) -> Result<TempDir, Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::ConsumerV1,
        false,
        "feature-toggle",
        "default",
    )?;
    let root = repo.path();
    remove_feature_toggle_nextest_task(root)?;
    remove_feature_toggle_nextest_task(&root.join("crates/maintenance"))?;
    fs::write(
        root.join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"maintenance\"\nversion = \"0.1.0\"\n",
    )?;
    let manifest_path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)?;
    let workspace = "\n[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n";
    assert_eq!(manifest.matches(workspace).count(), 1);
    let features = "\n[features]\ndefault = []\ndemo-only = []\n";
    let replacement = format!("{features}{workspace}");
    let manifest = manifest.replacen(workspace, &replacement, 1);
    assert!(manifest.contains("demo-only = []"));
    fs::write(manifest_path, manifest)?;
    let maintenance_manifest = root.join("crates/maintenance/Cargo.toml");
    let manifest = fs::read_to_string(&maintenance_manifest)?;
    let maintenance_features = "[features]\nmaintenance-only = []";
    assert_eq!(manifest.matches(maintenance_features).count(), 1);
    let manifest = manifest.replacen(
        maintenance_features,
        "[features]\ndefault = []\nmaintenance-only = []",
        1,
    );
    assert!(manifest.contains("default = []"));
    fs::write(maintenance_manifest, manifest)?;
    write_feature_toggle_config(root, clippy_all_features, all_features)?;
    Ok(repo)
}

fn remove_feature_toggle_nextest_task(root: &Path) -> Result<(), Box<dyn Error>> {
    let task = root.join(".mise/tasks/test");
    assert_eq!(
        fs::read_to_string(&task)?,
        "#!/bin/sh\ncargo nextest run --locked\n"
    );
    fs::remove_file(task)?;
    Ok(())
}

fn write_feature_toggle_config(
    root: &Path,
    clippy_all_features: bool,
    all_features: bool,
) -> Result<(), Box<dyn Error>> {
    let clippy_setting = if clippy_all_features {
        "clippy_all_features = true\n"
    } else {
        ""
    };
    let features = if all_features { "[]" } else { "[\"default\"]" };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[stacks.rust]\n{clippy_setting}\n[[stacks.rust.configurations]]\nname = \"feature-toggle\"\nfeatures = {features}\nall_features = {all_features}\ntarget = \"host\"\n"
    );
    fs::write(root.join(".velnor/config.toml"), config)?;
    Ok(())
}

fn assert_feature_toggle_render_routes(
    prepared: &GenerationPreparation,
    rendered: &velnor_actions_workflow_renderer::tree::RenderedTree,
) -> Result<(), Box<dyn Error>> {
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    let proposal_paths = feature_toggle_task_paths(prepared);
    assert_required_owner_routes(prepared, workflow, &proposal_paths, true)
}

fn feature_toggle_task_paths(prepared: &GenerationPreparation) -> BTreeMap<String, String> {
    prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust"
                && task.configuration == "feature-toggle"
                && !task.no_targets
                && !task.identity.unit_id.is_empty()
        })
        .map(|task| (task.task_id.clone(), task.identity.unit_path.clone()))
        .collect()
}

fn feature_toggle_tasks(
    prepared: &GenerationPreparation,
) -> BTreeMap<&str, &velnor_actions_contract::ProposedTask> {
    prepared
        .discovery
        .proposals
        .iter()
        .filter(|task| {
            task.stack_id == "rust"
                && task.configuration == "feature-toggle"
                && !task.identity.unit_id.is_empty()
        })
        .map(|task| (task.task_id.as_str(), task))
        .collect()
}

fn assert_package_task_kind_count(
    tasks: &BTreeMap<&str, &velnor_actions_contract::ProposedTask>,
    task_kind: &str,
    expected_count: usize,
) {
    for manifest in ["Cargo.toml", "crates/maintenance/Cargo.toml"] {
        assert_eq!(
            tasks
                .values()
                .filter(|task| {
                    task.task_kind == task_kind && task.identity.unit_path == manifest
                })
                .count(),
            expected_count,
            "{task_kind} tasks for {manifest}"
        );
    }
}

fn assert_cargo_test_profile(prepared: &GenerationPreparation) {
    assert!(
        prepared
            .discovery
            .workspaces
            .iter()
            .all(|workspace| { workspace.profile.test_runner.as_str() == "cargo_test" })
    );
    assert!(prepared.discovery.proposals.iter().any(|task| {
        task.stack_id == "rust"
            && task.task_kind == "test"
            && task.identity.test_runner == "cargo_test"
    }));
}

fn feature_flag_count(arguments: &[std::ffi::OsString]) -> usize {
    arguments
        .iter()
        .filter(|argument| *argument == "--all-features")
        .count()
}

fn feature_flag_count_strings(arguments: &[String]) -> usize {
    arguments
        .iter()
        .filter(|argument| argument.as_str() == "--all-features")
        .count()
}
