use super::*;

#[test]
fn missing_plan_on_tofu_inventory_diagnoses_planning_failed() -> TestResult {
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "plan": null,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["plan", "tofu-stacks-a"],
        "required_jobs": [
            {"job_id": "plan", "conclusion": "success"},
            {"job_id": "tofu-stacks-a", "conclusion": "success"},
        ],
    });
    let report = merge(&request)?;
    report.validate()?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.contains(&"source_missing".to_owned()),
        "{:?}",
        report.miss_reasons
    );

    // A missing matrix file with a present tofu plan is also planning_failed.
    let (_repo, plan) = plan_for_tofu_change("stacks/a/main.tf")?;
    let request = json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": "pull_request",
        "plan": plan,
        "matrix": null,
        "matrix_reports": [],
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
    });
    assert_eq!(merge(&request)?.status, FinalStatus::PlanningFailed);
    Ok(())
}
