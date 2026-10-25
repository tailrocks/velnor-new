//! Merge gate cases: round-trip, precedence, no-work (tamper: `impl_merge_tamper`).

use crate::impl_common::{TestResult, merge_fixture, passing_reports, plan_for_source_change};
use velnor_actions_contract::{
    FinalReport, FinalStatus, MatrixReport, MatrixStatus, Plan, TaskReport, TaskStatus,
};

/// Merge one request and parse the final report.
pub(crate) fn merge(
    request: &serde_json::Value,
) -> Result<FinalReport, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&merge_fixture(&request.to_string())?)?)
}

/// Canonical merge request for a plan with report/job overrides.
///
/// The declared inventory mirrors the supplied results, so callers testing
/// inventory mismatches must overwrite `required_job_ids` explicitly.
/// Consistent per-task files attach automatically; callers testing task
/// evidence gaps must overwrite `task_reports` explicitly.
pub(crate) fn merge_request(
    plan: &Plan,
    matrix: &serde_json::Value,
    reports: &serde_json::Value,
    jobs: &serde_json::Value,
) -> serde_json::Value {
    let jobs = with_matrix_owner_results(plan, jobs);
    let ids: Vec<serde_json::Value> = job_ids(&jobs);
    let plan_value = serde_json::to_value(plan).unwrap_or(serde_json::Value::Null);
    serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": plan_value.get("event").cloned().unwrap_or(serde_json::Value::Null),
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
        "task_reports": task_reports_for(&plan_value, reports),
        "required_job_ids": ids,
        "required_jobs": jobs,
    })
}

/// Add successful Required results for every owner in this plan matrix.
pub(crate) fn with_matrix_owner_results(
    plan: &Plan,
    jobs: &serde_json::Value,
) -> Vec<serde_json::Value> {
    let mut jobs = jobs.as_array().cloned().unwrap_or_default();
    let owner_conclusion = jobs
        .iter()
        .find(|job| job.get("job_id").and_then(serde_json::Value::as_str) == Some("plan"))
        .and_then(|job| job.get("conclusion"))
        .and_then(serde_json::Value::as_str)
        .map(|conclusion| {
            if conclusion == "success" {
                "success"
            } else {
                "skipped"
            }
        });
    let Some(owner_conclusion) = owner_conclusion else {
        return jobs;
    };
    let mut present: std::collections::BTreeSet<String> = jobs
        .iter()
        .filter_map(|job| job.get("job_id")?.as_str().map(str::to_owned))
        .collect();
    for job_id in plan.matrix.include.iter().map(|entry| &entry.job_id) {
        if present.insert(job_id.clone()) {
            jobs.push(serde_json::json!({"job_id": job_id, "conclusion": owner_conclusion}));
        }
    }
    jobs
}

/// Required owner IDs corresponding to [`with_matrix_owner_results`].
pub(crate) fn job_ids(jobs: &[serde_json::Value]) -> Vec<serde_json::Value> {
    jobs.iter().map(|job| job["job_id"].clone()).collect()
}

/// Consistent per-task files for a plan plus its matrix reports.
///
/// Mirrors every matrix task entry into the `TaskReport` file the merge
/// requires: same report ID, task, status, and exit, with the plan
/// obligation's digest. Entries without an obligation digest are
/// skipped; the merge fails those legs closed on incoherence.
pub(crate) fn task_reports_for(
    plan: &serde_json::Value,
    reports: &serde_json::Value,
) -> serde_json::Value {
    let mut digests = std::collections::BTreeMap::new();
    let obligations = plan
        .get("obligations")
        .and_then(serde_json::Value::as_array);
    for obligation in obligations.into_iter().flatten() {
        let id = obligation
            .get("task_id")
            .and_then(serde_json::Value::as_str);
        let digest = obligation
            .get("task_digest")
            .and_then(serde_json::Value::as_str);
        if let (Some(id), Some(digest)) = (id, digest) {
            digests.insert(id, digest);
        }
    }
    let mut out = Vec::new();
    for report in reports.as_array().into_iter().flatten() {
        let tasks = report.get("tasks").and_then(serde_json::Value::as_array);
        for task in tasks.into_iter().flatten() {
            let id = task.get("task_id").and_then(serde_json::Value::as_str);
            let report_id = task
                .get("task_report_id")
                .and_then(serde_json::Value::as_str);
            let (Some(id), Some(report_id)) = (id, report_id) else {
                continue;
            };
            let Some(digest) = digests.get(id) else {
                continue;
            };
            let platform_binding = unavailable_platform_binding(
                plan,
                report.get("matrix_key").and_then(serde_json::Value::as_str),
            );
            let blocked =
                task.get("status").and_then(serde_json::Value::as_str) == Some("not_selected");
            out.push(serde_json::json!({
                "schema": TaskReport::SCHEMA,
                "task_report_id": report_id,
                "run_key": plan.get("run_key").and_then(serde_json::Value::as_str).unwrap_or("local"),
                "event": plan.get("event"),
                "trust": plan.get("trust"),
                "matrix_id": report.get("matrix_id"),
                "matrix_key": report.get("matrix_key"),
                "task_id": id,
                "task_digest": digest,
                "status": task.get("status"),
                "not_selected_reason": if blocked {
                    serde_json::json!("upstream_failed")
                } else {
                    serde_json::Value::Null
                },
                "cache": {"layer": "task", "key": "", "result": "not_attempted"},
                "platform_binding": platform_binding,
                "exit_code": task.get("exit_code"),
                "duration_ms": null,
                "outputs": [],
            }));
        }
    }
    out.sort_by(|left, right| {
        let l = left
            .get("task_report_id")
            .and_then(serde_json::Value::as_str);
        let r = right
            .get("task_report_id")
            .and_then(serde_json::Value::as_str);
        l.cmp(&r)
    });
    serde_json::Value::Array(out)
}

