use super::*;
use serde_json::Value;

fn jobs_page(jobs: &Value, total_count: usize) -> Value {
    json!({"total_count":total_count,"jobs":jobs})
}

fn list_route_page(page: usize) -> String {
    route(&format!(
        "actions/workflows/7/runs?head_sha={}&branch=main&event=push&per_page=100&page={page}",
        source().sha
    ))
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
    let mut responses = first_three(jobs_page(&json!(fillers), 101));
    responses.extend([
        (
            route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
            jobs_page(&json!([required()]), 101),
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
        let mut responses = first_three(jobs_page(&json!([required()]), 1));
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
        let mut responses = first_three(jobs_page(&json!([required()]), 1));
        responses.push((list_route_page(1), json!({"workflow_runs":[latest]})));
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
        fixture.assert_drained();
        assert_eq!(fixture.observed().len(), 4);
    }
}

#[test]
fn rest_run_identity_mutations_fail_before_job_lookup() {
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
            "accepted REST identity mutation {pointer}"
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
fn latest_and_current_run_repository_names_are_checked() {
    let mut foreign_latest = run_json();
    foreign_latest["repository"]["full_name"] = json!("attacker/fork");
    let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
    responses.push((
        list_route_page(1),
        json!({"workflow_runs":[foreign_latest]}),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 4);

    let mut foreign_current = run_json();
    foreign_current["head_repository"]["full_name"] = json!("attacker/fork");
    let mut responses = first_three(json!({"total_count":1,"jobs":[required()]}));
    responses.extend([
        (list_route_page(1), json!({"workflow_runs":[run_json()]})),
        (route("actions/runs/22"), foreign_current),
    ]);
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    assert!(api.verify_ci_identity(22, 3, source(), true).is_err());
    fixture.assert_drained();
    assert_eq!(fixture.observed().len(), 5);
}
