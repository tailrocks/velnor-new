//! Never-bare diagnostics: limits/reference/shard failures carry miss tokens.

use velnor_actions_contract_workflow::{FinalStatus, MatrixReport, Plan};

use crate::cases::gates_shard::sharded_case;
use crate::support::{TestResult, passing_reports, plan_for_source_change};

/// Full final report for one merge request (status plus miss tokens).
pub(crate) fn merge_report_with(
    plan: &Plan,
    reports: &[MatrixReport],
    proofs: &[serde_json::Value],
    extra: &serde_json::Value,
) -> Result<velnor_actions_contract_workflow::FinalReport, Box<dyn std::error::Error>> {
    let plan_value = serde_json::to_value(plan).unwrap_or(serde_json::Value::Null);
    let reports_value = serde_json::to_value(reports).unwrap_or(serde_json::Value::Null);
    let task_files = crate::cases::merge::task_reports_for(&plan_value, &reports_value);
    let mut request = serde_json::json!({
        "schema": 1,
        "run_key": "local",
        "actual_event": plan_value.get("event").cloned().unwrap_or(serde_json::Value::Null),
        "plan": plan,
        "matrix": plan.matrix,
        "matrix_reports": reports,
        "task_reports": task_files,
        "required_job_ids": ["plan"],
        "required_jobs": [{"job_id": "plan", "conclusion": "success"}],
        "shard_proofs": proofs,
    });
    for (key, value) in extra.as_object().ok_or("not an object")? {
        request[key] = value.clone();
    }
    Ok(serde_json::from_str(
        &velnor_actions_orchestrator::merge_internal(&request.to_string())?,
    )?)
}

#[test]
fn limits_reference_and_shard_failures_carry_miss_tokens() -> TestResult {
    let (plan, _inventory, reports, proofs) = sharded_case()?;
    let bad_limits = serde_json::json!({"limits": {"compiler_budget": 4, "test_budget": 1, "max_parallel": 4, "capacity": 8, "shards": 2, "retries": 0}});
    let bad_reference =
        serde_json::json!({"reference_task_ids": ["stack/rust/root/clippy/default"]});
    for (extra, case_proofs) in [
        (&bad_limits, proofs.as_slice()),
        (&bad_reference, proofs.as_slice()),
        (&serde_json::json!({}), &[] as &[serde_json::Value]),
    ] {
        let report = merge_report_with(&plan, &reports, case_proofs, extra)?;
        assert_eq!(report.status, FinalStatus::PlanningFailed, "{extra}");
        assert!(
            !report.miss_reasons.is_empty(),
            "never-bare diagnostic required: {extra}"
        );
    }
    Ok(())
}

/// Binding failures carry miss tokens: a report whose task set
/// contradicts its entry fails with `cache_corrupt`, never bare.
#[test]
fn binding_failures_carry_miss_tokens() -> TestResult {
    let (_repo, plan) = plan_for_source_change()?;
    let mut reports = passing_reports(&plan)?;
    assert!(!reports.is_empty(), "fixture needs a report");
    // Internally coherent (shapes, counts, id multiset) but bound to a
    // task the entry never scheduled: partition accepts it, the
    // per-entry binding check rejects it.
    reports[0].expected_task_ids = vec!["stack/rust/root/fmt/default".to_owned()];
    let report = merge_report_with(&plan, &reports, &[], &serde_json::json!({}))?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report.miss_reasons.iter().any(|r| r == "cache_corrupt"),
        "{:?}",
        report.miss_reasons
    );
    Ok(())
}
