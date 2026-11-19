// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path};

use serde_json::{Value, json};
use velnor_actions_contract::{
    ExecutionMode, WorkflowPolicy, expand_workflow_with_lane_plan, named_check_lanes,
};
use velnor_actions_orchestrator::{
    MergeContext, merge_internal_with_context, plan_internal, prepare,
};

use super::one_job_per_crate::mbx_repo_for;
use super::scoped_workspace::commit;
use super::workflow_configuration_fixture::write_root_cargo_lock;

const CHECK: &str = "\n[[checks]]\nid = \"scoped\"\ntask = \"fixture-check\"\nselection = \"declared_inputs\"\ninputs = [\"crates/member/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n";

#[test]
fn dispatched_inventory_binds_plan_and_required() -> Result<(), Box<dyn Error>> {
    dispatched_inventory(true)?;
    dispatched_inventory(false)
}

fn dispatched_inventory(tofu: bool) -> Result<(), Box<dyn Error>> {
    let repo = mbx_repo_for(tofu, WorkflowPolicy::VelnorRepositoryV1)?;
    let root = repo.path();
    fs::write(
        root.join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"true\"\n",
    )?;
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/member\"]\nresolver = \"2\"\n",
    )?;
    fs::remove_dir_all(root.join("src"))?;
    fs::create_dir_all(root.join("crates/member/src"))?;
    fs::write(
        root.join("crates/member/Cargo.toml"),
        "[package]\nname = \"member\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(root.join("crates/member/src/lib.rs"), "pub fn f() {}\n")?;
    write_root_cargo_lock(root, &["member"])?;
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?.replace("schema = 1", "schema = 2");
    let config = format!(
        "{config}\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n[execution]\nmode = \"both\"\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n{CHECK}"
    );
    fs::write(&config_path, &config)?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("docs/guide.md"), "before\n")?;
    let base = commit(root, "configure dispatch fixture")?;
    fs::write(
        root.join("crates/member/src/lib.rs"),
        "pub fn changed() {}\n",
    )?;
    let affected = commit(root, "change the checked source")?;
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
    let both = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
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
    let both_inventory = both
        .lane_plan
        .as_ref()
        .ok_or("both inventory missing")?
        .inventory();
    assert_eq!(hosted_inventory.mode, Some(ExecutionMode::Hosted));
    assert_eq!(scale_inventory.mode, Some(ExecutionMode::ScaleSet));
    assert_eq!(both_inventory.mode, Some(ExecutionMode::Both));
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
    let both_response = plan(
        root,
        &base,
        &affected,
        &serde_json::to_value(&both_inventory)?,
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
    let lanes = lanes_for(root, &inventory)?;
    let without_inventory = plan_internal(&json!({"schema": 1, "op": "plan-v1", "run_key": "r190002-a1", "event": "pull_request", "base": base, "head": affected, "root": root, "named_check_lanes": lanes}).to_string())?;
    assert_eq!(
        with_inventory,
        serde_json::from_str::<Value>(&without_inventory)?
    );
    fs::write(&config_path, &config)?;
    fs::write(root.join("docs/guide.md"), "after\n")?;
    let docs = commit(root, "change docs")?;

    for (expansion, inventory, response, mode) in [
        (
            &hosted,
            serde_json::to_value(&hosted_inventory)?,
            &hosted_response,
            ExecutionMode::Hosted,
        ),
        (
            &scale,
            serde_json::to_value(&scale_inventory)?,
            &scale_response,
            ExecutionMode::ScaleSet,
        ),
        (
            &both,
            serde_json::to_value(&both_inventory)?,
            &both_response,
            ExecutionMode::Both,
        ),
    ] {
        let entries = response["plan"]["matrix"]["include"]
            .as_array()
            .ok_or("matrix missing")?;
        if tofu && mode == ExecutionMode::Hosted {
            let tofu_entries: Vec<_> = entries
                .iter()
                .filter(|entry| entry["stack_id"] == "tofu")
                .collect();
            assert_ne!(tofu_entries, [] as [&Value; 0]);
            for entry in tofu_entries {
                let job_id = entry["job_id"].as_str().ok_or("job id missing")?;
                let job = expansion
                    .ir
                    .jobs
                    .get(job_id)
                    .ok_or("matrix job missing from workflow")?;
                assert!(job.check_runner.is_none());
                assert_eq!(entry["planned_platform"]["runs_on"], job.runs_on);
                assert_eq!(
                    entry["planned_platform"]["runner_environment"],
                    "github-hosted"
                );
                assert_eq!(
                    entry["planned_platform"]["target"],
                    "x86_64-unknown-linux-gnu"
                );
            }
        }
        if tofu || mode != ExecutionMode::Both {
            continue;
        }
        let skipped = plan(root, &affected, &docs, &inventory)?;
        let planned = skipped["plan"]["jobs"].as_array().ok_or("jobs missing")?;
        let checks: Vec<&str> = planned
            .iter()
            .filter_map(|job| job["job_id"].as_str())
            .filter(|id| id.starts_with("check-scoped"))
            .collect();
        assert_ne!(checks, [] as [&str; 0]);
        for id in &checks {
            let job = expansion.ir.jobs.get(*id).ok_or("check job missing")?;
            let gate = velnor_actions_contract::workflow::not_affected_gate(id);
            assert!(
                job.condition
                    .as_deref()
                    .is_some_and(|condition| condition.contains(&gate)),
                "{id}"
            );
            let disposition = |response: &Value| {
                response["plan"]["jobs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|job| job["job_id"] == *id)
                    .map(|job| job["disposition"].clone())
            };
            assert_eq!(disposition(response), Some(json!("run")), "{id}");
            assert_eq!(disposition(&skipped), Some(json!("not_affected")), "{id}");
        }
        let required_ids = &expansion.ir.jobs["required"].needs;
        let baseline = required(&skipped, required_ids, &inventory, None)?;
        assert!(
            baseline["status"] == "passed" || baseline["status"] == "no_work",
            "{baseline}"
        );
        let skipped_check = checks.first().ok_or("check job missing")?;
        let mismatch = required(&skipped, required_ids, &inventory, Some(skipped_check))?;
        assert!(
            mismatch["miss_reasons"]
                .as_array()
                .ok_or("miss reasons missing")?
                .iter()
                .any(|reason| reason == "gate_mismatch"),
            "{mismatch}"
        );
        let other = if mode == ExecutionMode::Hosted {
            serde_json::to_value(&scale_inventory)?
        } else {
            serde_json::to_value(&hosted_inventory)?
        };
        let swapped = required(&skipped, required_ids, &other, None)?;
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

fn lanes_for(root: &Path, inventory: &Value) -> Result<Value, Box<dyn Error>> {
    let mode: Option<ExecutionMode> = serde_json::from_value(inventory["mode"].clone())?;
    let prepared = prepare(root)?;
    Ok(serde_json::to_value(named_check_lanes(
        &prepared.workflow.ir,
        &prepared.config,
        mode,
    )?)?)
}

fn plan(root: &Path, base: &str, head: &str, inventory: &Value) -> Result<Value, Box<dyn Error>> {
    let lanes = lanes_for(root, inventory)?;
    let request = json!({"schema": 1, "op": "plan-v1", "run_key": "r190002-a1", "event": "pull_request", "base": base, "head": head, "root": root, "lane_inventory": inventory, "named_check_lanes": lanes});
    Ok(serde_json::from_str(&plan_internal(&request.to_string())?)?)
}

fn required(
    response: &Value,
    required_ids: &[String],
    inventory: &Value,
    succeeded_job: Option<&str>,
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
            json!({"job_id": id, "conclusion": if skipped && succeeded_job != Some(id.as_str()) { "skipped" } else { "success" }})
        })
        .collect();
    let request = json!({"schema": 1, "run_key": "r190002-a1", "actual_event": "pull_request", "plan": response["plan"], "matrix": response["matrix"], "matrix_reports": [], "task_reports": [], "required_job_ids": required_ids, "required_jobs": results, "lane_inventory": inventory});
    Ok(serde_json::from_str(&merge_internal_with_context(
        &request.to_string(),
        &MergeContext::detached(),
    )?)?)
}
