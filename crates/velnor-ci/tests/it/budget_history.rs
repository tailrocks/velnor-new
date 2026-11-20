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

mod paging;
mod window;

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
