//! Full-run events never skip work: plan validation and `Required` backstops.
//!
//! `selection_mode` is the planner's only guarantee that push, merge
//! group, and qualification runs execute everything. Both `Plan::validate`
//! and the `merge-v1` fold re-check it independently, so a planner bug
//! cannot let a skipped crate job pass `Required` on a full-run event.

use velnor_actions_contract::{FinalStatus, WorkflowEvent};

use crate::impl_common::TestResult;
use crate::impl_merge::{merge, merge_request};
use crate::impl_merge_plan::empty_diff_plan;

#[test]
fn plan_validation_rejects_not_affected_work_on_full_events() -> TestResult {
    let (_repo, plan, skipped) = empty_diff_plan()?;
    assert!(!skipped.is_empty(), "{:?}", plan.jobs);
    assert!(plan.validate().is_ok(), "pull request plans may skip work");
    // A forged qualification plan already fails its dispatch-context check.
    for event in [WorkflowEvent::Push, WorkflowEvent::MergeGroup] {
        let mut forged = plan.clone();
        forged.event = event;
        let error = forged.validate().expect_err("full events never skip");
        assert!(
            error.to_string().contains("not_affected_on_full_event"),
            "{event:?}: {error}"
        );
    }
    Ok(())
}

#[test]
fn required_rejects_not_affected_work_on_a_full_actual_event() -> TestResult {
    let (_repo, plan, skipped) = empty_diff_plan()?;
    let mut jobs = vec![serde_json::json!({"job_id": "plan", "conclusion": "success"})];
    jobs.extend(
        skipped
            .iter()
            .map(|id| serde_json::json!({"job_id": id, "conclusion": "skipped"})),
    );
    let jobs = serde_json::Value::Array(jobs);
    let matrix = serde_json::to_value(&plan.matrix)?;
    let no_reports = serde_json::json!([]);
    let passing = merge(&merge_request(&plan, &matrix, &no_reports, &jobs))?;
    assert_eq!(passing.status, FinalStatus::Passed, "{:?}", passing.miss_reasons);
    for event in ["push", "merge_group", "qualification"] {
        let mut request = merge_request(&plan, &matrix, &no_reports, &jobs);
        request["actual_event"] = serde_json::json!(event);
        let report = merge(&request)?;
        assert_eq!(report.status, FinalStatus::PlanningFailed, "{event}");
        assert!(
            report
                .miss_reasons
                .contains(&"not_affected_on_full_event".to_owned()),
            "{event}: {:?}",
            report.miss_reasons
        );
    }
    Ok(())
}
