// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeSet, error::Error, fs};

#[path = "both_shared_producers/preview.rs"]
mod preview;

use super::scoped_workspace::commit;
use serde_json::{Value, json};
use velnor_actions_contract::{
    ExecutionMode, JobDisposition, Plan, StepKind, WorkflowPolicy, expand_workflow_with_lane_plan,
    named_check_lanes,
};
use velnor_actions_orchestrator::{
    MergeContext, merge_internal_with_context, plan_internal, prepare, render_staged_tree_with,
};
use velnor_actions_workflow_renderer::render::WORKFLOW_PATH;

use super::maintenance_features::owner_feature_repo;
use super::support::rendered_job_block;
use super::workflow_configuration_fixture::{add_independent_mbx_package, write_root_cargo_lock};

#[test]
fn stable_both_keeps_shared_producers_and_splits_named_check() -> Result<(), Box<dyn Error>> {
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
    add_independent_mbx_package(root, "crates/a/c", "c")?;
    let config_path = root.join(".velnor/config.toml");
    let config = fs::read_to_string(&config_path)?
        .replace("schema = 1", "schema = 2")
        .replace(
            "[[stacks.rust.configurations]]",
            "[stacks.rust]\nnon_inputs = [\"docs/**\"]\n[[stacks.rust.configurations]]",
        );
    fs::write(
        &config_path,
        format!(
            "{config}\n[execution]\nmode = \"both\"\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n\n[[checks]]\nid = \"maintenance-helpers\"\ntask = \"fixture-check\"\ninputs = [\"crates/maintenance/src/lib.rs\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n"
        ),
    )?;
    let mise = root.join("mise.toml");
    let content = fs::read_to_string(&mise).unwrap_or_default();
    fs::write(
        &mise,
        format!("{content}\n[tasks.fixture-check]\nrun = \"true\"\n"),
    )?;
    let prepared = prepare(root)?;
    let lanes = named_check_lanes(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let expanded = expand_workflow_with_lane_plan(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Both),
    )?;
    let required_ids = &expanded.ir.jobs["required"].needs;
    let inventory = expanded
        .lane_plan
        .as_ref()
        .ok_or("missing lane plan")?
        .inventory();
    let hosted_lanes = named_check_lanes(
        &prepared.workflow.ir,
        &prepared.config,
        Some(ExecutionMode::Hosted),
    )?;
    let ids: BTreeSet<_> = lanes["check-maintenance-helpers"]
        .iter()
        .map(|lane| lane.job_id.as_str())
        .collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "check-maintenance-helpers__hosted",
            "check-maintenance-helpers__local"
        ])
    );
    let hosted_tree = render_staged_tree_with(&prepared, Some(ExecutionMode::Hosted))?;
    let hosted_workflow = hosted_tree
        .get(WORKFLOW_PATH)
        .ok_or("missing hosted workflow")?;
    rendered_job_block(hosted_workflow, "check-maintenance-helpers")?;
    rendered_job_block(hosted_workflow, "ledger-cli")?;
    let tree = render_staged_tree_with(&prepared, Some(ExecutionMode::Both))?;
    let workflow = tree.get(WORKFLOW_PATH).ok_or("missing workflow")?;
    for id in &ids {
        rendered_job_block(workflow, id)?;
    }
    let hosted = rendered_job_block(workflow, "check-maintenance-helpers__hosted")?;
    let local = rendered_job_block(workflow, "check-maintenance-helpers__local")?;
    assert!(hosted.contains("VELNOR_CHECK_JOB_ID: check-maintenance-helpers__hosted"));
    assert!(hosted.contains("VELNOR_CHECK_LANE_VARIANT: hosted"));
    assert!(local.contains("VELNOR_CHECK_JOB_ID: check-maintenance-helpers__local"));
    assert!(local.contains("VELNOR_CHECK_LANE_VARIANT: scale_set"));
    assert!(local.contains(velnor_actions_contract::config::EPHEMERAL_CHECK_ADMISSION_CONDITION));
    let seal = tree
        .get(velnor_actions_contract::workflow::state_seal::STATE_SEAL_WORKFLOW_PATH)
        .ok_or("missing state workflow")?;
    for id in ["state-producer-crates-a-b", "state-producer-crates-a-c"] {
        let block = rendered_job_block(seal, id)?;
        assert!(block.contains("ubuntu-26.04"), "{id}: {block}");
        assert!(!seal.contains(&format!("{id}__hosted:")));
        assert!(!seal.contains(&format!("{id}__local:")));
    }
    let ledger = rendered_job_block(workflow, "ledger-cli")?;
    assert!(ledger.contains("Build ledger CLI"));
    assert!(ledger.contains("ubuntu-26.04"));
    assert!(!workflow.contains("ledger-cli__hosted:"));
    assert!(!workflow.contains("ledger-cli__local:"));
    let required = rendered_job_block(workflow, "required")?;
    assert!(!required.contains("ledger-cli"));
    let job = prepared
        .workflow
        .ir
        .jobs
        .get("ledger-cli")
        .ok_or("missing ledger")?;
    let upload = job
        .steps
        .iter()
        .find_map(|step| match &step.kind {
            StepKind::Action { uses, with, .. } if uses.starts_with("actions/upload-artifact@") => {
                with.get("name")
            }
            _ => None,
        })
        .ok_or("missing ledger upload")?;
    assert_eq!(Some(upload), job.outputs.get("artifact_name"));
    preview::assert_preview_render(root)?;
    let scoped = fs::read_to_string(&config_path)?.replace(
        "task = \"fixture-check\"",
        "task = \"fixture-check\"\nselection = \"declared_inputs\"",
    );
    fs::write(&config_path, scoped)?;
    let stable = prepare(root)?;
    let output = tempfile::tempdir()?;
    let destination = output.path().join("rejected-stable");
    let error = velnor_actions_orchestrator::generate(
        &stable,
        &velnor_actions_orchestrator::GenerateOptions {
            output_dir: Some(destination.clone()),
        },
    )
    .err()
    .ok_or("stable scoped generation was accepted")?;
    assert!(
        error
            .to_string()
            .contains("declared_inputs_requires_approved_preview_controller")
    );
    assert!(!destination.exists());
    preview::assert_preview_render(root)?;
    fs::create_dir_all(root.join("docs"))?;
    fs::write(root.join("docs/guide.md"), "baseline\n")?;
    let base = commit(root, "configure scoped check")?;
    fs::write(
        root.join("crates/maintenance/src/lib.rs"),
        "pub fn changed() {}\n",
    )?;
    let affected = commit(root, "affect check")?;
    assert_dispositions(
        root,
        &base,
        &affected,
        &lanes,
        &inventory,
        required_ids,
        "run",
    )?;
    assert_hosted_wire(root, &base, &affected, &hosted_lanes)?;
    fs::write(root.join("docs/guide.md"), "changed\n")?;
    let unrelated = commit(root, "change unrelated crate")?;
    assert_dispositions(
        root,
        &affected,
        &unrelated,
        &lanes,
        &inventory,
        required_ids,
        "not_affected",
    )?;
    Ok(())
}

