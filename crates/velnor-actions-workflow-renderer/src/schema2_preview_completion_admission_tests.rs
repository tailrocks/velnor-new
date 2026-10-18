//! Completion payloads are candidates; authenticated run/workflow identity decides.
use super::support::{
    DEFAULT_BRANCH, OTHER_SHA, REPOSITORY, SOURCE_TOKEN, Scenario, assert_rejected, ci_run,
    execute_preview, job_pages, pages, required_job,
};
use serde_json::{Value, json};
use std::error::Error;

fn preview() -> Scenario {
    let mut scenario = Scenario::valid();
    scenario.event = "workflow_run".to_owned();
    scenario.workflow_ref =
        format!("{REPOSITORY}/.github/workflows/binary-preview.yml@refs/heads/{DEFAULT_BRANCH}");
    scenario
}

fn run() -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_str(&ci_run(
        42,
        12,
        3,
        SOURCE_TOKEN,
        DEFAULT_BRANCH,
        "push",
        ("completed", "success"),
    ))?)
}

fn event(run: &Value) -> Value {
    json!({"action":"completed", "repository":{"id":100,"full_name":REPOSITORY}, "workflow":{"id":200,"name":"CI","path":".github/workflows/ci.yml"}, "workflow_run":run})
}

#[test]
fn payload_and_authenticated_run_reject_each_wrong_identity_independently()
-> Result<(), Box<dyn Error>> {
    for (pointer, value) in [
        ("/id", json!(41)),
        ("/run_attempt", json!(2)),
        ("/workflow_id", json!(201)),
        ("/path", json!(".github/workflows/other.yml")),
        ("/name", json!("Other CI")),
        ("/event", json!("pull_request")),
        ("/head_sha", json!("a".repeat(40))),
        ("/head_branch", json!("other")),
        ("/status", json!("in_progress")),
        ("/conclusion", json!("failure")),
        ("/conclusion", Value::Null),
        ("/repository/id", json!(101)),
        ("/repository/full_name", json!("fork/other")),
        ("/head_repository/id", json!(101)),
        ("/head_repository/full_name", json!("fork/other")),
    ] {
        let mut wrong = run()?;
        *wrong.pointer_mut(pointer).ok_or("fixture pointer")? = value;
        for authenticated in [false, true] {
            let mut scenario = preview();
            if authenticated {
                scenario.completion_run = Some(wrong.to_string());
            } else {
                scenario.completion_event = Some(event(&wrong).to_string());
            }
            let result = execute_preview(&scenario)?;
            assert_rejected(&result);
            assert!(
                result.stderr.contains("completion run identity mismatch"),
                "{pointer}: {}",
                result.stderr
            );
        }
    }
    Ok(())
}

#[test]
fn completion_event_envelope_is_not_a_generic_privileged_trigger() -> Result<(), Box<dyn Error>> {
    for (pointer, value) in [
        ("/action", json!("requested")),
        ("/workflow/id", json!(201)),
        ("/workflow/name", json!("Other")),
        ("/workflow/path", json!(".github/workflows/other.yml")),
        ("/repository/id", json!(101)),
        ("/repository/full_name", json!("fork/other")),
    ] {
        let mut payload = event(&run()?);
        *payload.pointer_mut(pointer).ok_or("fixture pointer")? = value;
        let mut scenario = preview();
        scenario.completion_event = Some(payload.to_string());
        assert_rejected(&execute_preview(&scenario)?);
    }
    for payload in ["not JSON", "null", "{}"] {
        let mut scenario = preview();
        scenario.completion_event = Some(payload.to_owned());
        assert_rejected(&execute_preview(&scenario)?);
    }
    Ok(())
}

#[test]
fn configured_ci_name_alone_cannot_substitute_another_workflow() -> Result<(), Box<dyn Error>> {
    for workflow in [
        r#"{"id":201,"name":"CI","path":".github/workflows/ci.yml","state":"active"}"#,
        r#"{"id":200,"name":"Other","path":".github/workflows/ci.yml","state":"active"}"#,
        r#"{"id":200,"name":"CI","path":".github/workflows/other.yml","state":"active"}"#,
        r#"{"id":200,"name":"CI","path":".github/workflows/ci.yml","state":"disabled_manually"}"#,
        r#"{"id":null,"name":"CI","path":".github/workflows/ci.yml","state":"active"}"#,
    ] {
        let mut scenario = preview();
        scenario.ci_workflow = Some(workflow.to_owned());
        assert_rejected(&execute_preview(&scenario)?);
    }
    Ok(())
}

#[test]
fn completion_cannot_bypass_latest_rerun_required_or_current_tip() -> Result<(), Box<dyn Error>> {
    let mut pending = preview();
    pending.runs = pages(&[vec![ci_run(
        42,
        12,
        4,
        SOURCE_TOKEN,
        DEFAULT_BRANCH,
        "push",
        ("in_progress", ""),
    )]]);
    let result = execute_preview(&pending)?;
    assert_rejected(&result);
    assert_eq!(
        result
            .calls
            .lines()
            .filter(|line| line.ends_with("/ci.yml/runs"))
            .count(),
        1
    );
    let mut duplicate = preview();
    duplicate.jobs = job_pages(&[vec![
        required_job(42, SOURCE_TOKEN, DEFAULT_BRANCH, "completed", "success"),
        required_job(42, SOURCE_TOKEN, DEFAULT_BRANCH, "completed", "success"),
    ]]);
    assert_rejected(&execute_preview(&duplicate)?);
    let mut advanced = preview();
    advanced.second_tip = Some(format!(r#"[{{"sha":"{OTHER_SHA}"}}]"#));
    assert_rejected(&execute_preview(&advanced)?);
    Ok(())
}
