use super::{assembled, plan, staged};
use crate::{MergeContext, merge_internal_with_context};
use std::path::PathBuf;
use velnor_actions_contract::{FinalReport, FinalStatus, MatrixEntry};

fn merge_with_context(request: &str, context: &MergeContext) -> FinalReport {
    serde_json::from_str(
        &merge_internal_with_context(request, context).expect("merge with explicit context"),
    )
    .expect("final report")
}

#[test]
fn repository_context_without_workspace_fails_closed() {
    let (temp, _) = staged(true);
    let report = merge_with_context(&assembled(&temp), &MergeContext::required_workspace(None));

    assert_eq!(report.status, FinalStatus::PlanningFailed);
}

#[test]
fn repository_context_rejects_fixture_head_at_checkout_root() {
    let (temp, _) = staged(true);
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("checkout root");
    let report = merge_with_context(
        &assembled(&temp),
        &MergeContext::required_workspace(Some(workspace)),
    );

    assert_eq!(report.status, FinalStatus::PlanningFailed);
}

#[test]
fn native_plan_requires_workspace_even_with_detached_context() {
    let mut native_plan = plan();
    let entry = native_plan.matrix.include[0].clone();
    native_plan.matrix.include[0] = MatrixEntry::derive(
        "native",
        &entry.task_id,
        &entry.run,
        &entry.task_digest,
        entry.adapter_metadata,
        entry.execute_task_ids,
        &entry.input_digest,
        &native_plan.run_key,
        &entry.job_id,
        entry.planned_platform,
    )
    .expect("native entry identity");
    native_plan.validate().expect("native fixture plan");

    assert!(crate::merge::workspace_validation_required(
        &native_plan,
        &MergeContext::detached()
    ));
    assert!(!crate::merge::workspace_validation_required(
        &plan(),
        &MergeContext::detached()
    ));
}
