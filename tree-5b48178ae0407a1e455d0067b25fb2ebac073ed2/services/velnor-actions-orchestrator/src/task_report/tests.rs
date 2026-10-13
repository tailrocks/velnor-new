use super::*;
use std::fs;
use velnor_actions_contract::task_report_id_for_task;
use velnor_actions_contract_config::config::{CheckPlatform, HostContainerProfile, QualifiedTool};
use velnor_actions_contract_workflow::FinalReport;
use velnor_actions_contract_workflow::MatrixEntry;
use velnor_actions_contract_workflow::{CacheLayer, CacheOutcome, CacheResult};
use velnor_actions_contract_workflow::{ExecuteTaskRef, TaskReport, TaskStatus};
use velnor_actions_contract_workflow::{FinalStatus, Plan};
use velnor_actions_orchestrator_check_acquisition::tools::QualifiedToolReceipt;
use velnor_actions_orchestrator_check_preparation::container_receipts::ContainerReceipt;
use velnor_actions_orchestrator_merge_request::assemble_with_needs;
use velnor_actions_orchestrator_task_report::task_report::{
    derive_downstream, single_task_aggregate,
};
use velnor_actions_orchestrator_task_report_write::write_task_report_to;
fn artifact(temp: &tempfile::TempDir, plan: &Plan, name: &str) -> std::path::PathBuf {
    let entry = &plan.matrix.include[0];
    temp.path()
        .join("velnor/local/reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key)
        .join(name)
}
fn assembled(temp: &tempfile::TempDir) -> String {
    assemble_with_needs(
        "local",
        &temp.path().join("velnor/local"),
        Some(r#"{"plan":"success","check-demo":"success"}"#),
        Some(r#"["check-demo","plan"]"#),
        Some("pull_request"),
        Some(r#"{"pull_request":{"head":{"repo":{"fork":false}}}}"#),
    )
    .expect("assemble")
}
fn staged_with_tools(
    with_proof: bool,
    declarations: &[QualifiedTool],
    qualified_tools: &[QualifiedToolReceipt],
) -> (tempfile::TempDir, Plan) {
    staged_with_envelope(with_proof, declarations, qualified_tools, None, None)
}
fn verdict(request: &str) -> FinalReport {
    serde_json::from_str(
        &velnor_actions_orchestrator_internal::merge_entry::merge_internal(request).expect("merge"),
    )
    .expect("final report")
}
fn plan() -> Plan {
    plan_with_tools(&[])
}
fn staged_with_container(
    with_proof: bool,
    declarations: &[QualifiedTool],
    qualified_tools: &[QualifiedToolReceipt],
    profile: &HostContainerProfile,
    container: &ContainerReceipt,
) -> (tempfile::TempDir, Plan) {
    staged_with_envelope(
        with_proof,
        declarations,
        qualified_tools,
        Some(profile),
        Some(container),
    )
}

mod check_gate_tests;
mod receipt_budget_tests;
mod support;
mod task_report_cover_tests;
mod task_report_merge_tests;
mod task_report_order_tests;
mod task_report_tests;

use support::*;
