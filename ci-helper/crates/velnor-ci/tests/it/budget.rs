// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::budget::{History, Population, Sample, evaluate};
use velnor_model::SpeedBudget;

fn sample(run_id: u64, wall: u64, runner: u64, setup: Option<u64>) -> Sample {
    Sample {
        run_id,
        wall_seconds: wall,
        runner_seconds: runner,
        setup_seconds: setup,
        jobs: Vec::new(),
    }
}

fn history(population: Population, samples: Vec<Sample>) -> History {
    History {
        population,
        samples,
    }
}

fn budget(pr_wall: u64, main_wall: u64, runner: u64, setup: u64) -> SpeedBudget {
    SpeedBudget {
        pr_wall_seconds: pr_wall,
        main_wall_seconds: main_wall,
        runner_seconds: runner,
        setup_seconds: setup,
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
