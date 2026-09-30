//! Merge trust coherence: forged plan/report trust fails closed.

use velnor_actions_contract::FinalStatus;

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_merge::{merge, merge_request, success_jobs};

#[test]
fn trust_mismatch_fails_closed_with_scope_token() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let reports_value = serde_json::to_value(&reports)?;
    let mut forged_plan = plan.clone();
    forged_plan.trust = velnor_actions_contract::Trust::Trusted;
    let request = merge_request(&forged_plan, &matrix, &reports_value, &success_jobs());
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "plan trust forgery needs a scope token: {:?}",
        final_report.miss_reasons
    );
    let mut request = merge_request(&plan, &matrix, &reports_value, &success_jobs());
    let files = request["task_reports"].as_array_mut().ok_or("task files")?;
    files[0]["trust"] = serde_json::json!("trusted");
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "report trust forgery needs a scope token: {:?}",
        final_report.miss_reasons
    );
    Ok(())
}
