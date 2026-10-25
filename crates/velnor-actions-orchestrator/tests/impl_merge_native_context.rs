//! Contract-valid native merge plans require an explicit runner workspace.

use std::collections::BTreeMap;

use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, FinalStatus, JobDisposition, MatrixEntry, MatrixReport,
    MatrixStatus, ObligationDecision, Plan, PlanJob, PlanObligation, PlannedPlatform, TaskStatus,
    digest_b3,
};
use velnor_actions_orchestrator::{MergeContext, merge_internal_with_context};

use crate::impl_common::{TestResult, plan_for_source_change};
use crate::impl_merge::merge_request;

#[test]
fn detached_context_rejects_contract_valid_native_plan() -> TestResult {
    let (_repository, mut plan) = plan_for_source_change()?;
    replace_with_contract_valid_native_check(&mut plan)?;
    plan.validate()?;
    let entry = &plan.matrix.include[0];
    let task_report_id = velnor_actions_contract::task_report_id_for_task(
        &plan.run_key,
        &entry.matrix_key,
        &entry.task_digest,
    )?;
    let reports = [MatrixReport {
        schema: 1,
        report_id: entry.report_id.clone(),
        run_key: plan.run_key.clone(),
        matrix_id: entry.id.clone(),
        matrix_key: entry.matrix_key.clone(),
        status: MatrixStatus::Failed,
        expected_task_ids: vec![entry.task_id.clone()],
        task_report_ids: vec![task_report_id.clone()],
        tasks: vec![velnor_actions_contract::MatrixTaskEntry {
            task_report_id,
            task_id: entry.task_id.clone(),
            status: TaskStatus::Failed,
            exit_code: 1,
        }],
        selected: 1,
        reused: 0,
        executed: 0,
        empty_partition: 0,
        not_selected: 0,
        failed: 1,
        cancelled: 0,
    }];
    reports[0].validate()?;
    let matrix = serde_json::to_value(&plan.matrix)?;
    let reports = serde_json::to_value(reports)?;
    let mut jobs = vec![serde_json::json!({
        "job_id": "plan",
        "conclusion": "success",
    })];
    jobs.extend(
        plan.jobs
            .iter()
            .map(|job| serde_json::json!({"job_id": job.job_id.as_str(), "conclusion": "success"})),
    );
    let request = merge_request(&plan, &matrix, &reports, &serde_json::Value::Array(jobs));
    let merged = merge_internal_with_context(&request.to_string(), &MergeContext::detached())?;
    let report: velnor_actions_contract::FinalReport = serde_json::from_str(&merged)?;
    assert_eq!(report.status, FinalStatus::PlanningFailed);
    assert!(
        report
            .miss_reasons
            .iter()
            .any(|reason| reason == "cache_corrupt"),
        "detached native workspace admission fails closed: {:?}",
        report.miss_reasons
    );
    assert!(
        !report
            .miss_reasons
            .iter()
            .any(|reason| reason == "no_entry"),
        "matrix entries all have matching failed command reports: {:?}",
        report.miss_reasons
    );
    Ok(())
}

fn replace_with_contract_valid_native_check(plan: &mut Plan) -> TestResult {
    const TASK_ID: &str = "internal/native-check/tap-tree";
    const JOB_ID: &str = "check-tap-tree";
    let task_digest = digest_b3(b"native-tap-tree-task");
    let input_digest = digest_b3(b"native-tap-tree-input");
    let closure_digest = digest_b3(b"native-tap-tree-closure");
    let task_binding_digest = velnor_actions_contract::TaskBindingDigest::new(task_digest.clone())?;
    let mbx_generation = velnor_actions_contract::cachekey::mbx_cache_generation(
        velnor_actions_mise::ToolCatalog::pinned()
            .version(velnor_actions_mise::PinnedTool::MrBoxington),
    );
    let mbx_digest = digest_b3(mbx_generation.as_bytes());
    let entry = MatrixEntry::derive(
        "native",
        TASK_ID,
        "native-homebrew-check-v1",
        &task_digest,
        serde_json::json!({
            "check_id": "tap-tree",
            "operation": "tap_tree",
            "runner": {
                "label": "macos-15",
                "platform": "macos_arm64",
                "executor": "hosted"
            },
            "identity": {}
        }),
        ExecuteTaskIds {
            tasks: BTreeMap::from([(
                "native_check".to_owned(),
                ExecuteTaskRef::Single(TASK_ID.to_owned()),
            )]),
        },
        &input_digest,
        &plan.run_key,
        JOB_ID,
        PlannedPlatform::new("macos-15", "aarch64-apple-darwin")?,
    )?;
    plan.obligations = vec![PlanObligation {
        task_id: TASK_ID.to_owned(),
        decision: ObligationDecision::Execute,
        reason: "native_homebrew_check_requires_execution".to_owned(),
        task_digest,
        task_binding_digest,
        input_digest,
        closure_digest,
        graph_digest: digest_b3(b"no-workspace"),
        mbx_digest,
        profile: velnor_actions_contract::config::NativeHomebrewOperation::TapTree
            .as_str()
            .to_owned(),
        job_id: JOB_ID.to_owned(),
    }];
    plan.packages.clear();
    plan.matrix.include = vec![entry];
    plan.task_ids = vec![TASK_ID.to_owned()];
    plan.jobs = vec![PlanJob {
        job_id: JOB_ID.to_owned(),
        disposition: JobDisposition::Run,
    }];
    plan.edges.clear();
    Ok(())
}
