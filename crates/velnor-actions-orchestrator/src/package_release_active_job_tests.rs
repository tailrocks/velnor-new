use super::*;
use serde_json::{Value, json};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
fn workflow() -> WorkflowMetadata {
    WorkflowMetadata {
        id: 8,
        name: "Rust binary preview".to_owned(),
        path: ".github/workflows/binary-preview.yml".to_owned(),
        state: "active".to_owned(),
    }
}
fn identity(workflow: &WorkflowMetadata) -> CurrentRun<'_> {
    CurrentRun {
        id: 22,
        attempt: 3,
        source: RunSource {
            repository_id: 9,
            sha: SHA,
            branch: "main",
        },
        workflow,
        event: "workflow_run",
    }
}
fn route(path: &str) -> String {
    format!("repos/example/app/{path}")
}
fn run(status: &str) -> Value {
    json!({"id":22,"run_attempt":3,"workflow_id":8,"name":"Rust binary preview","path":".github/workflows/binary-preview.yml","head_sha":SHA,"head_branch":"main","event":"workflow_run","status":status,"conclusion":null,"repository":{"id":9},"head_repository":{"id":9}})
}
fn job(id: u64, name: &str, status: &str) -> Value {
    json!({"id":id,"run_id":22,"head_sha":SHA,"name":name,"status":status,"conclusion":null,"started_at":"2026-10-20T20:54:22Z"})
}
fn inventory(jobs: Vec<Value>) -> Value {
    let count = jobs.len();
    json!({"total_count":count,"jobs":Value::Array(jobs)})
}
fn jobs_route() -> String {
    route("actions/runs/22/attempts/3/jobs?per_page=100&page=1")
}
fn reads(status: &str) -> Vec<(String, Value)> {
    vec![
        (route("actions/runs/22/attempts/3"), run(status)),
        (route("actions/runs/22"), run(status)),
    ]
}

#[test]
fn five_running_payloads_and_one_queued_job_do_not_revoke_exact_running_role() {
    for status in ["queued", "in_progress"] {
        let jobs = (1..=6)
            .map(|id| {
                job(
                    id,
                    &format!("Build product / target-{id}"),
                    if id == 6 { "queued" } else { "in_progress" },
                )
            })
            .collect();
        let mut responses = reads(status);
        responses.push((jobs_route(), inventory(jobs)));
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        api.verify_current_role(&identity(&workflow()), "Build product / target-1")
            .expect("exact executing leaf despite aggregate queue");
        fixture.assert_drained();
    }
}

#[test]
fn active_role_rejects_terminal_unknown_and_superseded_workflow_identities() {
    for (pointer, value) in [
        ("/status", json!("completed")),
        ("/status", json!("waiting")),
        ("/status", json!("cancelled")),
        ("/status", json!("unknown")),
        ("/conclusion", json!("failure")),
        ("/run_attempt", json!(4)),
        ("/head_sha", json!("b".repeat(40))),
        ("/workflow_id", json!(9)),
        ("/repository/id", json!(10)),
        ("/head_repository/id", json!(10)),
    ] {
        for newer_endpoint in [false, true] {
            let mut changed = run("queued");
            *changed
                .pointer_mut(pointer)
                .expect("valid fixture authority") = value.clone();
            let responses = if newer_endpoint {
                vec![
                    (route("actions/runs/22/attempts/3"), run("queued")),
                    (route("actions/runs/22"), changed),
                ]
            } else {
                vec![(route("actions/runs/22/attempts/3"), changed)]
            };
            let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
            api.verify_current_role(&identity(&workflow()), "active")
                .expect_err("untrusted authority must fail");
            fixture.assert_drained();
        }
    }
}

#[test]
fn exact_role_rejects_missing_shadow_foreign_terminal_and_unstarted_jobs() {
    for mutation in 0..11 {
        let mut selected = job(81, "active", "in_progress");
        match mutation {
            0 => selected["id"] = json!(0),
            1 => selected["run_id"] = json!(23),
            2 => selected["head_sha"] = json!("b".repeat(40)),
            3 => selected["status"] = json!("queued"),
            4 => selected["status"] = json!("completed"),
            5 => selected["conclusion"] = json!("failure"),
            6 => selected["started_at"] = Value::Null,
            7 => selected["name"] = json!("other"),
            _ => {}
        }
        let mut rows = vec![selected.clone()];
        if mutation == 8 {
            selected["id"] = json!(82);
            rows.push(selected.clone());
        }
        if mutation == 9 {
            rows.push(selected.clone());
        }
        if mutation == 10 {
            rows.clear();
        }
        let mut responses = reads("queued");
        responses.push((jobs_route(), inventory(rows)));
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        api.verify_current_role(&identity(&workflow()), "active")
            .expect_err("untrusted authority must fail");
        fixture.assert_drained();
    }
}

