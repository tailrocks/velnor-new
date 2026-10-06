//! Named-check artifact proof remains mandatory through assembly and final merge.
use super::task_report_tests::fixture_plan;
use super::{
    single_task_aggregate, terminal_task_report, write_entry_reports, write_task_report_to,
};
use crate::check_evidence::{gate::execution_receipt, verify_evidence};
use std::collections::BTreeMap;
use std::fs;
use velnor_actions_contract::config::{
    CheckEvidence, CheckPlatform, HostContainerProfile, QualifiedTool,
};
use velnor_actions_contract::{
    ExecuteTaskIds, ExecuteTaskRef, FinalReport, FinalStatus, MatrixEntry, Plan, PlannedPlatform,
    canonical_json_bytes,
};

const TASK: &str = "stack/mise/demo/check/default";
fn plan_with_tools(qualified_tools: &[QualifiedTool]) -> Plan {
    let mut plan = fixture_plan();
    let mut obligation = plan.obligations.remove(0);
    obligation.task_id = TASK.into();
    let mut tool_specs: Vec<_> = qualified_tools
        .iter()
        .filter_map(|tool| match &tool.backend {
            velnor_actions_contract::config::QualifiedToolBackend::Aqua { package } => {
                Some(format!("aqua:{package}@{}", tool.version))
            }
            _ => None,
        })
        .collect();
    tool_specs.sort();
    let qualification_digest =
        velnor_actions_mise::checks::DiscoveredCheck::qualification_fingerprint(
            qualified_tools,
            &tool_specs,
        )
        .expect("fingerprint");
    let mut entry = MatrixEntry::derive("mise", TASK, "true", &obligation.task_digest,
        serde_json::json!({"check_id":"demo","system_tools":[],"qualified_tools":qualified_tools,"tool_specs":tool_specs,"qualification_digest":qualification_digest,"evidence":{"path":"proof.json","expected_scenarios":["one"]},
            "runner":{"label":"ubuntu-24.04","platform":"linux_x64","executor":"hosted"}}),
        ExecuteTaskIds { tasks: BTreeMap::from([("check".into(), ExecuteTaskRef::Single(TASK.into()))]) },
        &obligation.input_digest, "local", "check-demo",
        PlannedPlatform::new("ubuntu-24.04", "x86_64-unknown-linux-gnu").expect("planned platform")).expect("entry");
    entry.declared_outputs = vec!["proof.json".into()];
    plan.matrix.include = vec![entry];
    plan.obligations = vec![obligation];
    plan.task_ids = vec![TASK.into()];
    plan.packages.clear();
    plan.edges.clear();
    plan.validate().expect("plan");
    plan
}
fn plan() -> Plan {
    plan_with_tools(&[])
}
fn producer_json(plan: &Plan) -> String {
    serde_json::json!({"schema":1,"source":"mise-task-v1","head":plan.head,"check_id":"demo","platform":"linux_x64",
        "scenarios":[{"id":"one","executed":true,"status":"passed"}]}).to_string()
}
fn staged(with_proof: bool) -> (tempfile::TempDir, Plan) {
    staged_with_envelope(with_proof, &[], &[], None, None)
}
fn staged_with_tools(
    with_proof: bool,
    declarations: &[QualifiedTool],
    qualified_tools: &[crate::check_evidence::gate::tools::QualifiedToolReceipt],
) -> (tempfile::TempDir, Plan) {
    staged_with_envelope(with_proof, declarations, qualified_tools, None, None)
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

fn staged_with_envelope(
    with_proof: bool,
    declarations: &[QualifiedTool],
    qualified_tools: &[crate::check_evidence::gate::tools::QualifiedToolReceipt],
    container_profile: Option<&HostContainerProfile>,
    container: Option<&crate::check_evidence::gate::container::ContainerReceipt>,
) -> (tempfile::TempDir, Plan) {
    let temp = tempfile::TempDir::new().expect("temp");
    let mut plan = plan_with_tools(declarations);
    if let Some(profile) = container_profile {
        plan.matrix.include[0].adapter_metadata["runner"] = serde_json::json!({
            "label":"native-scale",
            "platform":"linux_x64",
            "executor":"ephemeral_self_hosted",
            "container":profile,
        });
        plan.validate().expect("container runner metadata");
    }
    let entry = &plan.matrix.include[0];
    fs::write(temp.path().join("proof.json"), producer_json(&plan)).expect("producer evidence");
    let receipt = verify_evidence(
        temp.path(),
        &CheckEvidence {
            path: "proof.json".into(),
            expected_scenarios: vec!["one".into()],
        },
        "demo",
        &plan.head,
        CheckPlatform::LinuxX64,
    )
    .expect("verified evidence");
    let run = temp.path().join("velnor/local");
    fs::create_dir_all(&run).expect("run");
    fs::write(
        run.join("plan.json"),
        canonical_json_bytes(&plan).expect("plan bytes"),
    )
    .expect("plan");
    fs::write(
        run.join("matrix.json"),
        canonical_json_bytes(&plan.matrix).expect("matrix bytes"),
    )
    .expect("matrix");
    let mut task =
        terminal_task_report(&plan, entry, &entry.task_digest, 0, Some(1)).expect("task");
    task.outputs = vec!["proof.json".into()];
    let matrix = single_task_aggregate(&plan, entry, &task).expect("matrix");
    write_entry_reports(temp.path(), &plan, entry, &task, &matrix).expect("reports");
    let home = run.join(&entry.matrix_key);
    if with_proof {
        fs::create_dir(home.join("evidence")).expect("evidence dir");
        fs::write(home.join("evidence/proof.json"), &receipt.bytes).expect("evidence artifact");
        let mut execution = execution_receipt(
            &plan,
            entry,
            "demo",
            CheckPlatform::LinuxX64,
            Some(receipt),
            vec![],
            qualified_tools.to_vec(),
        );
        execution.container = container.cloned();
        fs::write(
            home.join("check-execution.json"),
            canonical_json_bytes(&execution).expect("receipt bytes"),
        )
        .expect("receipt");
    }
    let downloads = run.join("reports").join(&entry.artifact_id);
    fs::create_dir_all(&downloads).expect("downloads");
    fs::rename(home, downloads.join(&entry.matrix_key)).expect("stage artifact");
    (temp, plan)
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
fn verdict(request: &str) -> FinalReport {
    serde_json::from_str(&crate::merge_internal(request).expect("merge")).expect("final report")
}
fn artifact(temp: &tempfile::TempDir, plan: &Plan, name: &str) -> std::path::PathBuf {
    let entry = &plan.matrix.include[0];
    temp.path()
        .join("velnor/local/reports")
        .join(&entry.artifact_id)
        .join(&entry.matrix_key)
        .join(name)
}
#[test]
fn genuine_receipt_and_scenario_bytes_pass_assembled_final_gate() {
    let (temp, _) = staged(true);
    assert_eq!(verdict(&assembled(&temp)).status, FinalStatus::Passed);
}
#[test]
fn missing_qualified_observation_transport_fails_final_gate() {
    let (temp, plan) = staged(true);
    let path = artifact(&temp, &plan, "check-execution.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("receipt")).expect("json");
    receipt
        .as_object_mut()
        .expect("object")
        .remove("qualified_tools");
    fs::write(path, receipt.to_string()).expect("omit tool observations");
    let mut request: serde_json::Value = serde_json::from_str(&assembled(&temp)).expect("request");
    request["assembly_errors"] = serde_json::json!([]);
    assert_ne!(verdict(&request.to_string()).status, FinalStatus::Passed);
}
#[test]
fn ordinary_matching_task_reports_without_receipt_cannot_pass() {
    let (temp, _) = staged(false);
    assert_ne!(verdict(&assembled(&temp)).status, FinalStatus::Passed);
    let mut request: serde_json::Value = serde_json::from_str(&assembled(&temp)).expect("json");
    request["assembly_errors"] = serde_json::json!([]);
    assert_ne!(
        verdict(&request.to_string()).status,
        FinalStatus::Passed,
        "fold independently requires proof"
    );
}
#[test]
fn corrupted_deleted_skipped_and_foreign_evidence_fail_final_gate() {
    for mode in [
        "corrupt",
        "delete",
        "skipped",
        "foreign",
        "duplicate",
        "stale",
    ] {
        let (temp, plan) = staged(true);
        let path = artifact(&temp, &plan, "evidence/proof.json");
        if mode == "delete" {
            fs::remove_file(path).expect("delete");
        } else {
            let mut bytes = producer_json(&plan);
            if mode == "corrupt" {
                bytes.push('x');
            }
            if mode == "skipped" {
                bytes = bytes.replace("passed", "skipped");
            }
            if mode == "foreign" {
                bytes = bytes.replace("one", "foreign");
            }
            if mode == "duplicate" {
                bytes = bytes.replace(
                    '[',
                    "[{\"id\":\"one\",\"executed\":true,\"status\":\"passed\"},",
                );
            }
            if mode == "stale" {
                bytes = bytes.replace(&plan.head, "old");
            }
            fs::write(path, bytes).expect("mutated evidence");
        }
        assert_ne!(
            verdict(&assembled(&temp)).status,
            FinalStatus::Passed,
            "{mode}"
        );
    }
}
#[test]
fn forged_receipt_hash_does_not_bypass_scenario_validation() {
    let (temp, plan) = staged(true);
    let bytes = producer_json(&plan).replace("passed", "skipped");
    fs::write(artifact(&temp, &plan, "evidence/proof.json"), &bytes).expect("evidence");
    let receipt_path = artifact(&temp, &plan, "check-execution.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&receipt_path).expect("receipt")).expect("json");
    receipt["evidence"]["digest"] =
        serde_json::json!(velnor_actions_contract::digest_b3(bytes.as_bytes()));
    fs::write(receipt_path, receipt.to_string()).expect("forged hash");
    assert_ne!(verdict(&assembled(&temp)).status, FinalStatus::Passed);
}
#[test]
fn generic_report_producer_refuses_named_check_success() {
    let plan = plan();
    let temp = tempfile::TempDir::new().expect("temp");
    let run = temp.path().join("velnor/local");
    fs::create_dir_all(&run).expect("run");
    fs::write(
        run.join("plan.json"),
        canonical_json_bytes(&plan).expect("json"),
    )
    .expect("plan");
    assert!(write_task_report_to("local", TASK, 0, None, &[], temp.path()).is_err());
}

#[test]
fn native_pin_without_matching_runtime_proof_fails_final_gate() {
    let (temp, _) = staged(true);
    let mut request: serde_json::Value = serde_json::from_str(&assembled(&temp)).expect("json");
    let pins = serde_json::json!([{ "kind":"swift", "version":"6.4", "build":"swiftlang-6.4.0.34.1 clang-2100.3.34.1" }]);
    request["plan"]["matrix"]["include"][0]["adapter_metadata"]["system_tools"] = pins.clone();
    request["matrix"]["include"][0]["adapter_metadata"]["system_tools"] = pins;
    assert_ne!(verdict(&request.to_string()).status, FinalStatus::Passed);
}

#[test]
fn omitted_native_proof_field_is_not_an_empty_verified_set() {
    let (temp, plan) = staged(true);
    let path = artifact(&temp, &plan, "check-execution.json");
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).expect("receipt")).expect("json");
    receipt
        .as_object_mut()
        .expect("object")
        .remove("system_tools");
    fs::write(path, receipt.to_string()).expect("receipt");
    assert_ne!(verdict(&assembled(&temp)).status, FinalStatus::Passed);
}

#[test]
fn container_declaration_without_receipt_fails_after_assembly_errors_cleared() {
    let (temp, _) = staged(true);
    let mut request: serde_json::Value = serde_json::from_str(&assembled(&temp)).expect("json");
    let runner = serde_json::json!({
        "label":"qualified-linux", "platform":"linux_x64", "executor":"ephemeral_self_hosted",
        "container": {"provider":"docker", "context":"qualified", "socket_path":"/qualified/docker.sock", "socket_uid":0,
            "cli":{"path":"/qualified/docker","sha256":"0".repeat(64),"version":"29.1.0","build":"abc123"},
            "daemon":{"version":"29.1.0","platform":"linux_x64","operating_system":"Qualified Linux","identity_policy":"execution_scoped"}}
    });
    request["plan"]["matrix"]["include"][0]["adapter_metadata"]["runner"] = runner.clone();
    request["matrix"]["include"][0]["adapter_metadata"]["runner"] = runner;
    request["assembly_errors"] = serde_json::json!([]);
    assert_ne!(verdict(&request.to_string()).status, FinalStatus::Passed);
}

#[cfg(test)]
#[path = "check_gate_receipt_budget_tests.rs"]
mod receipt_budget_tests;
