// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, branch_path, runs_path};
use velnor_ci::image_deploy::gate::{Clock, Outcome, Request, Wait, tip_only, wait_for_required};

use super::support::Recorded;
use super::{AHEAD, GREEN};
use crate::support::repo;

struct Live(RefCell<BTreeMap<String, Value>>);

impl GitHub for Live {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.0
            .borrow()
            .get(path)
            .map(Value::to_string)
            .ok_or_else(|| CiError::new(format!("no recorded response for {path}")))
    }
}

struct Moving<'a> {
    live: &'a Live,
    later: &'a BTreeMap<String, Value>,
    reads: &'a Cell<u32>,
}

impl GitHub for Moving<'_> {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if path == branch_path(&repo(), "main") {
            self.reads.set(self.reads.get() + 1);
            if self.reads.get() >= 2 {
                return Ok(self.later[path].to_string());
            }
        }
        self.live.get(path)
    }
}

struct Flaky<'a> {
    live: &'a Live,
    failures: &'a Cell<u32>,
    error: CiError,
}

impl GitHub for Flaky<'_> {
    fn get(&self, path: &str) -> Result<String, CiError> {
        if self.failures.get() > 0 {
            self.failures.set(self.failures.get() - 1);
            return Err(self.error.clone());
        }
        self.live.get(path)
    }
}

fn at_tip(recorded: Recorded, sha: &str) -> Recorded {
    recorded.edit(&branch_path(&repo(), "main"), |branch| {
        branch["commit"] = json!({"sha": sha});
    })
}

fn running() -> Recorded {
    at_tip(Recorded::new(), GREEN)
        .green_run(|run| run["status"] = json!("in_progress"))
        .green_job("Required", |job| {
            job["status"] = json!("in_progress");
            job["conclusion"] = Value::Null;
        })
}

fn failing() -> Recorded {
    at_tip(Recorded::new(), GREEN).green_job("Required", |job| job["conclusion"] = json!("failure"))
}

struct Run {
    outcome: Result<Outcome, CiError>,
    pauses: Vec<u64>,
}

fn request(total: u64, tip: bool, pr_only: &[String]) -> Request<'_> {
    Request {
        sha: GREEN,
        require_tip: tip,
        wait: Wait {
            total: Duration::from_secs(total),
            interval: Duration::from_secs(30),
        },
        run_id: None,
        pr_only_checks: pr_only,
    }
}

fn drive(github: &dyn GitHub, request: &Request<'_>, on_pause: &dyn Fn(usize)) -> Run {
    let elapsed = Cell::new(Duration::ZERO);
    let pauses = RefCell::new(Vec::new());
    let now = || elapsed.get();
    let sleep = |duration: Duration| {
        elapsed.set(elapsed.get() + duration);
        pauses.borrow_mut().push(duration.as_secs());
        on_pause(pauses.borrow().len());
    };
    let outcome = wait_for_required(
        github,
        &repo(),
        request,
        &Clock {
            now: &now,
            sleep: &sleep,
        },
    );
    Run {
        outcome,
        pauses: pauses.into_inner(),
    }
}

fn wait(first: &Recorded, then: &Recorded, request: &Request<'_>, after: usize) -> Run {
    let live = Live(RefCell::new(first.0.clone()));
    drive(&live, request, &|count| {
        if count >= after {
            *live.0.borrow_mut() = then.0.clone();
        }
    })
}

#[test]
fn a_running_required_check_is_waited_for_until_it_passes_and_the_pause_backs_off() {
    let green = at_tip(Recorded::new(), GREEN);
    let run = wait(&running(), &green, &request(2400, true, &[]), 4);
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.pauses, [30, 45, 67, 101]);
    let long = wait(&running(), &green, &request(2400, true, &[]), 9);
    assert_eq!(long.pauses[4..], [120, 120, 120, 120, 120]);
}

#[test]
fn a_check_that_never_finishes_fails_after_the_wall_clock_wait_and_a_zero_wait_looks_once() {
    let run = wait(&running(), &running(), &request(60, true, &[]), 99);
    let message = run.outcome.err().map(|error| error.to_string());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("not all green after 60 seconds")),
        "{message:?}"
    );
    assert_eq!(run.pauses, [30, 45]);
    let once = wait(&running(), &running(), &request(0, false, &[]), 99);
    assert!(once.outcome.is_err());
    assert_eq!(once.pauses, Vec::<u64>::new());
    let ready = wait(
        &at_tip(Recorded::new(), GREEN),
        &running(),
        &request(0, false, &[]),
        99,
    );
    assert!(ready.outcome.is_ok_and(|outcome| outcome.ready));
}

