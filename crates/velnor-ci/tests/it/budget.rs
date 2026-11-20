// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::budget::{History, Population, Sample, evaluate};
use velnor_model::{Selection, SpeedBudget};

pub(crate) fn sample(run_id: u64, wall: u64, runner: u64, setup: Option<u64>) -> Sample {
    Sample {
        run_id,
        selection: None,
        wall_seconds: wall,
        runner_seconds: runner,
        setup_seconds: setup,
        jobs: Vec::new(),
    }
}

pub(crate) fn history(population: Population, samples: Vec<Sample>) -> History {
    History {
        population,
        mode: None,
        samples,
    }
}

pub(crate) fn history_of(population: Population, mode: Selection, samples: Vec<Sample>) -> History {
    History {
        mode: Some(mode),
        ..history(population, samples)
    }
}

pub(crate) fn budget(pr_wall: u64, main_wall: u64, runner: u64, setup: u64) -> SpeedBudget {
    SpeedBudget {
        pr_wall_seconds: pr_wall,
        main_wall_seconds: main_wall,
        runner_seconds: runner,
        setup_seconds: setup,
        ..SpeedBudget::default()
    }
}

#[test]
fn a_budget_of_zero_switches_its_check_off() {
    let histories = vec![history(
        Population::PullRequest,
        vec![sample(1, 900, 800, Some(300))],
    )];
    let evaluation = evaluate(&budget(0, 0, 0, 0), &histories);
    assert!(evaluation.breaches.is_empty(), "{:?}", evaluation.breaches);
    assert!(evaluation.missing.is_empty(), "{:?}", evaluation.missing);
    assert!(evaluation.passed());
}

#[test]
fn a_measurement_equal_to_its_budget_passes_and_one_above_it_breaches() {
    let histories = vec![history(
        Population::PullRequest,
        vec![sample(1, 600, 400, Some(30))],
    )];
    let equal = evaluate(&budget(600, 600, 400, 30), &histories);
    assert!(equal.breaches.is_empty(), "{:?}", equal.breaches);

    let above = evaluate(&budget(599, 600, 400, 30), &histories);
    assert_eq!(above.breaches.len(), 1, "{:?}", above.breaches);
    assert_eq!(above.breaches[0].value, 600);
    assert_eq!(above.breaches[0].threshold, 599);
    assert!(!above.passed());
}

#[test]
fn a_pull_request_run_is_measured_against_the_pull_request_budget_and_a_push_against_the_other() {
    let pull = vec![history(
        Population::PullRequest,
        vec![sample(1, 600, 100, None)],
    )];
    let push = vec![history(
        Population::MainPush,
        vec![sample(2, 600, 100, None)],
    )];
    let pr = evaluate(&budget(500, 1000, 0, 0), &pull);
    assert_eq!(pr.breaches.len(), 1, "{:?}", pr.breaches);
    let main = evaluate(&budget(500, 1000, 0, 0), &push);
    assert!(main.breaches.is_empty(), "{:?}", main.breaches);
}

#[test]
fn the_median_of_the_window_breaches_the_budget_even_when_the_latest_run_is_under_it() {
    let histories = vec![history(
        Population::MainPush,
        vec![
            sample(3, 100, 100, None),
            sample(2, 900, 100, None),
            sample(1, 900, 100, None),
            sample(0, 900, 100, None),
        ],
    )];
    let evaluation = evaluate(&budget(0, 500, 0, 0), &histories);
    assert_eq!(evaluation.breaches.len(), 1, "{:?}", evaluation.breaches);
    assert_eq!(evaluation.breaches[0].value, 900);
    assert_eq!(evaluation.breaches[0].threshold, 500);
    assert_eq!(evaluation.medians[0].wall_seconds, 900);
}