fn assert_dispositions(
    root: &std::path::Path,
    base: &str,
    head: &str,
    lanes: &std::collections::BTreeMap<String, Vec<velnor_actions_contract::NamedCheckLane>>,
    inventory: &velnor_actions_contract::LaneInventory,
    required_ids: &[String],
    expected: &str,
) -> Result<(), Box<dyn Error>> {
    let request = json!({"schema": 1, "op": "plan-v1", "run_key": "r190001-a1", "event": "pull_request", "base": base, "head": head, "root": root, "named_check_lanes": lanes});
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let plan: Plan = serde_json::from_value(response["plan"].clone())?;
    assert_tampering_rejected(&plan)?;
    if expected == "not_affected" {
        assert_required_lane_binding(&response, inventory, required_ids)?;
    }
    let jobs = response["plan"]["jobs"]
        .as_array()
        .ok_or("missing dispositions")?;
    for id in [
        "check-maintenance-helpers__hosted",
        "check-maintenance-helpers__local",
    ] {
        let job = jobs
            .iter()
            .find(|job| job["job_id"] == id)
            .ok_or_else(|| format!("missing {id}"))?;
        assert_eq!(job["disposition"], expected, "{id}");
    }
    Ok(())
}

fn assert_tampering_rejected(plan: &Plan) -> Result<(), Box<dyn Error>> {
    plan.validate()?;
    let indices: Vec<_> = plan
        .jobs
        .iter()
        .enumerate()
        .filter_map(|(index, job)| {
            job.lane
                .as_ref()
                .filter(|lane| lane.logical_job_id.starts_with("check-"))
                .map(|_| index)
        })
        .collect();
    assert_eq!(indices.len(), 2);
    for index in &indices {
        let mut tampered = plan.clone();
        tampered.jobs[*index].lane = None;
        assert!(
            tampered.validate().is_err(),
            "missing lane ownership accepted"
        );
    }
    let mut duplicate = plan.clone();
    duplicate.jobs[indices[1]]
        .lane
        .as_mut()
        .ok_or("missing lane")?
        .lane_variant = duplicate.jobs[indices[0]]
        .lane
        .as_ref()
        .ok_or("missing lane")?
        .lane_variant;
    assert!(
        duplicate.validate().is_err(),
        "duplicate lane variant accepted"
    );

    let mut other_task = plan.clone();
    "check-other".clone_into(
        &mut other_task.jobs[indices[1]]
            .lane
            .as_mut()
            .ok_or("missing lane")?
            .logical_job_id,
    );
    assert!(
        other_task.validate().is_err(),
        "mismatched task identity accepted"
    );

    let mut disagreement = plan.clone();
    let lane = &mut disagreement.jobs[indices[1]];
    lane.disposition = match lane.disposition {
        JobDisposition::Run => JobDisposition::NotAffected,
        JobDisposition::NotAffected => JobDisposition::Run,
    };
    assert!(
        disagreement.validate().is_err(),
        "mismatched dispositions accepted"
    );
    Ok(())
}

