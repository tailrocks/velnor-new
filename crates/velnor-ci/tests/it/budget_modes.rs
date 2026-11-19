// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_ci::budget::{Evaluation, Population, evaluate};
use velnor_ci::budget_history::{Current, current, gather};
use velnor_ci::github::{Repo, event_runs_path, jobs_path, run_jobs_path, run_path};
use velnor_model::{ModeBudget, Selection, SpeedBudget, WORKFLOW_PATH};

use crate::budget::{budget, history, history_of, sample};
use crate::support::FakeGitHub;

fn repo() -> Repo {
    Repo::parse("owner/repo").unwrap_or_else(|e| unreachable!("{e}"))
}

/// 2026-11-01T10:00:00Z, the start of every recorded run below.
const STARTED: i64 = 1_793_527_200;
const FULL_RUN: u64 = 900;
const AFFECTED_RUN: u64 = 901;

fn at(seconds: u64) -> String {
    format!(
        "2026-11-01T{:02}:{:02}:{:02}Z",
        10 + seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Full runs are slow and affected runs are fast, as in the repository whose
/// default branch pushes were 625 to 1023 seconds with a full selection and 56
/// to 181 seconds with an affected one.
fn two_modes() -> SpeedBudget {
    SpeedBudget {
        full: ModeBudget {
            main_wall_seconds: Some(900),
            runner_seconds: Some(1000),
            ..ModeBudget::default()
        },
        affected: ModeBudget {
            main_wall_seconds: Some(100),
            runner_seconds: Some(150),
            ..ModeBudget::default()
        },
        ..SpeedBudget::default()
    }
}

fn breached(evaluation: &Evaluation) -> Vec<(Option<Selection>, u64, u64)> {
    evaluation
        .breaches
        .iter()
        .map(|breach| (breach.mode, breach.value, breach.threshold))
        .collect()
}

#[test]
fn an_affected_run_is_not_judged_by_the_limits_of_full_runs_and_the_reverse() {
    let histories = vec![
        history_of(
            Population::MainPush,
            Selection::Full,
            vec![sample(3, 800, 900, None), sample(2, 700, 800, None)],
        ),
        history_of(
            Population::MainPush,
            Selection::Affected,
            vec![sample(5, 90, 45, None), sample(4, 80, 45, None)],
        ),
    ];
    let evaluation = evaluate(&two_modes(), &histories);
    assert!(evaluation.passed(), "{evaluation:?}");

    let slow_affected = vec![history_of(
        Population::MainPush,
        Selection::Affected,
        vec![sample(6, 150, 45, None)],
    )];
    let evaluation = evaluate(&two_modes(), &slow_affected);
    assert_eq!(
        breached(&evaluation),
        [(Some(Selection::Affected), 150, 100)],
        "800 seconds is fine for a full run and not for an affected one"
    );
}

#[test]
fn a_mode_without_its_own_limit_uses_the_plain_key_and_zero_switches_it_off() {
    let mut limits = two_modes();
    limits.main_wall_seconds = 500;
    limits.runner_seconds = 500;
    limits.none = ModeBudget {
        main_wall_seconds: Some(0),
        ..ModeBudget::default()
    };
    let nothing = history_of(
        Population::MainPush,
        Selection::Nothing,
        vec![sample(7, 600, 400, None)],
    );
    let evaluation = evaluate(&limits, &[nothing]);
    assert!(
        evaluation.passed(),
        "the none table switched the wall check off: {evaluation:?}"
    );
    let nothing_slow_runner = history_of(
        Population::MainPush,
        Selection::Nothing,
        vec![sample(8, 600, 700, None)],
    );
    let evaluation = evaluate(&limits, &[nothing_slow_runner]);
    assert_eq!(
        breached(&evaluation),
        [(Some(Selection::Nothing), 700, 500)],
        "the runner limit falls back to the plain key"
    );
}

#[test]
fn a_run_with_an_unknown_selection_is_judged_by_the_plain_keys_only() {
    let mut limits = two_modes();
    limits.main_wall_seconds = 200;
    let unknown = vec![history(Population::MainPush, vec![sample(9, 300, 0, None)])];
    let evaluation = evaluate(&limits, &unknown);
    assert_eq!(breached(&evaluation), [(None, 300, 200)]);
}

#[test]
fn a_limit_in_a_mode_table_alone_enables_the_budget_and_names_missing_runs() {
    let only_full = SpeedBudget {
        full: ModeBudget {
            main_wall_seconds: Some(900),
            ..ModeBudget::default()
        },
        ..SpeedBudget::default()
    };
    assert!(only_full.enabled());
    let evaluation = evaluate(&only_full, &[]);
    assert_eq!(
        evaluation.missing,
        ["the generated workflow has no successful run to measure"]
    );
}

fn run_listing(fake: &mut FakeGitHub, ids: &[u64]) {
    let workflow = WORKFLOW_PATH
        .as_str()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_owned();
    let listed: Vec<Value> = ids
        .iter()
        .map(|id| {
            json!({
                "id": id, "event": "push", "run_attempt": 1, "head_branch": "main",
                "status": "completed", "conclusion": "success"
            })
        })
        .collect();
    for (event, branch, runs) in [
        ("push", Some("main"), listed),
        ("pull_request", None, Vec::new()),
        ("schedule", None, Vec::new()),
    ] {
        fake.respond(
            event_runs_path(&repo(), &workflow, event, branch, 1),
            &json!({ "total_count": 100, "workflow_runs": runs }).to_string(),
        );
    }
}

fn budget_job(title: Option<&str>, started: u64, done: Option<u64>) -> Value {
    json!({
        "id": 2, "name": title.unwrap_or("Speed budget"), "conclusion": done.map(|_| "success"),
        "created_at": at(0), "started_at": at(started), "completed_at": done.map(at),
        "steps": []
    })
}

fn rust_job(done: u64) -> Value {
    json!({
        "id": 1, "name": "rust", "conclusion": "success",
        "created_at": at(0), "started_at": at(1), "completed_at": at(done),
        "steps": []
    })
}

/// A finished default branch push that took `wall` seconds, whose budget job
/// was titled `title`.
fn finished(fake: &mut FakeGitHub, id: u64, wall: u64, title: Option<&str>) {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "completed", "conclusion": "success",
            "run_attempt": 1, "event": "push", "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(wall)
        })
        .to_string(),
    );
    let jobs = [rust_job(wall - 3), budget_job(title, wall - 2, Some(wall))];
    fake.respond(
        jobs_path(&repo(), id, 1, 1),
        &json!({ "total_count": 2, "jobs": jobs }).to_string(),
    );
}

