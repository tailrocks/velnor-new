//! Merge gate cases: round-trip, precedence, no-work (tamper: `impl_merge_tamper`).

use velnor_actions_contract::{
    FinalReport, FinalStatus, MatrixReport, MatrixStatus, Plan, TaskReport, TaskStatus,
};
use velnor_actions_orchestrator::merge_internal;

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};

/// Merge one request and parse the final report.
pub(crate) fn merge(
    request: &serde_json::Value,
) -> Result<FinalReport, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&merge_internal(
        &request.to_string(),
    )?)?)
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
    let ids: Vec<serde_json::Value> = jobs
        .as_array()
        .map(|jobs| jobs.iter().map(|job| job["job_id"].clone()).collect())
        .unwrap_or_default();
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

#[test]
fn round_trip_passed_with_counts() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.task_ids.is_empty(), "fixture must select work");
    let reports = passing_reports(&plan)?;
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::Passed);
    assert_eq!(final_report.report_id, "final-local");
    assert_eq!(final_report.plan_id, plan.plan_id);
    assert_eq!(final_report.counts.selected as usize, plan.task_ids.len());
    assert_eq!(final_report.counts.executed as usize, plan.task_ids.len());
    assert_eq!(final_report.counts.covered, 0);
    assert_eq!(final_report.counts.failed, 0);
    assert_eq!(final_report.counts.cancelled, 0);
    assert_eq!(final_report.counts.blocked, 0);
    assert_eq!(final_report.counts.not_run, 0);
    let mut artifacts: Vec<String> = plan
        .matrix
        .include
        .iter()
        .map(|entry| entry.artifact_id.clone())
        .collect();
    artifacts.sort();
    artifacts.dedup();
    assert_eq!(final_report.downloaded_artifact_ids, artifacts);
    assert_eq!(
        final_report.expected_report_ids.len(),
        plan.matrix.include.len()
    );
    Ok(())
}

#[test]
fn one_failed_crate_fails_required_with_counts() -> TestResult {
    let (_repo, plan) = crate::impl_orch_core::plan_for_partial_change()?;
    let mut reports = passing_reports(&plan)?;
    let mut failed = 0;
    for report in reports
        .iter_mut()
        .filter(|report| report.tasks.iter().any(|task| task.task_id.contains("/a/")))
    {
        set_task(report, TaskStatus::Failed, MatrixStatus::Failed)?;
        failed += 1;
    }
    assert!(failed > 0 && failed < reports.len(), "one crate fails");
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::Failed);
    assert_eq!(final_report.counts.failed as usize, failed);
    assert_eq!(
        final_report.counts.executed as usize,
        reports.len() - failed,
        "sibling crate still proves its work"
    );
    assert_eq!(final_report.counts.not_run, 0);
    Ok(())
}

#[test]
fn precedence_matrix() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let first = 0;
    let run = |reports: Vec<MatrixReport>,
               jobs: serde_json::Value|
     -> Result<FinalStatus, Box<dyn std::error::Error>> {
        let request = merge_request(&plan, &matrix, &serde_json::to_value(&reports)?, &jobs);
        Ok(merge(&request)?.status)
    };

    // Failed task beats everything below planning.
    let mut reports = passing_reports(&plan)?;
    set_task(
        &mut reports[first],
        TaskStatus::Failed,
        MatrixStatus::Failed,
    )?;
    assert_eq!(run(reports, success_jobs())?, FinalStatus::Failed);

    // Cancelled beats missing but loses to failure.
    let mut reports = passing_reports(&plan)?;
    set_task(
        &mut reports[first],
        TaskStatus::Cancelled,
        MatrixStatus::Cancelled,
    )?;
    reports.pop();
    assert_eq!(run(reports, success_jobs())?, FinalStatus::Cancelled);

    // Missing alone is not-run, never success.
    let mut reports = passing_reports(&plan)?;
    reports.pop();
    assert_eq!(run(reports, success_jobs())?, FinalStatus::NotRun);

    // Failed beats cancelled plus missing.
    let mut reports = passing_reports(&plan)?;
    set_task(
        &mut reports[first],
        TaskStatus::Failed,
        MatrixStatus::Failed,
    )?;
    if reports.len() > 1 {
        set_task(
            &mut reports[1],
            TaskStatus::Cancelled,
            MatrixStatus::Cancelled,
        )?;
    }
    reports.pop();
    assert_eq!(run(reports, success_jobs())?, FinalStatus::Failed);

    // Required-job conclusions join the same precedence.
    let reports = passing_reports(&plan)?;
    let failed_job = serde_json::json!([{"job_id": "plan", "conclusion": "failure"}]);
    assert_eq!(run(reports.clone(), failed_job)?, FinalStatus::Failed);
    let cancelled_job = serde_json::json!([{"job_id": "plan", "conclusion": "cancelled"}]);
    assert_eq!(run(reports.clone(), cancelled_job)?, FinalStatus::Cancelled);
    let skipped_job = serde_json::json!([{"job_id": "plan", "conclusion": "skipped"}]);
    assert_eq!(run(reports.clone(), skipped_job)?, FinalStatus::NotRun);

    // Duplicate reports are not-run, never success.
    let mut reports = passing_reports(&plan)?;
    reports.push(reports[first].clone());
    assert_eq!(run(reports, success_jobs())?, FinalStatus::NotRun);

    // Unexpected reports fail planning, even beside failures.
    let mut reports = passing_reports(&plan)?;
    set_task(
        &mut reports[first],
        TaskStatus::Failed,
        MatrixStatus::Failed,
    )?;
    reports.push(foreign_report()?);
    assert_eq!(run(reports, success_jobs())?, FinalStatus::PlanningFailed);

    // Matrix/plan disagreement fails planning.
    let reports = passing_reports(&plan)?;
    let mut trimmed = serde_json::to_value(&plan.matrix)?;
    trimmed["include"]
        .as_array_mut()
        .ok_or_else(|| std::io::Error::other("matrix shape"))?
        .pop();
    let request = merge_request(
        &plan,
        &trimmed,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::PlanningFailed);
    Ok(())
}