fn assert_hosted_wire(
    root: &std::path::Path,
    base: &str,
    head: &str,
    lanes: &std::collections::BTreeMap<String, Vec<velnor_actions_contract::NamedCheckLane>>,
) -> Result<(), Box<dyn Error>> {
    let request = json!({"schema": 1, "op": "plan-v1", "run_key": "r190001-a1", "event": "pull_request", "base": base, "head": head, "root": root, "named_check_lanes": lanes});
    let response: Value = serde_json::from_str(&plan_internal(&request.to_string())?)?;
    let jobs = response["plan"]["jobs"]
        .as_array()
        .ok_or("missing hosted jobs")?;
    let check = jobs
        .iter()
        .find(|job| job["job_id"] == "check-maintenance-helpers")
        .ok_or("missing hosted check")?;
    assert_eq!(check["disposition"], "run");
    assert!(jobs.iter().all(|job| job.get("lane").is_none()));
    Ok(())
}

fn assert_required_lane_binding(
    response: &Value,
    inventory: &velnor_actions_contract::LaneInventory,
    required_ids: &[String],
) -> Result<(), Box<dyn Error>> {
    let planned = response["plan"]["jobs"].as_array().ok_or("missing jobs")?;
    let required_jobs: Vec<Value> = required_ids.iter().map(|id| {
        let logical = id.strip_suffix("__hosted").or_else(|| id.strip_suffix("__local")).unwrap_or(id);
        let disposition = planned.iter().find(|job| job["job_id"] == *id)
            .or_else(|| planned.iter().find(|job| job["job_id"] == logical))
            .map(|job| job["disposition"].as_str().unwrap_or_default());
        json!({"job_id": id, "conclusion": if disposition == Some("not_affected") { "skipped" } else { "success" }})
    }).collect();
    let mut request = json!({
        "schema": 1, "run_key": "r190001-a1", "actual_event": "pull_request",
        "plan": response["plan"], "matrix": response["matrix"],
        "matrix_reports": [], "task_reports": [],
        "required_job_ids": required_ids,
        "required_jobs": required_jobs,
        "lane_inventory": inventory,
    });
    let merge = |request: &Value| -> Result<Value, Box<dyn Error>> {
        Ok(serde_json::from_str(&merge_internal_with_context(
            &request.to_string(),
            &MergeContext::detached(),
        )?)?)
    };
    let baseline = merge(&request)?;
    assert!(
        baseline["status"] == "passed" || baseline["status"] == "no_work",
        "baseline failed: {baseline}"
    );
    let results = request["required_jobs"]
        .as_array_mut()
        .ok_or("missing results")?;
    let local_result = results
        .iter_mut()
        .find(|job| job["job_id"] == "check-maintenance-helpers__local")
        .ok_or("local result missing")?;
    local_result["conclusion"] = json!("success");
    let mut mutant: Plan = serde_json::from_value(request["plan"].clone())?;
    let removed_tasks: BTreeSet<String> = mutant
        .obligations
        .iter()
        .filter(|obligation| obligation.job_id == "check-maintenance-helpers")
        .map(|obligation| obligation.task_id.clone())
        .collect();
    mutant
        .obligations
        .retain(|obligation| !removed_tasks.contains(&obligation.task_id));
    mutant.task_ids.retain(|task| !removed_tasks.contains(task));
    mutant
        .edges
        .retain(|edge| !removed_tasks.contains(&edge.from) && !removed_tasks.contains(&edge.to));
    mutant.jobs.retain(|job| {
        job.lane
            .as_ref()
            .is_none_or(|lane| lane.logical_job_id != "check-maintenance-helpers")
    });
    mutant
        .lane_inventory
        .as_mut()
        .ok_or("missing embedded inventory")?
        .routes
        .retain(|route| route.logical_job_id != "check-maintenance-helpers");
    mutant.validate()?;
    request["plan"] = serde_json::to_value(mutant)?;
    let tampered = merge(&request)?;
    assert!(
        tampered["miss_reasons"]
            .as_array()
            .is_some_and(|reasons| reasons.contains(&json!("trust_scope_mismatch"))),
        "removed pair accepted: {tampered}"
    );
    Ok(())
}
