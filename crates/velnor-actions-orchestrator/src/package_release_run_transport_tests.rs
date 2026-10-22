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
    json!({"id":81,"run_id":22,"name":"Required","head_sha":source().sha,"status":"completed","conclusion":"success"})
}
fn list_route() -> String {
    list_route_page(1)
}
fn list_route_page(page: usize) -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={}&branch=main&event=push&per_page=100&page={page}",
        source().sha,
    ))
}
fn jobs_page(jobs: &Value, total_count: usize) -> Value {
    json!({"total_count":total_count,"jobs":jobs["jobs"].clone()})
}
fn first_three(mut jobs: Value) -> Vec<(String, Value)> {
    if jobs.get("total_count").is_none() {
        let count = jobs["jobs"].as_array().map_or(0, Vec::len);
        jobs["total_count"] = json!(count);
    }
    vec![
        (route("actions/workflows/ci.yml"), metadata()),
        (route("actions/runs/22/attempts/3"), run_json()),
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
            jobs,
        ),
    ]
}

fn inventory_run(id: u64, run_number: u64) -> Value {
    let mut run = run_json();
    run["id"] = json!(id);
    run["run_number"] = json!(run_number);
    run
}

fn first_run_page() -> Value {
    json!({
        "workflow_runs": (0..100)
            .map(|index| inventory_run(1_000 + index, 1 + index))
            .collect::<Vec<_>>()
    })
}

#[test]
fn real_api_admits_required_job_and_latest_ci_from_later_inventory_pages() {
    let fillers = (0..100)
        .map(|n| {
            json!({"id":100+n,"run_id":22,"name":format!("Other {n}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})
        })
        .collect::<Vec<_>>();
    let mut latest = run_json();
    latest["run_number"] = json!(101);
    let mut responses = first_three(jobs_page(&json!({"jobs":fillers}), 101));
    responses.extend([
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
            jobs_page(&json!({"jobs":[required()]}), 101),
        ),
        (list_route_page(1), first_run_page()),
        (list_route_page(2), json!({"workflow_runs":[latest]})),
        (route("actions/runs/22"), run_json()),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), true)
        .expect("both paginated inventories authenticate the exact source");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 7);
}

#[test]
fn later_run_inventory_page_supersedes_older_success_with_failed_or_pending_run() {
    for (status, conclusion) in [
        ("completed", json!("failure")),
        ("in_progress", Value::Null),
    ] {
        let mut newer = inventory_run(23, 101);
        newer["status"] = json!(status);
        newer["conclusion"] = conclusion;
        let mut responses = first_three(json!({"jobs":[required()]}));
        responses.extend([
            (list_route_page(1), first_run_page()),
            (list_route_page(2), json!({"workflow_runs":[newer]})),
        ]);
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        assert!(
            api.verify_ci_identity(22, 3, source(), true).is_err(),
            "accepted newer {status} source CI run"
        );
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), 5);
    }
}

#[test]
fn latest_run_listing_rejects_a_different_run_or_attempt() {
    for latest in [inventory_run(23, 13), {
        let mut run = inventory_run(22, 13);
        run["run_attempt"] = json!(4);
        run
    }] {
        let mut responses = first_three(json!({"jobs":[required()]}));
        responses.push((list_route_page(1), json!({"workflow_runs":[latest]})));
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), 4);
    }
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
fn real_api_rejects_each_mutated_rest_run_identity_before_job_lookup() {
    for (pointer, replacement) in [
        ("/id", json!(23)),
        ("/run_attempt", json!(4)),
        ("/workflow_id", json!(8)),
        ("/name", json!("Other CI")),
        ("/path", json!(".github/workflows/other.yml")),
        ("/head_sha", json!("b".repeat(40))),
        ("/head_branch", json!("feature")),
        ("/event", json!("workflow_run")),
        ("/status", json!("in_progress")),
        ("/conclusion", json!("failure")),
        ("/repository/id", json!(10)),
        ("/repository/full_name", json!("attacker/fork")),
        ("/head_repository/id", json!(10)),
        ("/head_repository/full_name", json!("attacker/fork")),
        ("/repository", Value::Null),
        ("/head_repository", Value::Null),
        ("/workflow_id", Value::Null),
    ] {
        let mut run = run_json();
        *run.pointer_mut(pointer).expect("fixture field") = replacement;
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![
                (route("actions/workflows/ci.yml"), metadata()),
                (route("actions/runs/22/attempts/3"), run),
            ],
        );
        assert!(
            api.verify_ci_identity(22, 3, source(), false).is_err(),
            "accepted REST run mutation {pointer}"
        );
        fixture.assert_drained();
        assert_eq!(
            fixture.observed().len(),
            2,
            "looked up jobs after {pointer}"
        );
    }
}

