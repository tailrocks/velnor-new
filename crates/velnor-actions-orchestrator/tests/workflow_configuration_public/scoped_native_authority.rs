// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{GenerationPreparation, prepare};
use velnor_actions_workflow_renderer::{
    native_authority::{
        CargoWorkspaceFact, LogicalJobFact, LogicalJobSource, NativeAuthority,
        NativeAuthorityInput, NativeRustProvider, NativeRustSource, NativeTaskFact, RustTaskFact,
        WorkspaceFormatFact, WorkspacePackageFact,
    },
    native_job_builders::PlanJobToolNeeds,
};

use super::feature_toggles::feature_toggle_repo;

const SCOPED: &str = "schema = 1\n[workflow]\nname = \"CI\"\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[[stacks.rust.configurations]]\nname = \"scoped\"\nfeatures = [\"default\"]\ntarget = \"host\"\npackages = [\"maintenance\"]\n";

fn scoped_preparation() -> Result<(tempfile::TempDir, GenerationPreparation), Box<dyn Error>> {
    let repo = feature_toggle_repo(false, false)?;
    fs::write(repo.path().join(".velnor/config.toml"), SCOPED)?;
    let prepared = prepare(repo.path())?;
    Ok((repo, prepared))
}

fn workspace_fact(prepared: &GenerationPreparation) -> Result<CargoWorkspaceFact, Box<dyn Error>> {
    let workspace = prepared
        .discovery
        .workspaces
        .first()
        .ok_or("workspace missing")?;
    Ok(CargoWorkspaceFact {
        root: workspace.record.workspace_root.clone(),
        lockfile_path: None,
        test_runner: workspace.profile.test_runner,
        packages: workspace
            .record
            .packages
            .iter()
            .map(|record| WorkspacePackageFact {
                id: record.id.clone(),
                name: record.name.clone(),
            })
            .collect(),
    })
}

fn task_and_job(
    prepared: &GenerationPreparation,
    package: &str,
) -> Result<(RustTaskFact, LogicalJobFact), Box<dyn Error>> {
    let target = prepared
        .discovery
        .workspaces
        .first()
        .and_then(|workspace| {
            workspace
                .record
                .packages
                .iter()
                .find(|record| record.name == package)
        })
        .ok_or("package missing")?;
    let task = RustTaskFact {
        unit: target.id.clone(),
        no_targets: false,
        manifest: target.manifest.clone(),
        kind: velnor_actions_rust::TaskKind::Clippy,
        configuration: "scoped".to_owned(),
        all_features: false,
        shard: None,
    };
    let ids = velnor_actions_contract::assign_crate_job_ids(
        &BTreeSet::from([(target.id.clone(), target.name.clone(), "scoped".to_owned())]),
        velnor_actions_contract::CRATE_JOB_ID_PREFIX,
    );
    let job = LogicalJobFact {
        id: ids[&(target.id.clone(), "scoped".to_owned())].clone(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        needs: Vec::new(),
        outputs: BTreeMap::new(),
        source: LogicalJobSource::Task(NativeTaskFact {
            compile_driver: Some(velnor_actions_rust::CompileDriver::Cargo),
            unit: target.id.clone(),
            display_name: target.name.clone(),
            configuration: "scoped".to_owned(),
            rust_manifest: Some(target.manifest.clone()),
            tofu: Vec::new(),
        }),
    };
    Ok((task, job))
}

fn authority_error(
    prepared: &GenerationPreparation,
    config: &velnor_actions_contract::VelnorConfig,
    package: &str,
    format: Option<&WorkspaceFormatFact>,
) -> Result<String, Box<dyn Error>> {
    let workspaces = [workspace_fact(prepared)?];
    let (task, job) = task_and_job(prepared, package)?;
    let (tasks, jobs) = if format.is_some() {
        (Vec::new(), Vec::new())
    } else {
        (vec![task], vec![job])
    };
    let input = NativeAuthorityInput {
        config,
        runner: "ubuntu-26.04",
        published_generator_lock: None,
        approved_release: None,
        consumer_manifest_json: None,
        tool_pins: &[],
        rust_source: NativeRustSource::ConfigOrDefault,
        rust_provider: NativeRustProvider::Rustup,
        cargo_workspaces: &workspaces,
        workflow_sources: &[],
        runner_family_tools: None,
        generated_sources: &[],
        rust_tasks: &tasks,
        tofu_tasks: &[],
        gradle_roots: &[],
        logical_jobs: &jobs,
        workspace_format: format,
        tools: PlanJobToolNeeds::default(),
        rust_detected: true,
        state_roots: &[],
        seal: &velnor_actions_workflow_renderer::plan_sources::state_select::StateSealPlan::default(
        ),
    };
    Ok(NativeAuthority::build(input)
        .err()
        .map(|error| format!("{error:?}"))
        .unwrap_or_default())
}

#[test]
fn native_authority_refuses_a_task_for_a_package_outside_the_configuration_scope()
-> Result<(), Box<dyn Error>> {
    let (_repo, prepared) = scoped_preparation()?;
    let outside = authority_error(&prepared, &prepared.config, "demo", None)?;
    assert!(
        outside.contains("native_rust_task_package_scope"),
        "{outside}"
    );
    let inside = authority_error(&prepared, &prepared.config, "maintenance", None)?;
    assert!(
        !inside.contains("native_rust_task_package_scope"),
        "{inside}"
    );
    Ok(())
}

#[test]
fn native_authority_refuses_workspace_format_outside_the_configuration_scope()
-> Result<(), Box<dyn Error>> {
    let (_repo, prepared) = scoped_preparation()?;
    let format = WorkspaceFormatFact {
        manifest: "Cargo.toml".to_owned(),
        configuration: "scoped".to_owned(),
        target: "host".to_owned(),
        compile_driver: velnor_actions_rust::CompileDriver::Cargo,
    };
    let mut config = prepared.config.clone();
    let rust = config.stacks.rust.as_mut().ok_or("rust config missing")?;
    rust.configurations[0].packages = vec!["ghost".to_owned()];
    let outside = authority_error(&prepared, &config, "maintenance", Some(&format))?;
    assert!(
        outside.contains("native_workspace_format_scope"),
        "{outside}"
    );
    let inside = authority_error(&prepared, &prepared.config, "maintenance", Some(&format))?;
    assert!(
        !inside.contains("native_workspace_format_scope"),
        "{inside}"
    );
    Ok(())
}