#[test]
fn an_enabled_measurement_the_runs_do_not_have_is_missing_and_not_a_green_zero() {
    let histories = vec![history(
        Population::MainPush,
        vec![sample(1, 100, 100, None)],
    )];
    let evaluation = evaluate(&budget(0, 500, 0, 30), &histories);
    assert!(evaluation.breaches.is_empty(), "{:?}", evaluation.breaches);
    assert_eq!(evaluation.missing.len(), 1, "{:?}", evaluation.missing);
    assert!(
        evaluation.missing[0].contains("setup seconds"),
        "{:?}",
        evaluation.missing
    );
    assert!(!evaluation.passed());
}

#[test]
fn a_budget_for_a_kind_of_run_there_is_none_of_is_missing() {
    let histories = vec![history(
        Population::MainPush,
        vec![sample(1, 100, 100, None)],
    )];
    let evaluation = evaluate(&budget(300, 0, 0, 0), &histories);
    assert!(evaluation.breaches.is_empty(), "{:?}", evaluation.breaches);
    assert_eq!(evaluation.missing.len(), 1, "{:?}", evaluation.missing);
    assert!(
        evaluation.missing[0].contains("pull request runs are not measured"),
        "{:?}",
        evaluation.missing
    );
    assert!(!evaluation.passed());
}

#[test]
fn the_three_jobs_furthest_above_their_median_are_named() {
    let jobs = |name: &str, seconds: u64| (name.to_owned(), seconds);
    let mut latest = sample(4, 100, 100, None);
    latest.jobs = vec![
        jobs("rust-root", 500),
        jobs("rust-lint", 400),
        jobs("rust-test", 300),
        jobs("rust-policy", 200),
    ];
    let mut older = sample(3, 100, 100, None);
    older.jobs = vec![
        jobs("rust-root", 100),
        jobs("rust-lint", 100),
        jobs("rust-test", 100),
        jobs("rust-policy", 100),
    ];
    let histories = vec![history(Population::MainPush, vec![latest, older])];
    let evaluation = evaluate(&budget(0, 0, 0, 0), &histories);
    let names: Vec<&str> = evaluation
        .slowest_jobs
        .iter()
        .map(|job| job.name.as_str())
        .collect();
    assert_eq!(names, vec!["rust-root", "rust-lint", "rust-test"]);
    assert_eq!(evaluation.slowest_jobs[0].seconds, 500);
    assert_eq!(evaluation.slowest_jobs[0].median, 100);
}

#[test]
fn a_budget_without_any_run_to_measure_is_missing_and_not_a_green_zero() {
    let histories = vec![history(Population::MainPush, Vec::new())];
    let evaluation = evaluate(&budget(0, 600, 0, 0), &histories);
    assert!(evaluation.breaches.is_empty(), "{:?}", evaluation.breaches);
    assert_eq!(evaluation.missing.len(), 1, "{:?}", evaluation.missing);
    assert!(
        evaluation.missing[0].contains("no successful run"),
        "{:?}",
        evaluation.missing
    );
    assert!(!evaluation.passed());
}

fn listing() -> String {
    serde_json::json!({
        "total_count": 4,
        "workflow_runs": [
            {"id": 2, "event": "pull_request", "run_attempt": 1, "head_branch": "feature",
             "status": "completed", "conclusion": "success", "pull_requests": []},
            {"id": 4, "event": "pull_request", "run_attempt": 1, "head_branch": "feature",
             "status": "completed", "conclusion": "success", "pull_requests": []},
            {"id": 1, "event": "pull_request", "run_attempt": 1, "head_branch": "feature",
             "status": "completed", "conclusion": "failure", "pull_requests": []},
            {"id": 3, "event": "pull_request", "run_attempt": 2, "head_branch": "feature",
             "status": "completed", "conclusion": "success", "pull_requests": []}
        ]
    })
    .to_string()
}

pub(crate) fn run(started: &str, ended: &str) -> String {
    serde_json::json!({
        "name": "Rust CI", "status": "completed", "conclusion": "success", "run_attempt": 1,
        "event": "pull_request", "head_sha": "abc",
        "created_at": "2026-11-01T00:00:00Z", "run_started_at": started, "updated_at": ended
    })
    .to_string()
}

