// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use serde_json::{Value, json};
use velnor_actions_orchestrator::{
    MergeContext, PlanOutputMode, merge_internal_with_context, merge_passed, plan_internal,
    plan_outputs,
};

use super::bun_planning_fixture::{BunPlanningRepo, ChangedMember, make_bun_planning_repo};

const REQUIRED_LOGICAL_JOBS: [&str; 4] = [
    "bun-fixture-ui",
    "bun-fixture-web",
    "bun-fixture-web--unit",
    "bun-fixture-web--browser",
];

#[test]
fn configured_bun_both_projects_single_job_dispositions_into_required() -> Result<(), Box<dyn Error>>
{
    let repo = make_bun_planning_repo(ChangedMember::Web)?;
    assert_ne!(
        repo.head, repo.merge,
        "the fixture must preserve a separate pull request head"
    );
    let (request, response) = plan(&repo, "r190001-a1")?;
    assert_eq!(
        plan_jobs(&response)?,
        expected_jobs("not_affected", "run"),
        "a web-only change must keep the independent UI member skipped"
    );
    let outputs = plan_outputs(&request, PlanOutputMode::Static)?;
    assert_eq!(
        outputs.not_affected_jobs,
        ",bun-fixture-ui,bun-fixture-ui--assets,"
    );
    let passed = merge_verdict(&response, Some("bun-fixture-ui"), None)?;
    assert!(merge_passed(&passed.to_string())?);
    let skipped = merge_verdict(&response, Some("bun-fixture-ui"), Some("bun-fixture-web"))?;
    assert!(!merge_passed(&skipped.to_string())?);
    assert_miss_reason(&skipped, "planned_job_skipped")?;
    Ok(())
}

#[test]
fn bun_workspace_dependency_runs_selected_workspace_members() -> Result<(), Box<dyn Error>> {
    let repo = make_bun_planning_repo(ChangedMember::Ui)?;
    let (request, response) = plan(&repo, "r190002-a1")?;
    assert_eq!(
        plan_jobs(&response)?,
        expected_jobs("run", "run"),
        "changing the workspace dependency must select its web consumer"
    );
    let outputs = plan_outputs(&request, PlanOutputMode::Static)?;
    assert!(
        outputs.not_affected_jobs.is_empty(),
        "an affected workspace dependency must run every Bun output"
    );
    let passed = merge_verdict(&response, None, None)?;
    assert!(merge_passed(&passed.to_string())?);
    let skipped = merge_verdict(&response, None, Some("bun-fixture-web--browser"))?;
    assert!(!merge_passed(&skipped.to_string())?);
    assert_miss_reason(&skipped, "planned_job_skipped")?;
    Ok(())
}

fn plan(repo: &BunPlanningRepo, run_key: &str) -> Result<(String, Value), Box<dyn Error>> {
    let input = json!({
        "schema": 1,
        "op": "plan-v1",
        "run_key": run_key,
        "base": repo.base.as_str(),
        "head": repo.head.as_str(),
        "event": "pull_request",
        "root": repo.repo.path().to_string_lossy(),
    });
    let request = serde_json::to_string(&input)?;
    let response = plan_internal(&request)?;
    let parsed = serde_json::from_str(&response)?;
    Ok((response, parsed))
}

fn expected_jobs(ui: &str, web: &str) -> BTreeMap<String, String> {
    let mut expected = BTreeMap::new();
    for (logical_id, disposition) in [
        ("bun-fixture-ui", ui),
        ("bun-fixture-ui--assets", ui),
        ("bun-fixture-web", web),
        ("bun-fixture-web--unit", web),
        ("bun-fixture-web--browser", web),
        ("bun-fixture-web--assets", web),
    ] {
        expected.insert(logical_id.to_owned(), disposition.to_owned());
    }
    expected
}

fn plan_jobs(response: &Value) -> Result<BTreeMap<String, String>, Box<dyn Error>> {
    let jobs = response
        .get("plan")
        .and_then(|plan| plan.get("jobs"))
        .and_then(Value::as_array)
        .ok_or("plan response has no job dispositions")?;
    jobs.iter()
        .map(|job| {
            let id = job
                .get("job_id")
                .and_then(Value::as_str)
                .ok_or("plan job has no id")?;
            let disposition = job
                .get("disposition")
                .and_then(Value::as_str)
                .ok_or("plan job has no disposition")?;
            Ok((id.to_owned(), disposition.to_owned()))
        })
        .collect()
}

fn merge_verdict(
    response: &Value,
    skipped_member: Option<&str>,
    skipped_job: Option<&str>,
) -> Result<Value, Box<dyn Error>> {
    let run_key = response
        .get("plan")
        .and_then(|plan| plan.get("run_key"))
        .and_then(Value::as_str)
        .ok_or("plan response has no run key")?;
    let ids: Vec<String> = REQUIRED_LOGICAL_JOBS
        .iter()
        .map(|id| (*id).to_owned())
        .collect();
    let required_jobs: Vec<Value> = ids
        .iter()
        .map(|id| {
            let skipped = skipped_member.is_some_and(|member| id.starts_with(member))
                || skipped_job == Some(id.as_str());
            json!({
                "job_id": id,
                "conclusion": if skipped { "skipped" } else { "success" },
            })
        })
        .collect();
    let request = json!({
        "schema": 1,
        "run_key": run_key,
        "actual_event": "pull_request",
        "plan": response["plan"],
        "matrix": response["matrix"],
        "matrix_reports": [],
        "task_reports": [],
        "required_job_ids": ids,
        "required_jobs": required_jobs,
    });
    let merged =
        merge_internal_with_context(&serde_json::to_string(&request)?, &MergeContext::detached())?;
    Ok(serde_json::from_str(&merged)?)
}

fn assert_miss_reason(report: &Value, reason: &str) -> Result<(), Box<dyn Error>> {
    let reasons = report
        .get("miss_reasons")
        .and_then(Value::as_array)
        .ok_or("Required report has no miss reasons")?;
    assert!(
        reasons.iter().any(|value| value.as_str() == Some(reason)),
        "Required report does not explain {reason}: {reasons:?}"
    );
    Ok(())
}
