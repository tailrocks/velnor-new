use super::tests::{run_json, source};
use super::*;
use serde_json::{Value, json};

fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}
fn metadata() -> Value {
    json!({"id":7,"name":"CI","path":".github/workflows/ci.yml","state":"active"})
}
fn required() -> Value {
    json!({"run_id":22,"name":"Required","head_sha":source().sha,"status":"completed","conclusion":"success"})
}
fn list_route() -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={}&branch=main&event=push&per_page=100&page=1",
        source().sha
    ))
}
fn first_three(jobs: Value) -> Vec<(String, Value)> {
    vec![
        (route("actions/workflows/ci.yml"), metadata()),
        (route("actions/runs/22/attempts/3"), run_json()),
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
            jobs,
        ),
    ]
}

#[test]
fn real_api_ci_admission_checks_full_dispatch_order_and_current_attempt() {
    let mut responses = first_three(json!({"jobs":[required()]}));
    responses.extend([
        (list_route(), json!({"workflow_runs":[run_json()]})),
        (route("actions/runs/22"), run_json()),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), true)
        .expect("fully authenticated current CI");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 5);
}

#[test]
fn real_api_stops_at_disabled_or_wrong_canonical_workflow_metadata() {
    for (field, value) in [
        ("state", json!("disabled_manually")),
        ("path", json!(".github/workflows/other.yml")),
        ("name", json!("Other CI")),
        ("id", json!(0)),
    ] {
        let mut response = metadata();
        response[field] = value;
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![(route("actions/workflows/ci.yml"), response)],
        );
        assert!(
            api.verify_ci_identity(22, 3, source(), true).is_err(),
            "accepted {field}"
        );
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), 1);
    }
}

#[test]
fn real_api_rejects_stale_latest_or_new_attempt_before_publication() {
    for newer_at_list in [true, false] {
        let mut newer = run_json();
        newer["run_attempt"] = json!(4);
        let mut responses = first_three(json!({"jobs":[required()]}));
        responses.push((
            list_route(),
            json!({"workflow_runs":[if newer_at_list {newer.clone()} else {run_json()}]}),
        ));
        if !newer_at_list {
            responses.push((route("actions/runs/22"), newer));
        }
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), if newer_at_list { 4 } else { 5 });
    }
}

#[test]
fn real_api_rejects_duplicate_required_before_any_latest_lookup() {
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        first_three(json!({"jobs":[required(),required()]})),
    );
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 3);
}

#[test]
fn real_api_reads_required_from_second_job_page_and_preserves_original_ci() {
    let fillers = (0..100).map(|n| json!({"run_id":22,"name":format!("Other {n}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})).collect::<Vec<_>>();
    let mut responses = first_three(json!({"jobs":fillers}));
    responses.push((
        route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
        json!({"jobs":[required()]}),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), false)
        .expect("original historical CI remains authenticated");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 4);
}

#[test]
fn queued_fixture_rejects_mutation_transport_without_any_process_or_network() {
    let (api, fixture) = GitHubApi::with_fixture("example/app", vec![]);
    let error = api
        .post_fields(&route("releases"), &[])
        .expect_err("fixture must never fall through to gh");
    assert!(
        error
            .to_string()
            .contains("unexpected_fixture_transport_request"),
        "got {error}"
    );
    assert_eq!(fixture.observed(), Vec::<String>::new());
    fixture.assert_drained();
}