fn unavailable_platform_binding(
    plan: &serde_json::Value,
    matrix_key: Option<&str>,
) -> serde_json::Value {
    let planned_platform_id = plan
        .pointer("/matrix/include")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|entry| entry.get("matrix_key").and_then(serde_json::Value::as_str) == matrix_key)
        .and_then(|entry| entry.pointer("/planned_platform/platform_id"));
    serde_json::json!({
        "state": "unavailable",
        "planned_platform_id": planned_platform_id,
        "runner_environment": "unknown",
        "reason": "observation_not_recorded"
    })
}

/// One successful required job.
pub(crate) fn success_jobs() -> serde_json::Value {
    serde_json::json!([{"job_id": "plan", "conclusion": "success"}])
}

/// Rewrite the single task of a passing report, keeping counts coherent.
fn set_task(
    report: &mut MatrixReport,
    status: TaskStatus,
    aggregate: MatrixStatus,
) -> Result<(), Box<dyn std::error::Error>> {
    let task = report
        .tasks
        .first_mut()
        .ok_or_else(|| std::io::Error::other("report without tasks"))?;
    task.status = status;
    task.exit_code = i32::from(status == TaskStatus::Failed);
    report.status = aggregate;
    report.reused = 0;
    report.executed = 0;
    report.empty_partition = 0;
    report.not_selected = 0;
    report.failed = 0;
    report.cancelled = 0;
    match status {
        TaskStatus::Reused => report.reused = 1,
        TaskStatus::Executed => report.executed = 1,
        TaskStatus::EmptyPartition => report.empty_partition = 1,
        TaskStatus::NotSelected => report.not_selected = 1,
        TaskStatus::Failed => report.failed = 1,
        TaskStatus::Cancelled => report.cancelled = 1,
    }
    report.validate()?;
    Ok(())
}

/// A well-formed report for a matrix ID outside the plan.
fn foreign_report() -> Result<MatrixReport, Box<dyn std::error::Error>> {
    let matrix_id = "stack:rust|task:stack/rust/foreign/build/default";
    let matrix_key = velnor_actions_contract::matrix_key_for_id(matrix_id)?;
    let digest = velnor_actions_contract::digest_b3(b"foreign-task");
    let task_report_id =
        velnor_actions_contract::task_report_id_for_task("local", &matrix_key, &digest)?;
    let task_id = "stack/rust/foreign/build/default".to_owned();
    Ok(MatrixReport {
        schema: 1,
        report_id: velnor_actions_contract::report_id_for_matrix("local", &matrix_key)?,
        run_key: "local".to_owned(),
        matrix_id: matrix_id.to_owned(),
        matrix_key,
        status: MatrixStatus::Passed,
        expected_task_ids: vec![task_id.clone()],
        task_report_ids: vec![task_report_id.clone()],
        tasks: vec![velnor_actions_contract::MatrixTaskEntry {
            task_report_id,
            task_id,
            status: TaskStatus::Executed,
            exit_code: 0,
        }],
        selected: 1,
        reused: 0,
        executed: 1,
        empty_partition: 0,
        not_selected: 0,
        failed: 0,
        cancelled: 0,
    })
}

#[path = "impl_merge_cases.rs"]
mod cases;
