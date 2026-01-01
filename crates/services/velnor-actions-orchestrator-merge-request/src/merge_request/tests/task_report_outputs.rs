use super::super::task_report_outputs::ExpectedProducerChannel;
use super::super::{AssemblyChannels, assemble_with_task_report_channels};
use super::{error_list, staged};
use velnor_actions_contract_workflow::{ArtifactBuildRunContext, Plan, canonical_plan_digest};

const RUN_KEY: &str = "r42-a2";
const HEAD: &str = "a4dfd62241cf85172a3c34ca5ab5e1750907d053";
const NEEDS_EXPECTED: &str = r#"["plan","task-linux"]"#;
const PRODUCERS_EXPECTED: &str = r#"["task-linux"]"#;

#[test]
fn assembles_typed_fanin_from_exact_successful_needs_outputs() {
    let request = assemble(
        Some(PRODUCERS_EXPECTED),
        success_needs(),
        "https://github.com",
    );
    let plan: Plan = serde_json::from_value(plan_value()).expect("typed plan");
    let digest = canonical_plan_digest(&plan).expect("plan digest");
    let sidecar = &request["task_report_outputs"];
    assert_eq!(sidecar["origin"], "github_com");
    assert_eq!(sidecar["head_sha"], HEAD);
    assert_eq!(sidecar["plan_digest"], digest);
    assert_eq!(sidecar["run"]["run_id"], "42");
    assert_eq!(sidecar["run"]["run_attempt"], 2);
    assert_eq!(
        sidecar["expected_workflow_job_keys"],
        serde_json::json!(["task-linux"])
    );
    assert_eq!(sidecar["producers"][0]["workflow_job_key"], "task-linux");
    assert_eq!(sidecar["producers"][0]["conclusion"], "success");
    assert_eq!(sidecar["producers"][0]["artifact_id"], 123);
    assert_eq!(sidecar["producers"][0]["check_run_id"], 456);
    assert_eq!(error_list(&request.to_string()).len(), 0);
}

#[test]
fn absent_channel_preserves_legacy_request_shape() {
    let request = assemble(None, success_needs(), "https://github.com");
    assert!(request.get("task_report_outputs").is_none());
    assert_eq!(error_list(&request.to_string()).len(), 0);
}

