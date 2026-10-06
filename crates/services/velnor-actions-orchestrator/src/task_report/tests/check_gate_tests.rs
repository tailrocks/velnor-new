//! Named-check artifact proof remains mandatory through assembly and final merge.
use super::*;

use std::fs;
use velnor_actions_contract::{FinalStatus, Plan, canonical_json_bytes};

fn staged(with_proof: bool) -> (tempfile::TempDir, Plan) {
    staged_with_envelope(with_proof, &[], &[], None, None)
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
