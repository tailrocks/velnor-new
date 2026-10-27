// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::path::Path;
use std::{fs, io};

use tempfile::TempDir;
use velnor_actions_contract::{Job, REQUIRED_JOB_ID, WorkflowPolicy};
use velnor_actions_orchestrator::{
    GenerationPreparation, OrchestratorError, prepare, render_staged_tree,
};
use velnor_actions_rust::VelnorV1TaskOwner;
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::{rendered_job_block, rust_owner_jobs, task_execution_ids};
use super::workflow_configuration_fixture;

#[test]
fn prepare_and_render_scope_repository_maintenance_owner_and_feature_union()
-> Result<(), Box<dyn Error>> {
    assert_owner_feature_success(
        WorkflowPolicy::ConsumerV1,
        true,
        "maintenance-feature",
        "maintenance-only",
        Some("maintenance-only"),
        true,
        VelnorV1TaskOwner::RepositoryMaintenance,
    )?;
    assert_owner_feature_success(
        WorkflowPolicy::VelnorRepositoryV1,
        true,
        "default",
        "default",
        None,
        false,
        VelnorV1TaskOwner::RepositoryMaintenance,
    )?;
    assert_repository_feature_union_excludes_tagged_member()?;
    assert_owner_feature_success(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "maintenance-feature",
        "maintenance-only",
        Some("maintenance-only"),
        true,
        VelnorV1TaskOwner::Project,
    )?;
    Ok(())
}

fn assert_owner_feature_success(
    policy: WorkflowPolicy,
    tagged: bool,
    configuration_name: &str,
    feature: &str,
    member_features: Option<&str>,
    expect_fallback: bool,
    expected_owner: VelnorV1TaskOwner,
) -> Result<(), Box<dyn Error>> {
    let repo = owner_feature_repo(policy, tagged, configuration_name, feature)?;
    let prepared = prepare(repo.path())?;
    assert_owner_feature_config(&prepared, policy, configuration_name, feature)?;
    assert_owner_workspace(&prepared, expected_owner)?;
    let proposal_paths =
        owner_feature_proposal_paths(&prepared, configuration_name, member_features)?;
    if expect_fallback {
        assert_owner_feature_fallback(&prepared, configuration_name);
    } else {
        assert_eq!(prepared.discovery.feature_fallbacks, Vec::new());
    }
    let rendered = render_staged_tree(&prepared)?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    assert_required_owner_routes(
        &prepared,
        workflow,
        &proposal_paths,
        member_features.is_some(),
    )?;
    Ok(())
}

pub(super) fn owner_feature_repo(
    policy: WorkflowPolicy,
    tagged: bool,
    configuration_name: &str,
    feature: &str,
) -> Result<TempDir, Box<dyn Error>> {
    let policy_name = match policy {
        WorkflowPolicy::ConsumerV1 => "consumer-v1",
        WorkflowPolicy::VelnorRepositoryV1 => "velnor-repository-v1",
    };
    let config = format!(
        "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"{policy_name}\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"{configuration_name}\"\nfeatures = [\"{feature}\"]\nall_features = false\ntarget = \"host\"\n"
    );
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    if policy == WorkflowPolicy::VelnorRepositoryV1 {
        workflow_configuration_fixture::git(
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/tailrocks/velnor-new.git",
            ],
            root,
        )?;
        fs::write(
            root.join(".velnor/generator.lock"),
            include_str!("../../../../.velnor/generator.lock"),
        )?;
        fs::write(
            root.join(".velnor/controller-approvals.json"),
            include_str!("../../../../.velnor/controller-approvals.json"),
        )?;
    }
    let mut root_manifest = fs::read_to_string(root.join("Cargo.toml"))?;
    root_manifest.push_str("\n[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n");
    fs::write(root.join("Cargo.toml"), root_manifest)?;
    write_owner_member(root, tagged)?;
    workflow_configuration_fixture::write_nextest_task(root)?;
    workflow_configuration_fixture::write_nextest_task(&root.join("crates/maintenance"))?;
    Ok(repo)
}