#[test]
fn real_api_rejects_duplicate_required_before_any_latest_lookup() {
    let mut duplicate = required();
    duplicate["id"] = json!(82);
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        first_three(json!({"jobs":[required(),duplicate]})),
    );
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 3);
}

#[test]
fn real_api_reads_required_from_second_job_page_and_preserves_original_ci() {
    let fillers = (0..100)
        .map(|n| {
            json!({"id":100+n,"run_id":22,"name":format!("Other {n}"),"head_sha":source().sha,"status":"completed","conclusion":"success"})
        })
        .collect::<Vec<_>>();
    let mut responses = first_three(jobs_page(&json!({"jobs":fillers}), 101));
    responses.push((
        route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
        jobs_page(&json!({"jobs":[required()]}), 101),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), false)
        .expect("original historical CI remains authenticated");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 4);
}

#[test]
fn unconfigured_mutation_is_rejected_without_any_process_or_network() {
    let (api, fixture) = GitHubApi::with_fixture("example/app", vec![]);
    let error = api
        .post_fields(&route("releases"), &[])
        .expect_err("fixture must never fall through to gh");
    assert!(
        error.to_string().contains("unexpected_fixture_api_call"),
        "got {error}"
    );
    assert_eq!(
        fixture.observed(),
        vec![format!("POST {}", route("releases"))]
    );
    fixture.assert_drained();
}

#[test]
fn warm_completed_evidence_keeps_latest_list_and_current_run_fresh() {
    let mut responses = first_three(json!({"jobs":[required()]}));
    for _ in 0..2 {
        responses.extend([
            (list_route(), json!({"workflow_runs":[run_json()]})),
            (route("actions/runs/22"), run_json()),
        ]);
    }
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    for _ in 0..2 {
        api.verify_ci_identity(22, 3, source(), true)
            .expect("fresh latest CI");
    }
    fixture.assert_drained();
    assert_eq!(
        fixture.observed().len(),
        7,
        "immutable evidence read only once"
    );
}

#[test]
fn warm_completed_cache_never_accepts_new_attempt_pending_or_failed_current_ci() {
    for (field, value) in [
        ("run_attempt", json!(4)),
        ("status", json!("in_progress")),
        ("conclusion", json!("failure")),
        ("head_sha", json!("b".repeat(40))),
    ] {
        let mut changed = run_json();
        changed[field] = value;
        let mut responses = first_three(json!({"jobs":[required()]}));
        responses.extend([
            (list_route(), json!({"workflow_runs":[run_json()]})),
            (route("actions/runs/22"), changed),
        ]);
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        api.verify_ci_identity(22, 3, source(), false)
            .expect("original evidence");
        assert!(
            api.verify_ci_identity(22, 3, source(), true).is_err(),
            "accepted {field}"
        );
        fixture.assert_drained();
    }
}

#[test]
fn failed_required_evidence_is_not_cached_as_success() {
    let mut responses = first_three(json!({"jobs":[]}));
    responses.extend([
        (route("actions/runs/22/attempts/3"), run_json()),
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=1"),
            jobs_page(&json!({"jobs":[required()]}), 1),
        ),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), false).is_err());
    api.verify_ci_identity(22, 3, source(), false)
        .expect("new actual success");
    api.verify_ci_identity(22, 3, source(), false)
        .expect("cached exact success");
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 5);
}

#[test]
fn warm_completed_cache_still_rejects_a_newer_source_ci_run() {
    let mut newer = run_json();
    newer["id"] = json!(23);
    newer["run_number"] = json!(13);
    let mut responses = first_three(json!({"jobs":[required()]}));
    responses.push((list_route(), json!({"workflow_runs":[newer]})));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    api.verify_ci_identity(22, 3, source(), false)
        .expect("original success");
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
}

#[path = "package_release_run_release_transport_tests.rs"]
mod release_transport;

#[path = "package_release_run_inventory_transport_tests.rs"]
mod inventory_transport;