pub(crate) fn jobs(started: &str, ended: &str, restored: &str) -> String {
    serde_json::json!({
        "total_count": 1,
        "jobs": [{
            "id": 10, "name": "rust-root", "conclusion": "success",
            "created_at": started, "started_at": started, "completed_at": ended,
            "steps": [{
                "name": "velnor-ci restored", "conclusion": "success",
                "started_at": restored, "completed_at": restored
            }]
        }]
    })
    .to_string()
}

fn budget_command(dir: &std::path::Path, api: &str) -> std::process::Output {
    std::fs::create_dir_all(dir.join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(
        dir.join(".velnor/rust-ci.toml"),
        "[budget]\npr_wall_seconds = 500\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["bench", "budget", "--repo", "tailrocks/velnor-new"])
        .current_dir(dir)
        .env("GITHUB_API_URL", api)
        .env("GITHUB_TOKEN", "recorded")
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_STEP_SUMMARY");
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn the_command_measures_the_successful_first_attempts_and_names_the_slow_job() {
    let mut responses = std::collections::BTreeMap::new();
    responses.insert(
        "/repos/tailrocks/velnor-new".to_owned(),
        serde_json::json!({"id": 1, "default_branch": "main"}).to_string(),
    );
    let empty = serde_json::json!({"total_count": 0, "workflow_runs": []}).to_string();
    for query in ["event=push&branch=main", "event=schedule"] {
        responses.insert(
            format!("/repos/tailrocks/velnor-new/actions/workflows/rust-ci.yml/runs?status=success&{query}&per_page=100&page=1"),
            empty.clone(),
        );
    }
    responses.insert(
        "/repos/tailrocks/velnor-new/actions/workflows/rust-ci.yml/runs?status=success&event=pull_request&per_page=100&page=1".to_owned(),
        listing(),
    );
    responses.insert(
        "/repos/tailrocks/velnor-new/actions/runs/2".to_owned(),
        run("2026-11-01T00:00:10Z", "2026-11-01T00:10:10Z"),
    );
    responses.insert(
        "/repos/tailrocks/velnor-new/actions/runs/2/attempts/1/jobs?per_page=100&page=1".to_owned(),
        jobs(
            "2026-11-01T00:00:20Z",
            "2026-11-01T00:05:20Z",
            "2026-11-01T00:01:30Z",
        ),
    );
    responses.insert(
        "/repos/tailrocks/velnor-new/actions/runs/4".to_owned(),
        run("2026-11-01T00:00:10Z", "2026-11-01T00:06:50Z"),
    );
    responses.insert(
        "/repos/tailrocks/velnor-new/actions/runs/4/attempts/1/jobs?per_page=100&page=1".to_owned(),
        jobs(
            "2026-11-01T00:00:20Z",
            "2026-11-01T00:01:40Z",
            "2026-11-01T00:01:00Z",
        ),
    );
    let api = crate::io::serve(responses);
    let dir = crate::support::fresh_dir("velnor-ci-budget", "collect");
    let output = budget_command(&dir, &api);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(
        stdout.contains("pull request: 2 runs, median wall 500s, runner 190s, setup 55s"),
        "{stdout}{stderr}"
    );
    assert!(
        stdout.contains("rust-root (pull request): 300s, median 80s, +220s"),
        "{stdout}"
    );
    assert!(
        stderr.contains(
            "the pull request wall seconds of the latest run is 600s, above the budget 500s"
        ),
        "{stderr}"
    );
}

#[test]
fn a_window_with_one_run_that_lacks_the_setup_metric_fails_closed() {
    let histories = vec![history(
        Population::PullRequest,
        vec![
            sample(2, 100, 100, Some(10)),
            sample(1, 100, 100, None),
            sample(0, 100, 100, Some(10)),
        ],
    )];
    let evaluation = evaluate(&budget(0, 0, 0, 60), &histories);
    assert!(!evaluation.passed(), "{evaluation:?}");
    assert!(
        evaluation
            .missing
            .iter()
            .any(|text| text.contains("setup seconds of the measured runs is not measured")),
        "{:?}",
        evaluation.missing
    );
    assert_eq!(evaluation.medians[0].setup_seconds, None);
}
