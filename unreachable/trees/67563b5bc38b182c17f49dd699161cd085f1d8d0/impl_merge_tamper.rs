//! Merge tamper-rejection cases.
use velnor_actions_contract::{FinalStatus, MatrixReport};

use super::impl_merge::{merge, merge_request, success_jobs};
use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};

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
