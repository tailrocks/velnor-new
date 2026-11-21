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

mod limits;
mod runs;

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
