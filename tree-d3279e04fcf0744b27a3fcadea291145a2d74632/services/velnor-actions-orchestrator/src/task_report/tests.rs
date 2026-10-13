use super::*;
use super::{single_task_aggregate, write_task_report_to};
use std::fs;
use velnor_actions_contract::FinalReport;
use velnor_actions_contract::MatrixEntry;
use velnor_actions_contract::config::{CheckPlatform, HostContainerProfile, QualifiedTool};
use velnor_actions_contract::{ExecuteTaskRef, TaskReport, TaskStatus};
use velnor_actions_contract::{FinalStatus, Plan};
fn artifact(temp: &tempfile::TempDir, plan: &Plan, name: &str) -> std::path::PathBuf {
    let entry = &plan.matrix.include[0];
    temp.path()
        .join("velnor/local/reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key)
        .join(name)
}
fn assembled(temp: &tempfile::TempDir) -> String {
    crate::merge_request::assemble_with_needs(
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
    qualified_tools: &[crate::check_evidence::gate::tools::QualifiedToolReceipt],
) -> (tempfile::TempDir, Plan) {
    staged_with_envelope(with_proof, declarations, qualified_tools, None, None)
}
fn verdict(request: &str) -> FinalReport {
    serde_json::from_str(&crate::merge_internal(request).expect("merge")).expect("final report")
}
fn plan() -> Plan {
    plan_with_tools(&[])
}
fn staged_with_container(
    with_proof: bool,
    declarations: &[QualifiedTool],
    qualified_tools: &[crate::check_evidence::gate::tools::QualifiedToolReceipt],
    profile: &HostContainerProfile,
    container: &crate::check_evidence::gate::container::ContainerReceipt,
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
mod load_plan_strict_tests;
mod receipt_budget_tests;
mod support;
mod task_report_cover_tests;
mod task_report_merge_tests;
mod task_report_order_tests;
mod task_report_tests;

use support::*;
