// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_contract::{StepRole, WorkflowPolicy};
use velnor_actions_orchestrator::{finalized_jobs, prepare, render_staged_tree};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::workflow_configuration_fixture::write_root_cargo_lock;

#[test]
fn check_job_restores_cargo_sources_and_never_saves() -> Result<(), Box<dyn Error>> {
    let repo = owner_feature_repo(
        WorkflowPolicy::VelnorRepositoryV1,
        false,
        "default",
        "default",
    )?;
    let root = repo.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/maintenance\"]\nresolver = \"2\"\n",
    )?;
    fs::remove_dir_all(root.join("src"))?;
    write_root_cargo_lock(root, &["maintenance"])?;
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?;
    fs::write(
        &config_path,
        format!(
            "{config}\n[[checks]]\nid = \"maintenance-helpers\"\ntask = \"fixture-check\"\ninputs = [\"crates/maintenance/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
        ),
    )?;
    let mise = root.join("mise.toml");
    let content = fs::read_to_string(&mise).unwrap_or_default();
    fs::write(
        &mise,
        format!("{content}\n[tasks.fixture-check]\nrun = \"true\"\n"),
    )?;
    let prepared = prepare(root)?;
    let jobs = finalized_jobs(&prepared)?;
    let id = "check-maintenance-helpers";
    let job = jobs.get(id).ok_or("check job missing")?;
    let roles: Vec<_> = job.steps.iter().filter_map(|step| step.role).collect();
    assert!(roles.contains(&StepRole::CargoSourcesRestore));
    assert!(!roles.contains(&StepRole::CargoSourcesSave));
    let tree = render_staged_tree(&prepared)?;
    let block = rendered_job_block(tree.get(WORKFLOW_PATH).ok_or("workflow missing")?, id)?;
    let restore = block
        .find("uses: actions/cache/restore@")
        .ok_or("check job has no cache restore")?;
    let execute = block.find("Execute named check").ok_or("no execute step")?;
    assert!(restore < execute, "cache restore must precede the check");
    assert!(block.contains("${{needs.plan.outputs.sources_key}}"));
    assert!(!block.contains("actions/cache/save@"));
    assert!(!block.contains("actions/cache@"));
    Ok(())
}
