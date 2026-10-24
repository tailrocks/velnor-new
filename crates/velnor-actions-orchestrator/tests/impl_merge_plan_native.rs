use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_merge::{merge_request, success_jobs};
use velnor_actions_contract::{
    FinalReport, FinalStatus, MatrixEntry, MatrixReport, MatrixStatus, Plan, TaskStatus,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

#[test]
fn detached_context_rejects_synthetic_native_stack_plan() -> TestResult {
    let (_repo, ordinary_plan) = plan_for_source_change()?;
    let native_plan = plan_with_stack(&ordinary_plan, "native")?;

    // Failed task evidence avoids requiring native execution proofs here. The
    // in-crate context-policy test directly checks the actual native predicate.
    assert_eq!(
        merge_with_failed_reports(&ordinary_plan)?.status,
        FinalStatus::Failed,
        "the nonnative control reaches the ordinary failed-evidence fold"
    );
    assert_eq!(
        merge_with_failed_reports(&native_plan)?.status,
        FinalStatus::PlanningFailed,
        "the synthetic native stack entry requires a repository workspace"
    );
    Ok(())
}

fn plan_with_stack(plan: &Plan, stack_id: &str) -> Result<Plan, Box<dyn std::error::Error>> {
    let mut plan = plan.clone();
    let entry = plan
        .matrix
        .include
        .first()
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture plan has no matrix entry"))?;
    plan.matrix.include[0] = MatrixEntry::derive(
        stack_id,
        &entry.task_id,
        &entry.run,
        &entry.task_digest,
        entry.adapter_metadata,
        entry.execute_task_ids,
        &entry.input_digest,
        &plan.run_key,
        &entry.job_id,
        entry.planned_platform,
    )?;
    plan.validate()?;
    Ok(plan)
}

fn merge_with_failed_reports(plan: &Plan) -> Result<FinalReport, Box<dyn std::error::Error>> {
    let reports = serde_json::to_value(failed_reports(plan)?)?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let request = merge_request(plan, &matrix, &reports, &success_jobs());
    let merged = merge_internal_with_context(&request.to_string(), &MergeContext::detached())?;
    Ok(serde_json::from_str(&merged)?)
}

fn failed_reports(plan: &Plan) -> Result<Vec<MatrixReport>, Box<dyn std::error::Error>> {
    let mut reports = passing_reports(plan)?;
    for report in &mut reports {
        for task in &mut report.tasks {
            task.status = TaskStatus::Failed;
            task.exit_code = 1;
        }
        let failed = u32::try_from(report.tasks.len())?;
        report.status = MatrixStatus::Failed;
        report.selected = failed;
        report.reused = 0;
        report.executed = 0;
        report.empty_partition = 0;
        report.not_selected = 0;
        report.failed = failed;
        report.cancelled = 0;
        report.validate()?;
    }
    Ok(reports)
}
