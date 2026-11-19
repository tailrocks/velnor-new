// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::budget::Population;
use velnor_ci::budget_history::{gather, sample};
use velnor_ci::github::{Repo, event_runs_path, jobs_path, run_path};
use velnor_model::WORKFLOW_PATH;

use crate::support::FakeGitHub;

fn repo() -> Repo {
    Repo::parse("owner/repo").unwrap_or_else(|e| unreachable!("{e}"))
}

fn workflow_file() -> String {
    let path = WORKFLOW_PATH;
    path.as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned()
}

fn run(fake: &mut FakeGitHub, id: u64) {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "completed", "conclusion": "success",
            "run_attempt": 1, "event": "push", "head_sha": "a".repeat(40),
            "created_at": "2026-11-01T10:00:00Z",
            "run_started_at": "2026-11-01T10:00:00Z",
            "updated_at": "2026-11-01T10:05:00Z"
        })
        .to_string(),
    );
}

fn job(name: &str, completed_at: Option<&str>) -> Value {
    json!({
        "id": 1, "name": name, "conclusion": "success",
        "created_at": "2026-11-01T10:00:00Z",
        "started_at": "2026-11-01T10:00:10Z",
        "completed_at": completed_at,
        "steps": []
    })
}

fn jobs(fake: &mut FakeGitHub, id: u64, page: u32, total: usize, jobs: &[Value]) {
    fake.respond(
        jobs_path(&repo(), id, 1, page),
        &json!({ "total_count": total, "jobs": jobs }).to_string(),
    );
}

fn listed(id: u64, event: &str, branch: &str) -> Value {
    json!({
        "id": id, "event": event, "run_attempt": 1, "head_branch": branch,
        "status": "completed", "conclusion": "success"
    })
}

fn listing(fake: &mut FakeGitHub, event: &str, branch: Option<&str>, page: u32, runs: &[Value]) {
    fake.respond(
        event_runs_path(&repo(), &workflow_file(), event, branch, page),
        &json!({ "total_count": 1000, "workflow_runs": runs }).to_string(),
    );
}

#[test]
fn a_page_that_ends_before_the_job_count_is_refused_not_summed() {
    let mut fake = FakeGitHub::default();
    run(&mut fake, 7);
    let page: Vec<Value> = (0..100)
        .map(|n| job(&format!("job-{n}"), Some("2026-11-01T10:01:10Z")))
        .collect();
    jobs(&mut fake, 7, 1, 150, &page);
    jobs(&mut fake, 7, 2, 150, &[]);
    let error = sample(&fake, &repo(), 7).expect_err("a partial job list was accepted");
    assert!(
        error.to_string().contains("refusing incomplete jobs data"),
        "{error}"
    );
}

#[test]
fn a_finished_job_without_a_completion_time_is_a_missing_measurement_not_zero_seconds() {
    let mut fake = FakeGitHub::default();
    run(&mut fake, 7);
    jobs(
        &mut fake,
        7,
        1,
        2,
        &[
            job("fast", Some("2026-11-01T10:01:10Z")),
            job("broken", None),
        ],
    );
    let error = sample(&fake, &repo(), 7).expect_err("a job without completed_at was accepted");
    assert!(error.to_string().contains("broken"), "{error}");
    assert!(
        error.to_string().contains("completed_at missing"),
        "{error}"
    );
}

#[test]
fn complete_runs_are_summed_per_job() {
    let mut fake = FakeGitHub::default();
    run(&mut fake, 7);
    jobs(
        &mut fake,
        7,
        1,
        2,
        &[
            job("a", Some("2026-11-01T10:01:10Z")),
            job("b", Some("2026-11-01T10:02:10Z")),
        ],
    );
    let measured = sample(&fake, &repo(), 7).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(measured.runner_seconds, 60 + 120);
    assert_eq!(measured.wall_seconds, 300);
}

#[test]
fn the_run_history_stops_paging_once_every_population_has_enough_runs() {
    let mut fake = FakeGitHub::default();
    let pages = |event: &str, first: u64| -> Vec<Value> {
        (0..100).map(|n| listed(first + n, event, "main")).collect()
    };
    listing(
        &mut fake,
        "pull_request",
        None,
        1,
        &pages("pull_request", 1000),
    );
    listing(&mut fake, "push", Some("main"), 1, &pages("push", 2000));
    listing(
        &mut fake,
        "schedule",
        None,
        1,
        &[listed(3000, "schedule", "main")],
    );
    for id in [1000, 1001, 2000, 2001, 3000] {
        run(&mut fake, id);
        jobs(
            &mut fake,
            id,
            1,
            1,
            &[job("only", Some("2026-11-01T10:01:10Z"))],
        );
    }
    let histories = gather(&fake, &repo(), "main", 2).unwrap_or_else(|e| unreachable!("{e}"));
    let sizes: Vec<(Population, Vec<u64>)> = histories
        .iter()
        .map(|history| {
            (
                history.population,
                history.samples.iter().map(|sample| sample.run_id).collect(),
            )
        })
        .collect();
    assert_eq!(
        sizes,
        vec![
            (Population::PullRequest, vec![1000, 1001]),
            (Population::MainPush, vec![2000, 2001]),
            (Population::Schedule, vec![3000]),
        ]
    );
}