fn write_owner_member(root: &Path, tagged: bool) -> Result<(), Box<dyn Error>> {
    let owner_metadata = if tagged {
        "\n[package.metadata.velnor]\nv1-task-owner = \"repository-maintenance\"\n"
    } else {
        ""
    };
    let manifest = format!(
        "[package]\nname = \"maintenance\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{owner_metadata}\n[features]\nmaintenance-only = []\n"
    );
    let package_root = root.join("crates/maintenance");
    fs::create_dir_all(package_root.join("src"))?;
    fs::write(package_root.join("Cargo.toml"), manifest)?;
    fs::write(package_root.join("src/lib.rs"), "pub fn maintenance() {}\n")?;
    Ok(())
}

fn assert_owner_feature_config(
    prepared: &GenerationPreparation,
    policy: WorkflowPolicy,
    configuration_name: &str,
    feature: &str,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(prepared.config.workflow.policy, policy);
    let rust = prepared
        .config
        .stacks
        .rust
        .as_ref()
        .ok_or("Rust configuration is missing")?;
    assert_eq!(rust.configurations.len(), 1);
    let configuration = &rust.configurations[0];
    assert_eq!(configuration.name, configuration_name);
    assert_eq!(configuration.features, vec![feature.to_owned()]);
    assert!(!configuration.all_features);
    Ok(())
}

fn assert_owner_workspace(
    prepared: &GenerationPreparation,
    expected_owner: VelnorV1TaskOwner,
) -> Result<(), Box<dyn Error>> {
    let workspace = prepared
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root.is_empty())
        .ok_or("root Cargo workspace is missing")?;
    assert_eq!(workspace.record.packages.len(), 2);
    assert!(!workspace.record.has_external_dependencies);
    let demo = workspace
        .record
        .packages
        .iter()
        .find(|package| package.manifest == "Cargo.toml")
        .ok_or("root Cargo package is missing")?;
    assert_eq!(demo.name, "demo");
    assert!(demo.in_workspace && !demo.external);
    assert_eq!(demo.v1_task_owner, VelnorV1TaskOwner::Project);
    let maintenance = workspace
        .record
        .packages
        .iter()
        .find(|package| package.manifest == "crates/maintenance/Cargo.toml")
        .ok_or("maintenance Cargo member is missing")?;
    assert_eq!(maintenance.name, "maintenance");
    assert!(maintenance.in_workspace && !maintenance.external);
    assert_eq!(maintenance.v1_task_owner, expected_owner);
    Ok(())
}

fn owner_feature_proposal_paths(
    prepared: &GenerationPreparation,
    configuration_name: &str,
    member_features: Option<&str>,
) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let mut paths = BTreeMap::new();
    for task in &prepared.discovery.proposals {
        if task.stack_id != "rust"
            || task.configuration != configuration_name
            || task.no_targets
            || task.identity.unit_id.is_empty()
        {
            continue;
        }
        let manifest = task.identity.unit_path.as_str();
        let expected_features = match manifest {
            "Cargo.toml" => vec!["default".to_owned()],
            "crates/maintenance/Cargo.toml" => vec![
                member_features
                    .ok_or("unexpected maintenance package proposal")?
                    .to_owned(),
            ],
            other => return Err(format!("unexpected Rust package proposal: {other}").into()),
        };
        assert_eq!(task.identity.features, expected_features, "{manifest}");
        assert!(
            paths
                .insert(task.task_id.clone(), manifest.to_owned())
                .is_none()
        );
    }
    let expected_paths = if member_features.is_some() {
        BTreeSet::from([
            "Cargo.toml".to_owned(),
            "crates/maintenance/Cargo.toml".to_owned(),
        ])
    } else {
        BTreeSet::from(["Cargo.toml".to_owned()])
    };
    assert_eq!(
        paths.values().cloned().collect::<BTreeSet<_>>(),
        expected_paths
    );
    assert!(!paths.is_empty());
    Ok(paths)
}

fn assert_owner_feature_fallback(prepared: &GenerationPreparation, configuration_name: &str) {
    let fallbacks: Vec<_> = prepared
        .discovery
        .feature_fallbacks
        .iter()
        .filter(|fallback| {
            fallback.package_name == "demo" && fallback.configuration == configuration_name
        })
        .collect();
    assert_eq!(fallbacks.len(), 1);
    assert_eq!(fallbacks[0].requested, vec!["maintenance-only".to_owned()]);
    assert_eq!(fallbacks[0].applied, vec!["default".to_owned()]);
}

