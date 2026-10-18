//! Merge gate cases: round-trip, precedence, no-work.

use super::*;

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