/// The run that executes the budget job, `wall` seconds old.
fn executing(fake: &mut FakeGitHub, id: u64, wall: u64, title: Option<&str>) -> Current {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "in_progress", "conclusion": null,
            "run_attempt": 1, "event": "push", "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(wall)
        })
        .to_string(),
    );
    let jobs = [rust_job(wall - 3), budget_job(title, wall - 2, None)];
    fake.respond(
        run_jobs_path(&repo(), id, 1),
        &json!({ "total_count": 2, "jobs": jobs }).to_string(),
    );
    let now = STARTED + i64::try_from(wall).unwrap_or_default();
    current(fake, &repo(), id, now).unwrap_or_else(|e| unreachable!("{e}"))
}

fn judged(fake: &FakeGitHub, running: &Current, limits: &SpeedBudget) -> (Vec<u64>, Evaluation) {
    let histories =
        gather(fake, &repo(), "main", 10, Some(running)).unwrap_or_else(|e| unreachable!("{e}"));
    let ids = histories[0].samples.iter().map(|s| s.run_id).collect();
    (ids, evaluate(limits, &histories))
}

#[test]
fn a_full_run_after_only_affected_history_passes_within_its_own_limit() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1, 2, 3]);
    for id in [1, 2, 3] {
        finished(&mut fake, id, 60, Some("Speed budget (affected)"));
    }
    let running = executing(&mut fake, FULL_RUN, 700, Some("Speed budget (full)"));
    assert_eq!(running.sample.selection, Some(Selection::Full));
    let (ids, evaluation) = judged(&fake, &running, &two_modes());
    assert_eq!(ids, [FULL_RUN], "no affected run is a sample of a full run");
    assert!(evaluation.passed(), "{evaluation:?}");
}

