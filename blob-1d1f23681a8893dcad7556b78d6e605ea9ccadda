//! Contract final-report cases.
use crate::impl_contract_ids::sample_entry;
use velnor_actions_contract::{
    ContractError, FinalCounts, FinalReport, FinalStatus, JobConclusion, RequiredJobResult,
    artifact_id_for_matrix, artifact_id_for_plan, final_report_id_for_run, plan_id_for_run,
    run_key_for_ci, validate_final_report_id,
};

#[test]
fn final_reports_validate() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(5, 3);
    let entry = sample_entry(&run_key)?;
    let report_id = final_report_id_for_run(&run_key)?;
    validate_final_report_id(&report_id)?;
    let final_report = FinalReport {
        schema: 1,
        report_id,
        run_key: run_key.clone(),
        plan_id: plan_id_for_run(&run_key)?,
        expected_report_ids: vec![entry.report_id.clone()],
        downloaded_artifact_ids: vec![
            artifact_id_for_matrix(&run_key, &entry.matrix_key)?,
            artifact_id_for_plan(&run_key)?,
        ],
        required_job_results: vec![RequiredJobResult {
            job_id: "plan".to_owned(),
            conclusion: JobConclusion::Success,
        }],
        status: FinalStatus::Passed,
        counts: FinalCounts {
            selected: 1,
            reused: 0,
            executed: 1,
            empty_partition: 0,
            covered: 0,
            failed: 0,
            cancelled: 0,
            blocked: 0,
            not_run: 0,
        },
        miss_reasons: vec![],
    };
    final_report.validate()?;
    assert_eq!(
        final_report.artifact_id()?,
        format!("velnor-final-{run_key}")
    );
    Ok(())
}

#[test]
fn final_without_plan_is_planning_failed() -> Result<(), ContractError> {
    let run_key = run_key_for_ci(9, 3);
    let jobs = vec![
        RequiredJobResult {
            job_id: "plan".to_owned(),
            conclusion: JobConclusion::Failure,
        },
        RequiredJobResult {
            job_id: "alint".to_owned(),
            conclusion: JobConclusion::Success,
        },
    ];
    let report = FinalReport::without_plan(&run_key, jobs)?;
    report.validate()?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert_eq!(report.report_id, format!("final-{run_key}"));
    assert_eq!(report.plan_id, format!("plan-{run_key}"));
    assert_eq!(report.expected_report_ids, [] as [std::string::String; 0]);
    assert_eq!(
        report.downloaded_artifact_ids,
        [] as [std::string::String; 0]
    );
    assert_eq!(report.required_job_results[0].job_id, "alint");
    assert_eq!(report.counts.selected, 0);
    assert!(FinalReport::without_plan("bogus", Vec::new()).is_err());
    Ok(())
}
