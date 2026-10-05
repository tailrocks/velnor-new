//! Internal plan and merge integration cases.

use super::*;

#[test]
fn internal_plan_selects_affected_and_validates() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    assert!(!plan.task_ids.is_empty(), "affected obligations selected");
    Ok(())
}

#[test]
fn internal_merge_aggregates_reports() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let merge_request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": [],
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
    });
    let merged = merge_internal(&merge_request.to_string())?;
    let final_report: FinalReport = serde_json::from_str(&merged)?;
    final_report.validate()?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract::FinalStatus::NotRun
    );

    let reports = passing_reports(&plan)?;
    let plan_value = serde_json::to_value(&plan)?;
    let task_files = task_reports_for(&plan_value, &serde_json::to_value(&reports)?);
    let merge_request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": reports,
        "task_reports": task_files,
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
    });
    let merged = merge_internal(&merge_request.to_string())?;
    let final_report: FinalReport = serde_json::from_str(&merged)?;
    assert_eq!(
        final_report.status,
        velnor_actions_contract::FinalStatus::Passed
    );
    Ok(())
}

#[test]
fn internal_entrypoints_reject_bad_schema() -> TestResult {
    let err = err_of(plan_internal(r#"{"schema":2}"#), "plan schema rejected")?;
    assert!(
        matches!(err, OrchestratorError::Internal { .. }),
        "got {err}"
    );
    let err = err_of(merge_internal(r#"{"schema":2}"#), "merge schema rejected")?;
    assert!(
        matches!(err, OrchestratorError::Internal { .. }),
        "got {err}"
    );
    let err = err_of(plan_internal("not json"), "malformed rejected")?;
    assert!(
        matches!(err, OrchestratorError::Internal { .. }),
        "got {err}"
    );
    Ok(())
}

#[test]
fn plan_job_lines_come_from_finalized_jobs() -> TestResult {
    let repo = make_repo(config_with_branch())?;
    let prep = prepare(repo.path())?;
    let jobs = finalized_jobs(&prep)?;
    let text = plan_text(&prep, &jobs);
    assert!(!jobs.is_empty(), "finalized jobs exist");
    for (id, job) in &jobs {
        let line = format!("- {id} ({} steps)", job.steps.len());
        assert!(text.contains(&line), "missing {line}:\n{text}");
    }
    assert!(text.contains("1 Rust crate job"), "crate detail:\n{text}");
    Ok(())
}
