// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::JobConclusion;
use velnor_actions_workflow_renderer::render::TASK_JOB_ID;

pub(crate) const NEEDS_ENV: &str = "VELNOR_NEEDS_JSON";

pub(crate) fn parse_needs(
    needs: Option<&str>,
    expected: Option<&str>,
    errors: &mut Vec<String>,
) -> (Vec<String>, Vec<serde_json::Value>) {
    let Some(text) = needs.filter(|text| !text.trim().is_empty()) else {
        errors.push("missing_needs_channel".to_owned());
        return (Vec::new(), Vec::new());
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(text) else {
        errors.push("unparsable_needs".to_owned());
        return (Vec::new(), Vec::new());
    };
    let Some(map) = parsed.as_object() else {
        errors.push("unparsable_needs".to_owned());
        return (Vec::new(), Vec::new());
    };
    if map.is_empty() {
        errors.push("empty_needs".to_owned());
        return (Vec::new(), Vec::new());
    }
    let Some(want) = parse_expected(expected, errors) else {
        return (Vec::new(), Vec::new());
    };
    let mut observed = Vec::new();
    let mut results = Vec::new();
    for (job_id, entry) in map {
        if job_id.trim().is_empty() {
            errors.push("bad_needs_job".to_owned());
            continue;
        }
        if job_id == TASK_JOB_ID {
            continue;
        }
        let Some(conclusion) = needs_conclusion(entry) else {
            errors.push(format!("bad_needs_result:{job_id}"));
            continue;
        };
        observed.push(job_id.clone());
        results.push(serde_json::json!({"job_id": job_id, "conclusion": conclusion.as_str()}));
    }
    observed.sort();
    results.sort_by(|left, right| report_job(left).cmp(report_job(right)));
    if observed != want {
        errors.push("needs_inventory_mismatch".to_owned());
    }
    (want, results)
}

fn parse_expected(expected: Option<&str>, errors: &mut Vec<String>) -> Option<Vec<String>> {
    let Some(text) = expected.filter(|text| !text.trim().is_empty()) else {
        errors.push("missing_needs_expected".to_owned());
        return None;
    };
    let Ok(parsed) = serde_json::from_str::<Vec<String>>(text) else {
        errors.push("missing_needs_expected".to_owned());
        return None;
    };
    let mut want: Vec<String> = parsed
        .into_iter()
        .filter(|job_id| job_id != TASK_JOB_ID)
        .collect();
    want.sort();
    want.dedup();
    if want.is_empty() {
        errors.push("missing_needs_expected".to_owned());
        return None;
    }
    Some(want)
}

fn needs_conclusion(entry: &serde_json::Value) -> Option<JobConclusion> {
    let raw = entry
        .as_str()
        .or_else(|| entry.get("result").and_then(serde_json::Value::as_str))?;
    match raw {
        "success" => Some(JobConclusion::Success),
        "failure" => Some(JobConclusion::Failure),
        "cancelled" => Some(JobConclusion::Cancelled),
        "skipped" => Some(JobConclusion::Skipped),
        _ => None,
    }
}

fn report_job(result: &serde_json::Value) -> &str {
    result
        .get("job_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
}
