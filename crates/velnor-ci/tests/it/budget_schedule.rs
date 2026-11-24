// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The budget job runs on default branch pushes and on the nightly schedule, so
//! each of the two kinds of run must be judged against earlier runs of its own
//! event and selection.

use serde_json::{Value, json};
use velnor_ci::budget::{Population, evaluate};
use velnor_ci::budget_history::{Current, current, gather};
use velnor_ci::github::{Repo, event_runs_path, jobs_path, run_jobs_path, run_path};
use velnor_model::{ModeBudget, Selection, SpeedBudget, WORKFLOW_PATH};

use crate::support::FakeGitHub;

const STARTED: i64 = 1_793_527_200;
const NIGHTLY: u64 = 900;
const PUSH: u64 = 901;

fn repo() -> Repo {
    Repo::parse("owner/repo").unwrap_or_else(|e| unreachable!("{e}"))
}

fn at(seconds: u64) -> String {
    format!(
        "2026-11-01T{:02}:{:02}:{:02}Z",
        10 + seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn listing(fake: &mut FakeGitHub, event: &str, branch: Option<&str>, ids: &[u64]) {
    let path = WORKFLOW_PATH;
    let workflow = path.as_str().rsplit('/').next().unwrap_or_default();
    let runs: Vec<Value> = ids
        .iter()
        .map(|id| {
            json!({
                "id": id, "event": event, "run_attempt": 1, "head_branch": "main",
                "status": "completed", "conclusion": "success"
            })
        })
        .collect();
    fake.respond(
        event_runs_path(&repo(), workflow, event, branch, 1),
        &json!({ "total_count": 100, "workflow_runs": runs }).to_string(),
    );
}

fn jobs(wall: u64, title: &str, budget_done: Option<u64>) -> Value {
    let budget = json!({
        "id": 2, "name": title, "conclusion": budget_done.map(|_| "success"),
        "created_at": at(0), "started_at": at(wall - 2), "completed_at": budget_done.map(at),
        "steps": []
    });
    let rust = json!({
        "id": 1, "name": "rust", "conclusion": "success",
        "created_at": at(0), "started_at": at(1), "completed_at": at(wall - 3), "steps": []
    });
    json!({ "total_count": 2, "jobs": [rust, budget] })
}

fn finished(fake: &mut FakeGitHub, id: u64, event: &str, wall: u64, title: &str) {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "completed", "conclusion": "success",
            "run_attempt": 1, "event": event, "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(wall)
        })
        .to_string(),
    );
    fake.respond(
        jobs_path(&repo(), id, 1, 1),
        &jobs(wall, title, Some(wall)).to_string(),
    );
}

fn executing(fake: &mut FakeGitHub, id: u64, event: &str, wall: u64, title: &str) -> Current {
    fake.respond(
        run_path(&repo(), id),
        &json!({
            "name": "Rust CI", "status": "in_progress", "conclusion": null,
            "run_attempt": 1, "event": event, "head_sha": "a".repeat(40),
            "created_at": at(0), "run_started_at": at(0), "updated_at": at(wall)
        })
        .to_string(),
    );
    fake.respond(
        run_jobs_path(&repo(), id, 1),
        &jobs(wall, title, None).to_string(),
    );
    let now = STARTED + i64::try_from(wall).unwrap_or_default();
    current(fake, &repo(), id, now).unwrap_or_else(|e| unreachable!("{e}"))
}

/// Two full pushes that took 800 seconds, an affected push that took 70, and
/// two nightly full runs that took 1200.
fn lab() -> FakeGitHub {
    let mut fake = FakeGitHub::default();
    listing(&mut fake, "push", Some("main"), &[1, 2, 3]);
    listing(&mut fake, "schedule", None, &[4, 5]);
    listing(&mut fake, "pull_request", None, &[]);
    for id in [1, 2] {
        finished(&mut fake, id, "push", 800, "Speed budget (full)");
    }
    finished(&mut fake, 3, "push", 70, "Speed budget (affected)");
    for id in [4, 5] {
        finished(&mut fake, id, "schedule", 1200, "Speed budget (full)");
    }
    fake
}

fn limits() -> SpeedBudget {
    SpeedBudget {
        full: ModeBudget {
            main_wall_seconds: Some(1300),
            ..ModeBudget::default()
        },
        affected: ModeBudget {
            main_wall_seconds: Some(100),
            ..ModeBudget::default()
        },
        ..SpeedBudget::default()
    }
}

fn ids(
    histories: &[velnor_ci::budget::History],
    population: Population,
    mode: Selection,
) -> Vec<u64> {
    histories
        .iter()
        .find(|history| history.population == population && history.mode == Some(mode))
        .map_or_default(|history| history.samples.iter().map(|sample| sample.run_id).collect())
}

#[test]
fn a_nightly_full_run_is_judged_against_earlier_nightly_runs_not_full_pushes() {
    let mut fake = lab();
    let running = executing(&mut fake, NIGHTLY, "schedule", 1250, "Speed budget (full)");
    assert_eq!(running.population, Some(Population::Schedule));
    let histories =
        gather(&fake, &repo(), "main", 10, Some(&running)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        ids(&histories, Population::Schedule, Selection::Full),
        [NIGHTLY, 4, 5]
    );
    assert_eq!(
        ids(&histories, Population::MainPush, Selection::Full),
        [1, 2]
    );
    assert!(evaluate(&limits(), &histories).passed());
}

#[test]
fn a_push_is_judged_against_earlier_pushes_of_its_selection_not_nightly_runs() {
    let mut fake = lab();
    let running = executing(&mut fake, PUSH, "push", 80, "Speed budget (affected)");
    assert_eq!(running.population, Some(Population::MainPush));
    let histories =
        gather(&fake, &repo(), "main", 10, Some(&running)).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        ids(&histories, Population::MainPush, Selection::Affected),
        [PUSH, 3]
    );
    assert!(evaluate(&limits(), &histories).passed());
}
