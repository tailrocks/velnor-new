//! Merge gate cases: round-trip, precedence, tamper rejection, no-work.

use velnor_actions_contract::{
    FinalReport, FinalStatus, MatrixReport, MatrixStatus, Plan, TaskStatus,
};
use velnor_actions_orchestrator::merge_internal;

use crate::impl_common::{
    TestResult, config_with_branch, err_of, git, git_line, make_repo, passing_reports,
    plan_for_source_change,
};

/// Merge one request and parse the final report.
fn merge(request: &serde_json::Value) -> Result<FinalReport, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&merge_internal(
        &request.to_string(),
    )?)?)
}

/// Canonical merge request for a plan with report/job overrides.
///
/// The declared inventory mirrors the supplied results, so callers testing
/// inventory mismatches must overwrite `required_job_ids` explicitly.
fn merge_request(
    plan: &Plan,
    matrix: &serde_json::Value,
    reports: &serde_json::Value,
    jobs: &serde_json::Value,
) -> serde_json::Value {
    let ids: Vec<serde_json::Value> = jobs
        .as_array()
        .map(|jobs| jobs.iter().map(|job| job["job_id"].clone()).collect())
        .unwrap_or_default();
    serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "plan": plan,
        "matrix": matrix,
        "matrix_reports": reports,
        "required_job_ids": ids,
        "required_jobs": jobs,
    })
}

/// One successful required job.
fn success_jobs() -> serde_json::Value {
    serde_json::json!([{"job_id": "velnor-plan", "conclusion": "success"}])
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
    assert_eq!(final_report.downloaded_artifact_ids, artifacts);
    assert_eq!(
        final_report.expected_report_ids.len(),
        plan.matrix.include.len()
    );
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
    let failed_job = serde_json::json!([{"job_id": "velnor-plan", "conclusion": "failure"}]);
    assert_eq!(run(reports.clone(), failed_job)?, FinalStatus::Failed);
    let cancelled_job = serde_json::json!([{"job_id": "velnor-plan", "conclusion": "cancelled"}]);
    assert_eq!(run(reports.clone(), cancelled_job)?, FinalStatus::Cancelled);
    let skipped_job = serde_json::json!([{"job_id": "velnor-plan", "conclusion": "skipped"}]);
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

#[test]
fn tampered_reports_rejected() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let run = |reports: Vec<MatrixReport>| -> Result<FinalStatus, Box<dyn std::error::Error>> {
        let request = merge_request(
            &plan,
            &matrix,
            &serde_json::to_value(&reports)?,
            &success_jobs(),
        );
        Ok(merge(&request)?.status)
    };

    // Task-report ID recomputed with the wrong digest fails planning.
    let mut reports = passing_reports(&plan)?;
    let wrong_digest = velnor_actions_contract::digest_b3(b"tampered-task");
    let forged = velnor_actions_contract::task_report_id_for_task(
        "local",
        &reports[0].matrix_key,
        &wrong_digest,
    )?;
    reports[0].tasks[0].task_report_id = forged.clone();
    reports[0].task_report_ids = vec![forged];
    assert_eq!(run(reports)?, FinalStatus::PlanningFailed);

    // Malformed IDs are not-run, never success.
    let mut reports = passing_reports(&plan)?;
    reports[0].tasks[0].task_report_id = "task-broken".to_owned();
    reports[0].task_report_ids = vec!["task-broken".to_owned()];
    assert_eq!(run(reports)?, FinalStatus::NotRun);

    // Summary counts disagreeing with tasks are not-run.
    let mut reports = passing_reports(&plan)?;
    reports[0].executed = 0;
    reports[0].reused = 1;
    assert_eq!(run(reports)?, FinalStatus::NotRun);

    // A valid report bound to another run is not-run.
    let mut reports = passing_reports(&plan)?;
    reports[0].run_key = "r1-a1".to_owned();
    assert_eq!(run(reports)?, FinalStatus::NotRun);
    Ok(())
}

#[test]
fn empty_diff_no_baseline_executes_all() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    git(&["add", "."], root)?;
    git(&["commit", "-m", "one"], root)?;
    let head = git_line(&["rev-parse", "HEAD"], root)?;
    let plan_request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "base": head,
        "head": head,
        "event": "pull_request",
        "root": root.display().to_string(),
    });
    let response = velnor_actions_orchestrator::plan_internal(&plan_request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert!(!plan.obligations.is_empty(), "empty diff keeps universe");
    assert!(!plan.task_ids.is_empty(), "empty diff keeps universe");
    assert!(!plan.matrix.include.is_empty(), "empty diff executes");
    assert!(
        plan.obligations.iter().all(|ob| ob.reason == "unproven"),
        "nothing proven: {:?}",
        plan.obligations
    );

    let reports = passing_reports(&plan)?;
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::to_value(&reports)?,
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);

    // Without reports the unproven legs are not-run, never no-work.
    let request = merge_request(
        &plan,
        &serde_json::to_value(&plan.matrix)?,
        &serde_json::json!([]),
        &success_jobs(),
    );
    assert_eq!(merge(&request)?.status, FinalStatus::NotRun);

    // Malformed merge requests stay hard errors, not reports.
    let err = err_of(
        merge_internal(r#"{"schema":1,"run_key":"local"}"#),
        "truncated merge rejected",
    )?;
    assert!(matches!(
        err,
        velnor_actions_orchestrator::OrchestratorError::Internal { .. }
    ));
    Ok(())
}

#[test]
fn missing_plan_merges_to_planning_failed() -> TestResult {
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "plan": null,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["velnor-plan"],
        "required_jobs": [{"job_id": "velnor-plan", "conclusion": "failure"}],
    });
    let final_report = merge(&request)?;
    final_report.validate()?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert_eq!(final_report.report_id, "final-local");
    assert!(final_report.expected_report_ids.is_empty());
    assert_eq!(final_report.required_job_results.len(), 1);

    // A missing matrix file with a present plan is also planning_failed.
    let (_repo, plan) = plan_for_source_change()?;
    let request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "plan": plan,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["velnor-plan"],
        "required_jobs": [{"job_id": "velnor-plan", "conclusion": "success"}],
    });
    assert_eq!(merge(&request)?.status, FinalStatus::PlanningFailed);
    Ok(())
}
