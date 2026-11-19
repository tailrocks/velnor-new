// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::budget::{Population, evaluate};
use velnor_ci::budget_history::{Current, current, gather, sample};
use velnor_ci::github::{Repo, event_runs_path, jobs_path, run_jobs_path, run_path};
use velnor_model::WORKFLOW_PATH;

use crate::budget::budget;
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
    let histories = gather(&fake, &repo(), "main", 2, None).unwrap_or_else(|e| unreachable!("{e}"));
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

#[test]
fn a_history_with_no_scheduled_runs_is_read_for_a_bounded_number_of_pages() {
    let mut fake = FakeGitHub::default();
    let full = |event: &str, branch: &str, first: u64| -> Vec<Value> {
        (0..100)
            .map(|n| {
                let mut run = listed(first + n, event, branch);
                run["run_attempt"] = json!(2);
                run
            })
            .collect()
    };
    for page in 1..=10_u32 {
        let first = u64::from(page) * 1000;
        listing(
            &mut fake,
            "pull_request",
            None,
            page,
            &full("pull_request", "feature", first),
        );
        listing(
            &mut fake,
            "push",
            Some("main"),
            page,
            &full("push", "main", first),
        );
        listing(
            &mut fake,
            "schedule",
            None,
            page,
            &full("schedule", "main", first),
        );
    }
    let histories = gather(&fake, &repo(), "main", 2, None).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(histories.is_empty(), "{histories:?}");
}

const SLOW: u64 = 44_612_356_020;
const NORMAL: u64 = 44_610_215_475;
const QUICK: u64 = 44_609_189_060;
const REVERT: u64 = 44_613_446_455;
/// 2026-11-01T10:00:00Z, the start of every recorded run below.
const STARTED: i64 = 1_793_527_200;

fn at(seconds: u64) -> String {
    format!("2026-11-01T10:{:02}:{:02}Z", seconds / 60, seconds % 60)
}

fn recorded_run(fake: &mut FakeGitHub, id: u64, wall: u64) {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "completed", "conclusion": "success",
            "run_attempt": 1, "event": "push", "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(wall)
        })
        .to_string(),
    );
    jobs(fake, id, 1, 1, &[job("rust", Some(&at(wall)))]);
}

/// The lab's default-branch pushes, newest first, as `(run, wall, conclusion)`.
fn pushes(fake: &mut FakeGitHub, runs: &[(u64, u64, &str)]) {
    let listed: Vec<Value> = runs
        .iter()
        .map(|(id, _, conclusion)| {
            let mut run = listed(*id, "push", "main");
            run["conclusion"] = json!(conclusion);
            run
        })
        .collect();
    listing(fake, "push", Some("main"), 1, &listed);
    listing(fake, "pull_request", None, 1, &[]);
    listing(fake, "schedule", None, 1, &[]);
    for (id, wall, _) in runs {
        recorded_run(fake, *id, *wall);
    }
}

/// The run that executes the budget job: still in progress, `wall` seconds old.
fn executing(fake: &mut FakeGitHub, id: u64, event: &str, wall: u64) -> Current {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "in_progress", "conclusion": null,
            "run_attempt": 1, "event": event, "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(wall)
        })
        .to_string(),
    );
    let mut rust = job("rust", Some(&at(wall - 3)));
    rust["conclusion"] = json!("success");
    let mut budget = job("Speed budget", None);
    budget["conclusion"] = Value::Null;
    budget["started_at"] = json!(at(wall - 2));
    fake.respond(
        run_jobs_path(&repo(), id, 1),
        &json!({ "total_count": 2, "jobs": [rust, budget] }).to_string(),
    );
    let now = STARTED + i64::try_from(wall).unwrap_or_default();
    current(fake, &repo(), id, now).unwrap_or_else(|e| unreachable!("{e}"))
}

fn judged(fake: &FakeGitHub, running: &Current) -> velnor_ci::budget::Evaluation {
    let histories =
        gather(fake, &repo(), "main", 10, Some(running)).unwrap_or_else(|e| unreachable!("{e}"));
    evaluate(&budget(0, 125, 0, 0), &histories)
}

#[test]
fn a_slow_run_is_judged_by_its_own_wall_time_not_by_the_run_before_it() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[(NORMAL, 115, "success"), (QUICK, 99, "success")],
    );
    let running = executing(&mut fake, SLOW, "push", 226);
    let evaluation = judged(&fake, &running);
    assert!(!evaluation.passed(), "{evaluation:?}");
    assert_eq!(evaluation.breaches.len(), 1, "{:?}", evaluation.breaches);
    assert_eq!(evaluation.breaches[0].value, 226);
    assert_eq!(evaluation.breaches[0].population, Population::MainPush);
}

#[test]
fn the_run_after_a_breach_passes_when_it_is_back_to_normal() {
    let mut fake = FakeGitHub::default();
    // The slow run passed the old check, so it is listed as a success.
    pushes(
        &mut fake,
        &[
            (SLOW, 226, "success"),
            (NORMAL, 115, "success"),
            (QUICK, 99, "success"),
        ],
    );
    let running = executing(&mut fake, REVERT, "push", 93);
    let evaluation = judged(&fake, &running);
    assert!(evaluation.passed(), "{evaluation:?}");
}

#[test]
fn a_run_that_failed_the_budget_is_not_a_sample_for_the_next_run() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[
            (SLOW, 226, "failure"),
            (NORMAL, 115, "success"),
            (QUICK, 99, "success"),
        ],
    );
    let running = executing(&mut fake, REVERT, "push", 93);
    let histories =
        gather(&fake, &repo(), "main", 10, Some(&running)).unwrap_or_else(|e| unreachable!("{e}"));
    let ids: Vec<u64> = histories[0].samples.iter().map(|s| s.run_id).collect();
    assert_eq!(ids, [REVERT, NORMAL, QUICK]);
    assert!(judged(&fake, &running).passed());
}

#[test]
fn the_running_run_is_never_counted_twice_and_the_window_stays_at_the_last_runs() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[
            (REVERT, 93, "success"),
            (NORMAL, 115, "success"),
            (QUICK, 99, "success"),
        ],
    );
    let running = executing(&mut fake, REVERT, "push", 93);
    let histories =
        gather(&fake, &repo(), "main", 2, Some(&running)).unwrap_or_else(|e| unreachable!("{e}"));
    let ids: Vec<u64> = histories[0].samples.iter().map(|s| s.run_id).collect();
    assert_eq!(ids, [REVERT, NORMAL]);
}

#[test]
fn a_manual_run_has_no_population_so_only_the_recorded_runs_are_judged() {
    let mut fake = FakeGitHub::default();
    pushes(
        &mut fake,
        &[(NORMAL, 115, "success"), (QUICK, 99, "success")],
    );
    let running = executing(&mut fake, SLOW, "workflow_dispatch", 226);
    assert_eq!(running.population, None);
    let ids: Vec<u64> = gather(&fake, &repo(), "main", 10, Some(&running))
        .unwrap_or_else(|e| unreachable!("{e}"))[0]
        .samples
        .iter()
        .map(|s| s.run_id)
        .collect();
    assert_eq!(ids, [NORMAL, QUICK]);
}
