use super::*;
use serde_json::{Value, json};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn source() -> RunSource<'static> {
    RunSource {
        repository_id: 9,
        sha: SHA,
        branch: "main",
    }
}
fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}
fn metadata() -> Value {
    json!({"id":7,"name":"CI","path":".github/workflows/ci.yml","state":"active"})
}
fn run() -> Value {
    json!({"id":22,"run_attempt":3,"run_number":12,"workflow_id":7,"name":"CI",
        "path":".github/workflows/ci.yml","head_sha":SHA,"head_branch":"main",
        "event":"push","status":"completed","conclusion":"success",
        "repository":{"id":9},"head_repository":{"id":9}})
}
fn list_route() -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={SHA}&branch=main&event=push&per_page=100&page=1"
    ))
}
fn discovery(value: &Value) -> Vec<(String, Value)> {
    vec![
        (route("actions/workflows/ci.yml"), metadata()),
        (list_route(), json!({"workflow_runs":[value.clone()]})),
        (list_route(), json!({"workflow_runs":[value]})),
    ]
}
#[test]
fn continuation_pending_or_failed_latest_ci_never_admits_historical_success() {
    for (status, conclusion) in [
        ("in_progress", Value::Null),
        ("completed", json!("failure")),
    ] {
        let mut value = run();
        value["status"] = json!(status);
        value["conclusion"] = conclusion;
        let (api, fixture) = GitHubApi::with_fixture("example/app", discovery(&value));
        assert_eq!(
            latest_ci(&api, source()).expect("admitted read-only API result"),
            None
        );
        fixture.assert_drained();
    }
}
#[test]
fn continuation_missing_ci_is_pending_without_release_or_mutation_reads() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (route("actions/workflows/ci.yml"), metadata()),
            (list_route(), json!({"workflow_runs":[]})),
        ],
    );
    assert_eq!(
        latest_ci(&api, source()).expect("admitted read-only API result"),
        None
    );
    fixture.assert_drained();
}
#[test]
fn continuation_success_reauthenticates_required_and_latest_attempt() {
    let mut responses = discovery(&run());
    responses.extend([
        (route("actions/workflows/ci.yml"), metadata()),
        (route("actions/runs/22/attempts/3"), run()),
        (route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
         json!({"jobs":[{"run_id":22,"name":"Required","head_sha":SHA,"status":"completed","conclusion":"success"}]})),
        (list_route(), json!({"workflow_runs":[run()]})),
        (route("actions/runs/22"), run())]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert_eq!(
        latest_ci(&api, source()).expect("admitted read-only API result"),
        Some((22, 3))
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 8);
}
#[test]
fn continuation_main_ref_must_be_exact_commit_identity() {
    for value in [
        json!({"ref":"refs/heads/other","object":{"type":"commit","sha":SHA}}),
        json!({"ref":"refs/heads/main","object":{"type":"tag","sha":SHA}}),
        json!({"ref":"refs/heads/main","object":{"type":"commit","sha":"bad"}}),
    ] {
        let (api, fixture) =
            GitHubApi::with_fixture("example/app", vec![(route("git/ref/heads/main"), value)]);
        assert!(head(&api, "main").is_err());
        fixture.assert_drained();
    }
}

#[test]
fn continuation_current_source_rejects_head_drift_before_ci_reads() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (
                "repos/example/app".to_owned(),
                json!({"id":9,"full_name":"example/app"}),
            ),
            (
                route("git/ref/heads/main"),
                json!({"ref":"refs/heads/main","object":{"type":"commit","sha":"b".repeat(40)}}),
            ),
        ],
    );
    let error =
        verify_source(&api, "example/app", "main", SHA).expect_err("source freshness rejection");
    assert!(error.to_string().contains("tap_source_release_superseded"));
    fixture.assert_drained();
}
#[test]
fn continuation_current_source_rejects_new_failed_ci_without_publication_writes() {
    let mut failed = run();
    failed["conclusion"] = json!("failure");
    let mut responses = vec![
        (
            "repos/example/app".to_owned(),
            json!({"id":9,"full_name":"example/app"}),
        ),
        (
            route("git/ref/heads/main"),
            json!({"ref":"refs/heads/main","object":{"type":"commit","sha":SHA}}),
        ),
    ];
    responses.extend(discovery(&failed));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let error =
        verify_source(&api, "example/app", "main", SHA).expect_err("source freshness rejection");
    assert!(
        error
            .to_string()
            .contains("tap_source_latest_ci_not_successful")
    );
    fixture.assert_drained();
}

#[test]
fn continuation_newer_attempt_invalidates_success_before_candidate_selection() {
    let mut responses = discovery(&run());
    let mut newer = run();
    newer["run_attempt"] = json!(4);
    newer["status"] = json!("in_progress");
    newer["conclusion"] = Value::Null;
    responses.extend([
        (route("actions/workflows/ci.yml"), metadata()),
        (route("actions/runs/22/attempts/3"), run()),
        (route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
         json!({"jobs":[{"run_id":22,"name":"Required","head_sha":SHA,"status":"completed","conclusion":"success"}]})),
        (list_route(), json!({"workflow_runs":[run()]})),
        (route("actions/runs/22"), newer)]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(latest_ci(&api, source()).is_err());
    fixture.assert_drained();
}
