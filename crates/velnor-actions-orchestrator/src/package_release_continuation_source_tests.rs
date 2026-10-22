use super::*;
use serde_json::{Value, json};
use std::time::Duration;

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
        "actions/workflows/7/runs?head_sha={SHA}&per_page=100&page=1"
    ))
}
fn empty_listing() -> (String, Value) {
    (list_route(), json!({"workflow_runs":[]}))
}
fn reauthentication() -> Vec<(String, Value)> {
    vec![
        (route("actions/runs/22/attempts/3"), run()),
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
            json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"name":"Required","head_sha":SHA,"status":"completed","conclusion":"success"}]}),
        ),
    ]
}
fn current_main() -> Vec<(String, Value)> {
    vec![
        (
            "repos/example/app".to_owned(),
            json!({"id":9,"full_name":"example/app"}),
        ),
        (
            route("git/ref/heads/main"),
            json!({"ref":"refs/heads/main","object":{"type":"commit","sha":SHA}}),
        ),
    ]
}
fn discovery(value: &Value) -> Vec<(String, Value)> {
    vec![
        (route("actions/workflows/ci.yml"), metadata()),
        (list_route(), json!({"workflow_runs":[value.clone()]})),
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
    let mut responses = vec![(route("actions/workflows/ci.yml"), metadata())];
    responses.extend((0..6).map(|_| (list_route(), json!({"workflow_runs":[]}))));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert_eq!(
        super::latest_ci_with_sleep(&api, source(), |_| Ok(()))
            .expect("admitted read-only API result"),
        None
    );
    fixture.assert_drained();
}
#[test]
fn continuation_success_reauthenticates_required_and_latest_attempt() {
    let mut responses = discovery(&run());
    responses.extend([
        (route("actions/runs/22/attempts/3"), run()),
        (route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
         json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"name":"Required","head_sha":SHA,"status":"completed","conclusion":"success"}]})),
        (list_route(), json!({"workflow_runs":[run()]})),
        (route("actions/runs/22"), run())]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert_eq!(
        latest_ci(&api, source()).expect("admitted read-only API result"),
        Some((22, 3))
    );
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 6);
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
    let error = verify_source(&api, "example/app", "main", SHA, |_| Ok(()))
        .expect_err("source freshness rejection");
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
    let error = verify_source(&api, "example/app", "main", SHA, |_| Ok(()))
        .expect_err("source freshness rejection");
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
        (route("actions/runs/22/attempts/3"), run()),
        (route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
         json!({"total_count":1,"jobs":[{"id":81,"run_id":22,"name":"Required","head_sha":SHA,"status":"completed","conclusion":"success"}]})),
        (list_route(), json!({"workflow_runs":[run()]})),
        (route("actions/runs/22"), newer)]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(latest_ci(&api, source()).is_err());
    fixture.assert_drained();
}

#[test]
fn continuation_empty_listing_then_success_is_admitted_after_backoff() {
    let mut responses = vec![
        (route("actions/workflows/ci.yml"), metadata()),
        empty_listing(),
        empty_listing(),
        (list_route(), json!({"workflow_runs":[run()]})),
    ];
    responses.extend(reauthentication());
    // The latest-attempt recheck is a fresh listing with the same policy.
    responses.extend([
        empty_listing(),
        (list_route(), json!({"workflow_runs":[run()]})),
        (route("actions/runs/22"), run()),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let mut waits = Vec::new();
    assert_eq!(
        latest_ci_with_sleep(&api, source(), |delay| {
            waits.push(delay);
            Ok(())
        })
        .expect("admitted after empty listings"),
        Some((22, 3))
    );
    fixture.assert_drained();
    assert_eq!(waits, [3, 6, 3].map(Duration::from_secs));
}

#[test]
fn continuation_current_source_fails_closed_when_listing_stays_empty() {
    let mut responses = current_main();
    responses.push((route("actions/workflows/ci.yml"), metadata()));
    responses.extend((0..6).map(|_| empty_listing()));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let mut waits = Vec::new();
    let error = verify_source(&api, "example/app", "main", SHA, |delay| {
        waits.push(delay);
        Ok(())
    })
    .expect_err("an exhausted empty listing never admits the source");
    assert!(
        error
            .to_string()
            .contains("tap_source_latest_ci_not_successful")
    );
    fixture.assert_drained();
    assert_eq!(waits, [3, 6, 12, 24, 45].map(Duration::from_secs));
}

#[test]
fn continuation_mismatching_listing_is_not_retried() {
    let mut foreign = run();
    foreign["head_repository"]["id"] = json!(10);
    let mut other_branch = run();
    other_branch["head_branch"] = json!("feature");
    let mut other_sha = run();
    other_sha["head_sha"] = json!("b".repeat(40));
    let mut other_workflow = run();
    other_workflow["workflow_id"] = json!(8);
    let mut dispatch = run();
    dispatch["event"] = json!("workflow_dispatch");
    for value in [foreign, other_branch, other_sha, other_workflow, dispatch] {
        let (api, fixture) = GitHubApi::with_fixture("example/app", discovery(&value));
        let waits = std::cell::Cell::new(0);
        assert!(
            latest_ci_with_sleep(&api, source(), |_| {
                waits.set(waits.get() + 1);
                Ok(())
            })
            .is_err(),
            "accepted mismatching run {value}"
        );
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), 2);
        assert_eq!(waits.get(), 0);
    }
}

#[test]
fn continuation_listing_api_error_is_not_retried() {
    // The fixture has no listing response queued: the first listing read is
    // an API failure, which must surface at once without a retry.
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![(route("actions/workflows/ci.yml"), metadata())],
    );
    let waits = std::cell::Cell::new(0);
    let error = latest_ci_with_sleep(&api, source(), |_| {
        waits.set(waits.get() + 1);
        Ok(())
    })
    .expect_err("an API failure is terminal");
    assert!(error.to_string().contains("unexpected_fixture_api_call"));
    assert_eq!(fixture.observed().len(), 2);
    assert_eq!(waits.get(), 0);
}