#[test]
fn active_locator_caches_only_identity_and_refreshes_status_by_fixed_id() {
    let mut responses = reads("queued");
    responses.push((
        jobs_route(),
        inventory(vec![job(81, "active", "in_progress")]),
    ));
    responses.extend(reads("in_progress"));
    responses.push((route("actions/jobs/81"), job(81, "active", "completed")));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let workflow = workflow();
    let identity = identity(&workflow);
    api.verify_current_role(&identity, "active")
        .expect("valid fixture authority");
    api.verify_current_role(&identity, "active")
        .expect_err("untrusted authority must fail");
    fixture.assert_drained();
    assert_eq!(
        fixture
            .observed()
            .iter()
            .filter(|r| r.contains("/jobs?per_page"))
            .count(),
        1
    );
}

#[test]
fn active_job_inventory_requires_complete_consistent_pages_and_unique_ids() {
    for mutation in 0..3 {
        let mut first = inventory(
            (1..=100)
                .map(|id| job(id, if id == 1 { "active" } else { "other" }, "in_progress"))
                .collect(),
        );
        first["total_count"] = json!(101);
        let mut second = inventory(vec![job(101, "other", "queued")]);
        second["total_count"] = json!(101);
        match mutation {
            0 => second["total_count"] = json!(102),
            1 => second["jobs"] = json!([]),
            _ => second["jobs"][0]["id"] = json!(1),
        }
        let (api, fixture) = GitHubApi::with_fixture(
            "example/app",
            vec![
                (jobs_route(), first),
                (
                    route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
                    second,
                ),
            ],
        );
        api.require_active_job(&identity(&workflow()), "active")
            .expect_err("untrusted authority must fail");
        fixture.assert_drained();
    }
}

#[test]
fn fixed_job_refresh_rejects_identity_changes_and_cancellation_without_rebaseline() {
    for (field, value) in [
        ("id", json!(82)),
        ("run_id", json!(23)),
        ("name", json!("other")),
        ("head_sha", json!("b".repeat(40))),
        ("status", json!("cancelled")),
        ("conclusion", json!("success")),
        ("started_at", Value::Null),
    ] {
        let mut changed = job(81, "active", "in_progress");
        changed[field] = value;
        let mut responses = reads("in_progress");
        responses.push((
            jobs_route(),
            inventory(vec![job(81, "active", "in_progress")]),
        ));
        responses.extend(reads("in_progress"));
        responses.push((route("actions/jobs/81"), changed));
        let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
        let workflow = workflow();
        let identity = identity(&workflow);
        api.verify_current_role(&identity, "active")
            .expect("valid fixture authority");
        api.verify_current_role(&identity, "active")
            .expect_err("untrusted authority must fail");
        fixture.assert_drained();
    }
}

#[test]
fn failed_job_admission_does_not_poison_retry_or_share_a_different_role() {
    let mut responses = reads("queued");
    responses.push((jobs_route(), inventory(vec![job(81, "active", "queued")])));
    responses.extend(reads("queued"));
    responses.push((
        jobs_route(),
        inventory(vec![job(81, "active", "in_progress")]),
    ));
    responses.extend(reads("queued"));
    responses.push((
        jobs_route(),
        inventory(vec![job(81, "active", "in_progress")]),
    ));
    let (api, fixture) = GitHubApi::with_fixture("example/app", responses);
    let workflow = workflow();
    let identity = identity(&workflow);
    api.verify_current_role(&identity, "active")
        .expect_err("untrusted authority must fail");
    api.verify_current_role(&identity, "active")
        .expect("valid fixture authority");
    api.verify_current_role(&identity, "other")
        .expect_err("untrusted authority must fail");
    fixture.assert_drained();
}

#[test]
fn complete_second_page_selects_exact_role_without_accepting_partial_inventory() {
    let mut first = inventory((1..=100).map(|id| job(id, "other", "queued")).collect());
    first["total_count"] = json!(101);
    let mut second = inventory(vec![job(101, "active", "in_progress")]);
    second["total_count"] = json!(101);
    let (api, fixture) = GitHubApi::with_fixture(
        "example/app",
        vec![
            (jobs_route(), first),
            (
                route("actions/runs/22/attempts/3/jobs?per_page=100&page=2"),
                second,
            ),
        ],
    );
    api.require_active_job(&identity(&workflow()), "active")
        .expect("valid fixture authority");
    fixture.assert_drained();
}
