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

#[test]
fn plan_event_must_match_merge_time_actual_event() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let reports_value = serde_json::to_value(&reports)?;
    // Self-consistent forgery: push event plus trusted scope passes the
    // old plan-only check but must fail against the PR actual event.
    let mut forged = plan.clone();
    forged.event = velnor_actions_contract::WorkflowEvent::Push;
    forged.trust = velnor_actions_contract::Trust::Trusted;
    forged.validate()?;
    let mut request = merge_request(&forged, &matrix, &reports_value, &success_jobs());
    request["actual_event"] = serde_json::json!("pull_request");
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "event forgery needs a scope token: {:?}",
        final_report.miss_reasons
    );
    // A missing actual event fails closed the same way.
    let mut request = merge_request(&plan, &matrix, &reports_value, &success_jobs());
    request
        .as_object_mut()
        .ok_or("request object")?
        .remove("actual_event");
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "missing actual event needs a scope token: {:?}",
        final_report.miss_reasons
    );
    // Control: matching actual event passes.
    let request = merge_request(&plan, &matrix, &reports_value, &success_jobs());
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    Ok(())
}

#[test]
fn candidate_attestation_must_bind_the_plan_head() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let reports_value = serde_json::to_value(&reports)?;
    let jobs = serde_json::json!([
        {"job_id": "candidate", "conclusion": "success"},
        {"job_id": "plan", "conclusion": "success"},
    ]);
    let base = || {
        let mut request = merge_request(&plan, &matrix, &reports_value, &jobs);
        request["required_job_ids"] = serde_json::json!(["candidate", "plan"]);
        request
    };
    // Matching attestation passes.
    let mut request = base();
    request["candidate_attestation"] = serde_json::json!({"schema": 1, "commit": plan.head});
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    // Stale or cross-plan attestation fails with a scope token.
    let mut request = base();
    request["candidate_attestation"] = serde_json::json!({"schema": 1, "commit": "0".repeat(40)});
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"trust_scope_mismatch".to_owned()),
        "stale attestation needs a scope token: {:?}",
        final_report.miss_reasons
    );
    // Missing attestation in candidate mode fails as missing evidence.
    let request = base();
    let final_report = merge(&request)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    assert!(
        final_report
            .miss_reasons
            .contains(&"source_missing".to_owned()),
        "missing attestation needs a source token: {:?}",
        final_report.miss_reasons
    );
    // Outside candidate mode a stray attestation is ignored, not judged.
    let mut request = merge_request(&plan, &matrix, &reports_value, &success_jobs());
    request["candidate_attestation"] = serde_json::json!({"schema": 1, "commit": "0".repeat(40)});
    assert_eq!(merge(&request)?.status, FinalStatus::Passed);
    Ok(())
}
