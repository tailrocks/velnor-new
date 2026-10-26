// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;

use velnor_actions_contract::{Job, REQUIRED_JOB_ID};
use velnor_actions_orchestrator::{GenerationPreparation, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::support::{
    config_with_timeouts, discovered_rust_packages, rendered_job_block, rust_owner_jobs,
    task_execution_ids, task_execution_steps,
};
use super::workflow_configuration_fixture;

#[test]
fn prepare_and_render_keep_one_parallel_rust_owner_per_package() -> Result<(), Box<dyn Error>> {
    let config = config_with_timeouts(15, None)?;
    let repo = workflow_configuration_fixture::make_repo(&config)?;
    let root = repo.path();
    workflow_configuration_fixture::add_independent_mbx_package(root, "crates/nested", "nested")?;
    workflow_configuration_fixture::write_nextest_task(root)?;
    workflow_configuration_fixture::write_nextest_task(&root.join("crates/nested"))?;

    let prepared = prepare(root)?;
    let rendered = render_staged_tree(&prepared)?;
    let workflow = rendered
        .get(WORKFLOW_PATH)
        .ok_or("rendered workflow is missing")?;
    assert_eq!(
        prepared.workflow.ir.jobs[velnor_actions_workflow_renderer::render::PLAN_JOB_ID]
            .timeout_minutes
            .minutes(),
        15
    );
    let nested_workspace = prepared
        .discovery
        .workspaces
        .iter()
        .find(|workspace| workspace.record.workspace_root == "crates/nested")
        .ok_or("missing independent nested Cargo workspace")?;
    assert_eq!(nested_workspace.profile.compile_driver.as_str(), "mbx");
    assert!(nested_workspace.record.packages.iter().any(|package| {
        package.name == "nested" && package.manifest == "crates/nested/Cargo.toml"
    }));
    let (unit_paths, expected_packages) = discovered_rust_packages(&prepared);
    assert_package_tasks_are_not_planned(&prepared, &unit_paths)?;
    let owner_jobs = rust_owner_jobs(&prepared);
    assert_eq!(owner_jobs.len(), expected_packages.len());
    let required = prepared
        .workflow
        .ir
        .jobs
        .get(REQUIRED_JOB_ID)
        .ok_or("missing Required job")?;
    assert_rust_owner_jobs(
        &owner_jobs,
        required,
        &unit_paths,
        &expected_packages,
        workflow,
    )?;
    Ok(())
}

pub(super) fn assert_package_tasks_are_not_planned(
    prepared: &GenerationPreparation,
    unit_paths: &BTreeMap<&str, &str>,
) -> Result<(), Box<dyn Error>> {
    let plan_id = velnor_actions_workflow_renderer::render::PLAN_JOB_ID;
    let plan = prepared
        .workflow
        .ir
        .jobs
        .get(plan_id)
        .ok_or("missing Plan job")?;
    let planned_task_ids: BTreeSet<_> = task_execution_ids(plan).collect();
    let package_task_ids: BTreeSet<_> = unit_paths.keys().copied().collect();
    assert!(
        package_task_ids.is_disjoint(&planned_task_ids),
        "package-owned Rust tasks must not be emitted in Plan"
    );
    Ok(())
}

pub(super) fn assert_rust_owner_jobs(
    owner_jobs: &[(&str, &Job)],
    required: &Job,
    unit_paths: &BTreeMap<&str, &str>,
    expected_packages: &BTreeSet<&str>,
    workflow: &str,
) -> Result<(), Box<dyn Error>> {
    let mut owner_tasks = BTreeMap::new();
    let mut owners_by_package: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for &(id, job) in owner_jobs {
        assert_rust_owner_job(
            id,
            job,
            required,
            unit_paths,
            workflow,
            &mut owner_tasks,
            &mut owners_by_package,
        )?;
    }
    let package_task_ids: BTreeSet<_> = unit_paths.keys().copied().collect();
    assert_eq!(
        owner_tasks.keys().copied().collect::<BTreeSet<_>>(),
        package_task_ids
    );
    assert_eq!(owners_by_package.len(), expected_packages.len());
    assert!(owners_by_package.values().all(|owners| owners.len() == 1));
    assert_eq!(
        owners_by_package.keys().copied().collect::<BTreeSet<_>>(),
        *expected_packages
    );
    Ok(())
}

fn assert_rust_owner_job<'a>(
    id: &'a str,
    job: &'a Job,
    required: &Job,
    unit_paths: &BTreeMap<&'a str, &'a str>,
    workflow: &str,
    owner_tasks: &mut BTreeMap<&'a str, &'a str>,
    owners_by_package: &mut BTreeMap<&'a str, BTreeSet<&'a str>>,
) -> Result<(), Box<dyn Error>> {
    assert_eq!(
        job.timeout_minutes.minutes(),
        5,
        "default crate budget for {id}"
    );
    let task_steps = task_execution_steps(job);
    assert!(!task_steps.is_empty(), "{id} owns discovered Rust tasks");
    assert!(required.needs.iter().any(|need| need == id));
    for (_, task_id, _, _, _) in &task_steps {
        let package = *unit_paths
            .get(*task_id)
            .ok_or_else(|| format!("{id} emitted undiscovered Rust task {task_id}"))?;
        assert!(owner_tasks.insert(*task_id, id).is_none());
        owners_by_package.entry(package).or_default().insert(id);
    }

    let build_index = task_steps
        .iter()
        .position(|(_, _, name, _, _)| *name == "Build test executables")
        .ok_or_else(|| format!("{id} has no sequential build step"))?;
    assert!(!task_steps[build_index].3);
    let checks: Vec<_> = task_steps
        .iter()
        .filter(|step| step.0 != task_steps[build_index].0)
        .collect();
    assert!(checks.len() >= 2, "{id} has multiple discovered checks");
    assert!(
        checks
            .iter()
            .all(|step| { step.0 > task_steps[build_index].0 && step.3 })
    );

    let clippy = task_steps
        .iter()
        .find(|(_, _, name, _, _)| *name == "Clippy")
        .ok_or_else(|| format!("{id} has no Clippy task"))?;
    let documentation = task_steps
        .iter()
        .find(|(_, _, name, _, _)| *name == "Documentation")
        .ok_or_else(|| format!("{id} has no documentation task"))?;
    let clippy_manifest = *unit_paths
        .get(clippy.1)
        .ok_or_else(|| format!("{id} emitted undiscovered Clippy task {}", clippy.1))?;
    let documentation_manifest = *unit_paths.get(documentation.1).ok_or_else(|| {
        format!(
            "{id} emitted undiscovered Documentation task {}",
            documentation.1
        )
    })?;
    assert_eq!(documentation_manifest, clippy_manifest);
    let (clippy_target, documentation_target) = lane_targets_for_fixture_manifest(clippy_manifest)?;
    assert_eq!(clippy.4, Some(clippy_target));
    assert_eq!(documentation.4, Some(documentation_target));
    assert_ne!(clippy.4, documentation.4);

    let job_yaml = rendered_job_block(workflow, id)?;
    let parallel_count = task_steps.iter().filter(|step| step.3).count();
    assert_eq!(job_yaml.matches("background: true").count(), parallel_count);
    assert_eq!(
        job_yaml
            .matches("- name: Wait for parallel checks\n        wait-all:")
            .count(),
        1
    );
    Ok(())
}

fn lane_targets_for_fixture_manifest(
    manifest: &str,
) -> Result<(&'static str, &'static str), Box<dyn Error>> {
    match manifest {
        "Cargo.toml" => Ok(("target/velnor-clippy", "target/velnor-doc")),
        "crates/nested/Cargo.toml" => Ok((
            "crates/nested/target/velnor-clippy",
            "crates/nested/target/velnor-doc",
        )),
        _ => Err(format!("unexpected fixture workspace manifest: {manifest}").into()),
    }
}
