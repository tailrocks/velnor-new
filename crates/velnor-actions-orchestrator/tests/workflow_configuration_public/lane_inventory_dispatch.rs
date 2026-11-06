// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};

use serde_json::{Value, json};
use velnor_actions_contract::{ExecutionMode, WorkflowPolicy, expand_workflow_with_lane_plan};
use velnor_actions_orchestrator::{
    MergeContext, merge_internal_with_context, plan_internal, prepare,
};

use super::maintenance_features::owner_feature_repo;
use super::scoped_workspace::commit;
use super::workflow_configuration_fixture::{add_independent_mbx_package, write_root_cargo_lock};

#[test]
fn dispatched_inventory_binds_plan_and_required_without_named_checks() -> Result<(), Box<dyn Error>>
{
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
    add_independent_mbx_package(root, "crates/a-b", "a-b")?;
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?
        .replace("schema = 1", "schema = 2")
        .replace(
            "[[stacks.rust.configurations]]",
            "[stacks.rust]\nnon_inputs = [\"docs/**\"]\n[[stacks.rust.configurations]]",
        );
    let config = format!(
        "{config}\n[execution]\nmode = \"both\"\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n"
    );
    fs::write(&config_path, &config)?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("docs/guide.md"), "before\n")?;
    let base = commit(root, "configure dispatch fixture")?;
    fs::write(
        root.join("crates/maintenance/src/lib.rs"),
        "pub fn changed() {}\n",
    )?;
    let affected = commit(root, "change Rust source")?;
    let prepared = prepare(root)?;
    let hosted = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Hosted),
    )?;
    let scale = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::ScaleSet),
    )?;
    let hosted_inventory = hosted
        .lane_plan
        .as_ref()
        .ok_or("hosted inventory missing")?
        .inventory();
    let scale_inventory = scale
        .lane_plan
        .as_ref()
        .ok_or("scale inventory missing")?
        .inventory();
    assert_eq!(hosted_inventory.mode, Some(ExecutionMode::Hosted));
    assert_eq!(scale_inventory.mode, Some(ExecutionMode::ScaleSet));
    assert_ne!(
        serde_json::to_value(&hosted_inventory)?,
        serde_json::to_value(&scale_inventory)?
    );
    let hosted_response = plan(
        root,
        &base,
        &affected,
        &serde_json::to_value(&hosted_inventory)?,
    )?;
    let scale_response = plan(
        root,
        &base,
        &affected,
        &serde_json::to_value(&scale_inventory)?,
    )?;

    fs::write(
        &config_path,
        config.replace("mode = \"both\"", "mode = \"hosted\""),
    )?;
    let default_prepared = prepare(root)?;
    let default = expand_workflow_with_lane_plan(
        &default_prepared.workflow.ir,
        &default_prepared.config,
        None,
    )?;
    let inventory = serde_json::to_value(
        default
            .lane_plan
            .as_ref()
            .ok_or("default lane plan missing")?
            .inventory(),
    )?;
    let with_inventory = plan(root, &base, &affected, &inventory)?;
    let without_inventory = plan_internal(&json!({"schema": 1, "op": "plan-v1", "run_key": "r190002-a1", "event": "pull_request", "base": base, "head": affected, "root": root}).to_string())?;
    assert_eq!(
        with_inventory,
        serde_json::from_str::<Value>(&without_inventory)?
    );
    fs::write(&config_path, &config)?;
    fs::write(root.join("docs/guide.md"), "after\n")?;
    let docs = commit(root, "change docs")?;

    for (expansion, inventory, response, expected_environment) in [
        (
            &hosted,
            serde_json::to_value(&hosted_inventory)?,
            &hosted_response,
            "github-hosted",
        ),
        (
            &scale,
            serde_json::to_value(&scale_inventory)?,
            &scale_response,
            "self-hosted",
        ),
    ] {
        let entries = response["plan"]["matrix"]["include"]
            .as_array()
            .ok_or("matrix missing")?;
        let rust_entries: Vec<_> = entries
            .iter()
            .filter(|entry| entry["stack_id"] == "rust")
            .collect();
        assert_ne!(rust_entries, [] as [&Value; 0]);
        for entry in rust_entries {
            let job_id = entry["job_id"].as_str().ok_or("job id missing")?;
            let job = expansion
                .ir
                .jobs
                .get(job_id)
                .ok_or("matrix job missing from workflow")?;
            assert_eq!(entry["planned_platform"]["runs_on"], job.runs_on);
            assert_eq!(
                entry["planned_platform"]["runner_environment"],
                expected_environment
            );
            assert_eq!(
                entry["planned_platform"]["target"],
                "x86_64-unknown-linux-gnu"
            );
        }
        let skipped = plan(root, &affected, &docs, &inventory)?;
        let required_ids = &expansion.ir.jobs["required"].needs;
        let baseline = required(&skipped, required_ids, &inventory)?;
        assert!(
            baseline["status"] == "passed" || baseline["status"] == "no_work",
            "{baseline}"
        );
        let other = if expected_environment == "github-hosted" {
            serde_json::to_value(&scale_inventory)?
        } else {
            serde_json::to_value(&hosted_inventory)?
        };
        let swapped = required(&skipped, required_ids, &other)?;
        assert_ne!(
            swapped["status"], "passed",
            "cross-mode inventory accepted: {swapped}"
        );
        assert_ne!(
            swapped["status"], "no_work",
            "cross-mode inventory accepted: {swapped}"
        );
    }

    Ok(())
}

fn plan(root: &Path, base: &str, head: &str, inventory: &Value) -> Result<Value, Box<dyn Error>> {
    let request = json!({"schema": 1, "op": "plan-v1", "run_key": "r190002-a1", "event": "pull_request", "base": base, "head": head, "root": root, "lane_inventory": inventory});
    Ok(serde_json::from_str(&plan_internal(&request.to_string())?)?)
}

fn required(
    response: &Value,
    required_ids: &[String],
    inventory: &Value,
) -> Result<Value, Box<dyn Error>> {
    let jobs = response["plan"]["jobs"]
        .as_array()
        .ok_or("plan jobs missing")?;
    let results: Vec<_> = required_ids
        .iter()
        .map(|id| {
            let skipped = jobs
                .iter()
                .find(|job| job["job_id"] == *id)
                .is_some_and(|job| job["disposition"] == "not_affected");
            json!({"job_id": id, "conclusion": if skipped { "skipped" } else { "success" }})
        })
        .collect();
    let request = json!({"schema": 1, "run_key": "r190002-a1", "actual_event": "pull_request", "plan": response["plan"], "matrix": response["matrix"], "matrix_reports": [], "task_reports": [], "required_job_ids": required_ids, "required_jobs": results, "lane_inventory": inventory});
    Ok(serde_json::from_str(&merge_internal_with_context(
        &request.to_string(),
        &MergeContext::detached(),
    )?)?)
}