#[test]
fn an_affected_run_is_not_judged_against_the_full_runs_before_it() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1, 2, 3, 4]);
    for id in [1, 2] {
        finished(&mut fake, id, 800, Some("Speed budget (full)"));
    }
    for id in [3, 4] {
        finished(&mut fake, id, 70, Some("Speed budget (affected)"));
    }
    let running = executing(&mut fake, AFFECTED_RUN, 80, Some("Speed budget (affected)"));
    let (ids, evaluation) = judged(&fake, &running, &two_modes());
    assert_eq!(ids, [AFFECTED_RUN, 3, 4]);
    assert!(evaluation.passed(), "{evaluation:?}");
    let slow = executing(
        &mut fake,
        AFFECTED_RUN + 1,
        140,
        Some("Speed budget (affected)"),
    );
    let (_, evaluation) = judged(&fake, &slow, &two_modes());
    assert_eq!(
        breached(&evaluation).first(),
        Some(&(Some(Selection::Affected), 140, 100)),
        "{evaluation:?}"
    );
}

#[test]
fn runs_that_do_not_record_their_selection_are_one_group_of_their_own() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1, 2, 3]);
    finished(&mut fake, 1, 800, None);
    finished(&mut fake, 2, 60, Some("Speed budget (affected)"));
    finished(&mut fake, 3, 790, None);
    let running = executing(&mut fake, FULL_RUN, 810, None);
    assert_eq!(running.sample.selection, None);
    let (ids, _) = judged(&fake, &running, &budget(0, 900, 0, 0));
    assert_eq!(ids, [FULL_RUN, 1, 3]);
}

#[test]
fn a_skipped_budget_job_records_nothing() {
    let mut fake = FakeGitHub::default();
    run_listing(&mut fake, &[1]);
    fake.respond(
        run_path(&repo(), 1),
        &json!({
            "name": "Rust CI", "status": "completed", "conclusion": "success",
            "run_attempt": 1, "event": "push", "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(60)
        })
        .to_string(),
    );
    let mut skipped = budget_job(Some("Speed budget (full)"), 1, None);
    skipped["conclusion"] = json!("skipped");
    fake.respond(
        jobs_path(&repo(), 1, 1, 1),
        &json!({ "total_count": 2, "jobs": [rust_job(50), skipped] }).to_string(),
    );
    let measured = velnor_ci::budget_history::sample(&fake, &repo(), 1)
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(measured.selection, None);
}

#[test]
fn the_per_mode_tables_of_the_budget_are_read_with_the_plain_keys_as_fallback() {
    let config = b"[budget]\nmain_wall_seconds = 700\nrunner_seconds = 800\n\n[budget.full]\nmain_wall_seconds = 900\n\n[budget.affected]\nmain_wall_seconds = 100\nrunner_seconds = 0\n";
    let parsed = velnor_rust::budget(config)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .unwrap_or_else(|| unreachable!("the table is missing"));
    assert_eq!(parsed.main_wall_seconds, 700);
    assert_eq!(parsed.full.main_wall_seconds, Some(900));
    assert_eq!(parsed.affected.runner_seconds, Some(0));
    assert!(!parsed.none.is_set());
    let limits = parsed.for_mode(Some(Selection::Full));
    assert_eq!(
        (limits.main_wall_seconds, limits.runner_seconds),
        (900, 800)
    );
    let limits = parsed.for_mode(Some(Selection::Affected));
    assert_eq!((limits.main_wall_seconds, limits.runner_seconds), (100, 0));
    let limits = parsed.for_mode(Some(Selection::Nothing));
    assert_eq!(
        (limits.main_wall_seconds, limits.runner_seconds),
        (700, 800)
    );
    let limits = parsed.for_mode(None);
    assert_eq!(
        (limits.main_wall_seconds, limits.runner_seconds),
        (700, 800)
    );
}

#[test]
fn an_unknown_key_in_a_mode_table_or_an_unknown_mode_is_refused() {
    for (config, key) in [
        (
            "[budget.full]\nmain_wall_second = 900\n",
            "budget.full.main_wall_second",
        ),
        (
            "[budget.everything]\nmain_wall_seconds = 900\n",
            "budget.everything",
        ),
    ] {
        let error = velnor_rust::budget(config.as_bytes()).expect_err("the key was accepted");
        assert!(error.to_string().contains(key), "{error}");
    }
}

#[test]
fn the_title_of_the_budget_job_names_the_selection_and_nothing_else_does() {
    for selection in Selection::ALL {
        let title = format!("Speed budget ({})", selection.name());
        assert_eq!(Selection::of_job_title(&title), Some(selection));
    }
    for title in [
        "Speed budget",
        "Speed budget (unknown)",
        "Speed budget (everything)",
        "rust (full)",
        "Speed budget (full) ",
    ] {
        assert_eq!(Selection::of_job_title(title), None, "{title}");
    }
}
