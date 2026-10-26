// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn merge_request(
    request: &mut Value,
    keep_observation_tokens: bool,
) -> Result<FinalReport, Box<dyn Error>> {
    request["required_job_ids"] = json!([JOB_ID]);
    request["required_jobs"] = json!([{"job_id": JOB_ID, "conclusion": "success"}]);
    request["actual_event"] = json!("pull_request");
    let kept: Vec<Value> = if keep_observation_tokens {
        assembly_errors(request)
            .into_iter()
            .filter(|error| error.contains("observation"))
            .map(Value::String)
            .collect()
    } else {
        Vec::new()
    };
    request["assembly_errors"] = Value::Array(kept);
    let output = merge_internal_with_context(&request.to_string(), &MergeContext::detached())?;
    Ok(serde_json::from_str(&output)?)
}

fn conclusion_of(report: &FinalReport, job_id: &str) -> Result<JobConclusion, Box<dyn Error>> {
    report
        .required_job_results
        .iter()
        .find(|job| job.job_id == job_id)
        .map(|job| job.conclusion)
        .ok_or_else(|| fail("required result missing for planned job"))
}

fn check_transport_preserved() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let report_id = report
        .get("task_report_id")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("staged task report lost its id"))?
        .to_owned();
    let observation = valid_observation(&entry, &report_id)?;
    let temp = tempfile::tempdir()?;
    let tree = tree_of(&plan, &matrix, &report, &entry, &report_id);
    stage_tree(temp.path(), &tree, Some(&observation), false)?;
    let mut request = assembled(temp.path())?;
    let tasks = request
        .get("task_reports")
        .and_then(Value::as_array)
        .ok_or_else(|| fail("assembled request lost its task reports"))?;
    if !tasks
        .iter()
        .any(|task| task.get("task_report_id").and_then(Value::as_str) == Some(report_id.as_str()))
    {
        return Err(fail("assembled request lost the observed unit report"));
    }
    if !observation_tokens(&request).is_empty() {
        return Err(fail("valid bound observation left retrieval tokens"));
    }
    let merged = merge_request(&mut request, false)?;
    if merged.status != FinalStatus::Passed {
        return Err(fail("bound observation did not preserve Required pass"));
    }
    if merged.run_key != RUN_KEY
        || merged.plan_id != plan.plan_id
        || merged.report_id != format!("final-{RUN_KEY}")
    {
        return Err(fail("bound observation rewrote the source run binding"));
    }
    if merged.downloaded_artifact_ids != [entry.artifact_id.clone()] {
        return Err(fail("bound observation lost the covered artifact binding"));
    }
    if merged.counts.selected != 1 || merged.counts.executed != 1 {
        return Err(fail("bound observation miscounted the covered unit"));
    }
    if conclusion_of(&merged, JOB_ID)? != JobConclusion::Success {
        return Err(fail(
            "bound observation lost the successful unit conclusion",
        ));
    }
    if !merged.miss_reasons.is_empty() {
        return Err(fail("bound observation left unexpected miss reasons"));
    }
    let encoded = serde_json::to_string(&merged)?;
    if encoded.contains("gc1-t36") || encoded.contains(SESSION_ID) {
        return Err(fail("final artifact carried observation payload bytes"));
    }
    Ok(())
}

fn check_loose_files_ignored() -> Result<(), Box<dyn Error>> {
    let (plan, matrix, report, entry) = check_run_fixture()?;
    let report_id = report
        .get("task_report_id")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("staged task report lost its id"))?
        .to_owned();
    let temp = tempfile::tempdir()?;
    let tree = tree_of(&plan, &matrix, &report, &entry, &report_id);
    stage_tree(temp.path(), &tree, None, true)?;
    let mut request = assembled(temp.path())?;
    if !observation_tokens(&request).is_empty() {
        return Err(fail("loose files were treated as observation evidence"));
    }
    let merged = merge_request(&mut request, false)?;
    if merged.status != FinalStatus::Passed {
        return Err(fail("loose files broke Required pass"));
    }
    Ok(())
}

#[test]
fn gc1_t36_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_transport_preserved()?;
    check_loose_files_ignored()?;
    Ok(())
}

pub(crate) fn check_invalid_rejected(
    plan: &Plan,
    matrix: &Value,
    report: &Value,
    entry: &MatrixEntry,
    report_id: &str,
    observation: &str,
    case: &str,
) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let tree = tree_of(plan, matrix, report, entry, report_id);
    stage_tree(temp.path(), &tree, Some(observation), false)?;
    let mut request = assembled(temp.path())?;
    let expected = format!("corrupt_observation:{report_id}");
    let tokens = observation_tokens(&request);
    if !tokens
        .iter()
        .any(|token| token == &expected || token.starts_with("unparsable_observation"))
    {
        return Err(fail(case));
    }
    let merged = merge_request(&mut request, true)?;
    if merged.status != FinalStatus::PlanningFailed {
        return Err(fail(case));
    }
    if !merged.miss_reasons.contains(&"cache_corrupt".to_owned()) {
        return Err(fail(case));
    }
    Ok(())
}

pub(crate) fn staged_report_id(report: &Value) -> Result<String, Box<dyn Error>> {
    report
        .get("task_report_id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| fail("staged task report lost its id"))
}
