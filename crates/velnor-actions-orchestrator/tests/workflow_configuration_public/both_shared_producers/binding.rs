// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;

use serde_json::{Value, json};
use velnor_actions_contract::{JobDisposition, Plan};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context, plan_internal};

pub(super) fn assert_dispositions(
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

pub(super) fn assert_hosted_wire(
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