pub(super) fn assert_required_owner_routes(
    prepared: &GenerationPreparation,
    workflow: &str,
    proposal_paths: &BTreeMap<String, String>,
    include_member: bool,
) -> Result<(), Box<dyn Error>> {
    let required = prepared
        .workflow
        .ir
        .jobs
        .get(REQUIRED_JOB_ID)
        .ok_or("missing Required job")?;
    let owner_jobs = rust_owner_jobs(prepared);
    let owner_ids: BTreeSet<_> = owner_jobs.iter().map(|(id, _)| *id).collect();
    assert!(!owner_ids.is_empty());
    let required_rust_owners: BTreeSet<_> = required
        .needs
        .iter()
        .filter_map(|id| {
            let job = prepared.workflow.ir.jobs.get(id)?;
            (id.starts_with("rust-") && !job.is_dependency_seed()).then_some(id.as_str())
        })
        .collect();
    assert_eq!(required_rust_owners, owner_ids);
    for owner_id in owner_ids {
        let job_block = rendered_job_block(workflow, owner_id)?;
        assert!(job_block.starts_with(&format!("  {owner_id}:\n")));
    }
    assert_emitted_owner_tasks(&owner_jobs, proposal_paths, include_member)
}

fn assert_emitted_owner_tasks(
    owner_jobs: &[(&str, &Job)],
    proposal_paths: &BTreeMap<String, String>,
    include_member: bool,
) -> Result<(), Box<dyn Error>> {
    use super::support::logical_owner_id;
    use std::collections::btree_map::Entry;
    // Reader/writer branches run on mutually exclusive events, so each branch
    // carries its owner's task executions and both belong to one logical owner.
    let all: BTreeSet<&str> = owner_jobs.iter().map(|(id, _)| *id).collect();
    let mut emitted: BTreeMap<String, &str> = BTreeMap::new();
    let mut owners_by_manifest: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (owner_id, job) in owner_jobs {
        let owner = logical_owner_id(&all, owner_id);
        let task_ids: Vec<_> = task_execution_ids(job).collect();
        assert!(!task_ids.is_empty(), "{owner_id} owns no task execution");
        for task_id in task_ids {
            let manifest = proposal_paths
                .get(task_id)
                .ok_or_else(|| format!("{owner_id} emits unknown task {task_id}"))?;
            match emitted.entry(task_id.to_owned()) {
                Entry::Vacant(entry) => {
                    entry.insert(owner);
                }
                Entry::Occupied(entry) => {
                    assert!(entry.get() == &owner, "duplicate owner for {task_id}");
                }
            }
            owners_by_manifest
                .entry(manifest.as_str())
                .or_default()
                .insert(owner);
        }
    }
    assert_eq!(
        emitted.keys().cloned().collect::<BTreeSet<_>>(),
        proposal_paths.keys().cloned().collect::<BTreeSet<_>>()
    );
    let expected_manifests = if include_member {
        BTreeSet::from(["Cargo.toml", "crates/maintenance/Cargo.toml"])
    } else {
        BTreeSet::from(["Cargo.toml"])
    };
    assert_eq!(
        owners_by_manifest.keys().copied().collect::<BTreeSet<_>>(),
        expected_manifests
    );
    assert!(owners_by_manifest.values().all(|owners| owners.len() == 1));
    let distinct_owners: BTreeSet<_> = owners_by_manifest.values().flatten().copied().collect();
    assert_eq!(distinct_owners.len(), expected_manifests.len());
    Ok(())
}

fn assert_repository_feature_union_excludes_tagged_member() -> Result<(), Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        true,
        "maintenance-feature",
        "maintenance-only",
    )?;
    let Err(error) = prepare(repo.path()) else {
        return Err(io::Error::other("maintenance-only unexpectedly prepared").into());
    };
    match error {
        OrchestratorError::Config {
            file,
            key_path,
            problem,
        } => {
            assert_eq!(file, ".velnor/config.toml");
            assert_eq!(key_path, "stacks.rust.configurations.features");
            assert_eq!(
                problem,
                "unknown_feature:maintenance-feature:demo:maintenance-only"
            );
        }
        other => return Err(format!("unexpected feature-selection error: {other}").into()),
    }
    Ok(())
}