#[test]
fn a_failed_required_check_is_an_error_naming_it_after_the_short_grace() {
    let run = wait(&failing(), &failing(), &request(2400, true, &[]), 99);
    let message = run.outcome.err().map(|error| error.to_string());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("it concluded failure")),
        "{message:?}"
    );
    assert_eq!(run.pauses, [30, 45, 67]);
}

#[test]
fn a_lookup_that_fails_before_the_checks_exist_is_retried_within_the_grace() {
    let mut early = Recorded::new();
    early.0.remove(&runs_path(&repo(), GREEN, 1));
    let run = wait(
        &at_tip(early, GREEN),
        &at_tip(Recorded::new(), GREEN),
        &request(2400, true, &[]),
        1,
    );
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.pauses, [30]);
}

#[test]
fn transient_errors_and_rate_limits_are_retried_for_the_whole_wait_not_just_the_grace() {
    let live = Live(RefCell::new(at_tip(Recorded::new(), GREEN).0));
    let failures = Cell::new(12);
    let github = Flaky {
        live: &live,
        failures: &failures,
        error: CiError::transient("http status: 429"),
    };
    let run = drive(&github, &request(2400, true, &[]), &|_| {});
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.pauses.len(), 12);
    let permanent = Cell::new(12);
    let github = Flaky {
        live: &live,
        failures: &permanent,
        error: CiError::new("http status: 403"),
    };
    let run = drive(&github, &request(2400, true, &[]), &|_| {});
    assert!(run.outcome.is_err());
    assert_eq!(run.pauses.len(), 3);
    let always = Cell::new(u32::MAX);
    let github = Flaky {
        live: &live,
        failures: &always,
        error: CiError::transient("http status: 503"),
    };
    let run = drive(&github, &request(300, true, &[]), &|_| {});
    assert!(run.outcome.err().is_some_and(|error| error.is_transient()));
}

#[test]
fn a_commit_that_stopped_being_the_tip_is_superseded_and_never_waits() {
    let behind = at_tip(Recorded::new(), AHEAD);
    let run = wait(&behind, &behind, &request(2400, true, &[]), 99);
    let outcome = run.outcome.ok();
    assert!(outcome.as_ref().is_some_and(|outcome| !outcome.ready));
    assert!(
        outcome
            .as_ref()
            .is_some_and(|outcome| outcome.report.contains("no longer the tip of main"))
    );
    assert_eq!(run.pauses, Vec::<u64>::new());
    let without = wait(&behind, &behind, &request(2400, false, &[]), 99);
    assert!(without.outcome.is_ok_and(|outcome| outcome.ready));
}

#[test]
fn a_commit_superseded_while_the_gate_was_reading_is_not_published() {
    let green = at_tip(Recorded::new(), GREEN);
    let behind = at_tip(Recorded::new(), AHEAD);
    let live = Live(RefCell::new(green.0.clone()));
    let reads = Cell::new(0);
    let github = Moving {
        live: &live,
        later: &behind.0,
        reads: &reads,
    };
    let run = drive(&github, &request(60, true, &[]), &|_| {});
    assert!(run.outcome.is_ok_and(|outcome| !outcome.ready));
}

#[test]
fn the_tip_check_alone_reads_no_check_and_answers_for_the_publish_job() {
    let at = Live(RefCell::new(at_tip(Recorded::new(), GREEN).0));
    assert!(tip_only(&at, &repo(), GREEN).is_ok_and(|outcome| outcome.ready));
    let behind = Live(RefCell::new(at_tip(Recorded::new(), AHEAD).0));
    let outcome = tip_only(&behind, &repo(), GREEN);
    assert!(outcome.is_ok_and(|outcome| !outcome.ready && outcome.report.contains("no longer")));
    let mut bare = at_tip(Recorded::new(), GREEN).0;
    bare.retain(|path, _| !path.contains("/actions/") && !path.contains("/rules/"));
    assert!(tip_only(&Live(RefCell::new(bare)), &repo(), GREEN).is_ok());
}

#[test]
fn the_pr_only_checks_reach_the_release_gate() {
    let green = at_tip(Recorded::new(), GREEN);
    let blank = [String::new()];
    let run = wait(&green, &green, &request(0, false, &blank), 99);
    let message = run.outcome.err().map(|error| error.to_string());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("--pr-only-check #1 is blank")),
        "{message:?}"
    );
}
