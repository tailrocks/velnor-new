//! `merge_internal` entrypoint pins: envelope taxonomy plus the hermetic
//! no-work path over the real cover wiring.
//!
//! The envelope cases fail before cover runs (malformed JSON, schema,
//! run key); the verdict cases prove the moved entrypoint drives the
//! real cover port end to end (empty valid plan with agreeing matrix,
//! coherent trust, and one successful validator folds to `no_work`
//! with zero signals, no ambient environment consulted).

use serde_json::{Value, json};
use velnor_actions_orchestrator_internal::merge_entry::merge_internal;

/// Parse one merge response as JSON.
fn report_of(request: &Value) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_str(&merge_internal(
        &request.to_string(),
    )?)?)
}

/// Minimal valid empty plan: no obligations, legs, or packages.
fn empty_plan() -> Result<Value, Box<dyn std::error::Error>> {
    let plan_id = velnor_actions_contract::plan_id_for_run("local")?;
    Ok(json!({
        "schema": 1,
        "run_key": "local",
        "plan_id": plan_id,
        "base": null,
        "head": "0123456789abcdef0123456789abcdef01234567",
        "event": "local",
        "runner": {"label": "ubuntu-24.04", "selection": "latest_default"},
        "trust": "pr",
        "baseline": {"status": "unavailable"},
        "generator": {
            "version": "0.1.0",
            "target": "x86_64-unknown-linux-gnu",
            "sha256": "0000000000000000000000000000000000000000000000000000000000000000",
        },
        "packages": [],
        "obligations": [],
        "matrix": {"include": []},
        "task_ids": [],
    }))
}

/// Request envelope with empty evidence channels.
fn request(plan: Option<Value>) -> Value {
    let mut request = json!({
        "schema": 1,
        "run_key": "local",
        "matrix_reports": [],
        "required_job_ids": [],
        "required_jobs": [],
    });
    if let Some(plan) = plan {
        request["plan"] = plan;
    }
    request
}

#[test]
fn malformed_request_json_fails_before_cover() {
    let error = merge_internal("{oops").expect_err("malformed request fails");
    let problem = match error {
        velnor_actions_orchestrator_core::OrchestratorError::Internal { problem } => problem,
        _ => panic!("internal error, got {error:?}"),
    };
    assert!(
        problem.contains("malformed_json"),
        "strict-parse token: {problem}"
    );
}

#[test]
fn wellformed_wrong_shape_json_fails_before_cover() {
    let error = merge_internal(r#"{"schema":1}"#).expect_err("wrong-shape request fails");
    let problem = match error {
        velnor_actions_orchestrator_core::OrchestratorError::Internal { problem } => problem,
        _ => panic!("internal error, got {error:?}"),
    };
    assert!(
        problem.starts_with("malformed_request:"),
        "malformed token: {problem}"
    );
}

#[test]
fn unknown_schema_fails_before_cover() {
    let mut request = request(None);
    request["schema"] = json!(2);
    let error = merge_internal(&request.to_string()).expect_err("schema 2 fails");
    let problem = match error {
        velnor_actions_orchestrator_core::OrchestratorError::Internal { problem } => problem,
        _ => panic!("internal error, got {error:?}"),
    };
    assert_eq!(problem, "unsupported_schema:2");
}

#[test]
fn invalid_run_key_fails_before_cover() {
    let mut request = request(None);
    request["run_key"] = json!("");
    assert!(
        merge_internal(&request.to_string()).is_err(),
        "empty run key fails"
    );
}

#[test]
fn missing_plan_yields_source_missing_verdict() -> Result<(), Box<dyn std::error::Error>> {
    let report = report_of(&request(None))?;
    assert_eq!(report["status"], "planning_failed");
    assert_eq!(report["miss_reasons"], json!(["source_missing"]));
    assert_eq!(report["report_id"], "final-local");
    assert_eq!(
        report["plan_id"],
        velnor_actions_contract::plan_id_for_run("local")?
    );
    assert_eq!(report["counts"]["not_run"], 0);
    Ok(())
}

#[test]
fn assembly_errors_replace_source_missing_token() -> Result<(), Box<dyn std::error::Error>> {
    let mut request = request(None);
    request["assembly_errors"] = json!(["boom"]);
    let report = report_of(&request)?;
    assert_eq!(report["status"], "planning_failed");
    assert_eq!(report["miss_reasons"], json!(["cache_corrupt"]));
    Ok(())
}

#[test]
fn empty_valid_plan_yields_no_work() -> Result<(), Box<dyn std::error::Error>> {
    let mut request = request(Some(empty_plan()?));
    request["actual_event"] = json!("local");
    request["matrix"] = json!({"include": []});
    request["required_job_ids"] = json!(["gate"]);
    request["required_jobs"] = json!([{"job_id": "gate", "conclusion": "success"}]);
    let report = report_of(&request)?;
    assert_eq!(report["status"], "no_work");
    assert_eq!(report["miss_reasons"], json!([]));
    assert_eq!(report["report_id"], "final-local");
    assert_eq!(report["expected_report_ids"], json!([]));
    assert_eq!(report["downloaded_artifact_ids"], json!([]));
    assert_eq!(
        report["required_job_results"],
        json!([{"job_id": "gate", "conclusion": "success"}])
    );
    for count in [
        "selected",
        "reused",
        "executed",
        "empty_partition",
        "covered",
        "failed",
        "cancelled",
        "blocked",
        "not_run",
    ] {
        assert_eq!(report["counts"][count], 0, "zero {count}");
    }
    Ok(())
}