#[test]
fn malformed_inventory_and_nonrequired_keys_fail_closed() {
    let malformed = assemble(Some("not-json"), success_needs(), "https://github.com");
    assert_error(&malformed, "invalid_task_report_producers_expected");
    let duplicate = assemble(
        Some(r#"["task-linux","task-linux"]"#),
        success_needs(),
        "https://github.com",
    );
    assert_error(&duplicate, "invalid_task_report_producers_expected");
    let foreign = assemble(
        Some(r#"["task-other"]"#),
        success_needs(),
        "https://github.com",
    );
    assert_error(&foreign, "invalid_task_report_producers_expected");
}

#[test]
fn missing_needs_job_and_output_ids_fail_closed() {
    let absent = r#"{"plan":{"result":"success"}}"#;
    let request = assemble(Some(PRODUCERS_EXPECTED), absent, "https://github.com");
    assert_error(&request, "needs_inventory_mismatch");
    assert_error(&request, "missing_task_report_producer:task-linux");

    for invalid in ["0", "-1", "01", "+1", "abc", ""] {
        let needs = needs_with_ids(invalid, "456");
        let request = assemble(Some(PRODUCERS_EXPECTED), &needs, "https://github.com");
        assert_error(&request, "invalid_task_report_output_id:task-linux");
    }
}

#[test]
fn duplicate_actual_needs_keys_fail_closed() {
    let needs = r#"{"plan":{"result":"success"},"task-linux":{"result":"success","outputs":{"task_report_artifact_id":"123","task_report_check_run_id":"456"}},"task-linux":{"result":"success","outputs":{"task_report_artifact_id":"789","task_report_check_run_id":"012"}}}"#;
    let request = assemble(Some(PRODUCERS_EXPECTED), needs, "https://github.com");
    assert_error(&request, "invalid_task_report_producer_needs");
}

#[test]
fn failed_producer_is_not_fabricated_as_success() {
    let needs = r#"{"plan":{"result":"success"},"task-linux":{"result":"failure","outputs":{}}}"#;
    let request = assemble(Some(PRODUCERS_EXPECTED), needs, "https://github.com");
    assert!(request.get("task_report_outputs").is_none());
    assert_eq!(request["required_jobs"][1]["conclusion"], "failure");
    assert_eq!(error_list(&request.to_string()).len(), 0);
}

#[test]
fn unsupported_origin_and_run_identity_are_rejected() {
    let origin = assemble(
        Some(PRODUCERS_EXPECTED),
        success_needs(),
        "https://ghe.example",
    );
    assert_error(&origin, "unsupported_task_report_output_origin");

    let context = ArtifactBuildRunContext {
        repository_id: "123456789".to_owned(),
        repository: "owner/repo".to_owned(),
        run_id: "43".to_owned(),
        run_attempt: 2,
    };
    let request = assemble_with_context(
        Some(PRODUCERS_EXPECTED),
        success_needs(),
        "https://github.com",
        &context,
    );
    assert_error(&request, "invalid_task_report_output_context");
}

fn assemble(producer_expected: Option<&str>, needs: &str, server_url: &str) -> serde_json::Value {
    let context = context();
    assemble_with_context(producer_expected, needs, server_url, &context)
}

fn assemble_with_context(
    producer_expected: Option<&str>,
    needs: &str,
    server_url: &str,
    context: &ArtifactBuildRunContext,
) -> serde_json::Value {
    let plan = plan_value();
    let dir = staged(&plan.to_string(), &[]);
    std::fs::write(dir.path().join("matrix.json"), r#"{"include":[]}"#).expect("matrix");
    let request = assemble_with_task_report_channels(
        RUN_KEY,
        dir.path(),
        Some(needs),
        Some(NEEDS_EXPECTED),
        Some("push"),
        Some("{}"),
        AssemblyChannels {
            producer_expected: producer_expected.map_or(ExpectedProducerChannel::Absent, |value| {
                ExpectedProducerChannel::Present(value.to_owned())
            }),
            runtime_identity_override: Some((context, server_url)),
        },
    )
    .expect("assemble");
    serde_json::from_str(&request).expect("request JSON")
}

fn plan_value() -> serde_json::Value {
    serde_json::json!({
        "schema": 1,
        "run_key": RUN_KEY,
        "plan_id": "plan-r42-a2",
        "base": null,
        "head": HEAD,
        "event": "push",
        "runner": {"label":"ubuntu-26.04", "selection":"latest_default"},
        "trust": "trusted",
        "baseline": {"status":"unavailable", "reason":"test"},
        "generator": {"version":"0.1.1", "target":"x86_64-unknown-linux-gnu", "sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
        "packages": [],
        "obligations": [],
        "matrix": {"include":[]},
        "task_ids": [],
        "artifact_tasks": [],
        "warnings": [],
        "edges": []
    })
}

fn context() -> ArtifactBuildRunContext {
    ArtifactBuildRunContext {
        repository_id: "123456789".to_owned(),
        repository: "owner/repo".to_owned(),
        run_id: "42".to_owned(),
        run_attempt: 2,
    }
}

fn success_needs() -> &'static str {
    r#"{"plan":{"result":"success"},"task-linux":{"result":"success","outputs":{"task_report_artifact_id":"123","task_report_check_run_id":"456"}}}"#
}

fn needs_with_ids(artifact_id: &str, check_run_id: &str) -> String {
    format!(
        r#"{{"plan":{{"result":"success"}},"task-linux":{{"result":"success","outputs":{{"task_report_artifact_id":"{artifact_id}","task_report_check_run_id":"{check_run_id}"}}}}}}"#
    )
}

fn assert_error(request: &serde_json::Value, expected: &str) {
    let errors = error_list(&request.to_string());
    assert!(errors.iter().any(|error| error == expected), "{errors:?}");
    assert!(request.get("task_report_outputs").is_none());
}
