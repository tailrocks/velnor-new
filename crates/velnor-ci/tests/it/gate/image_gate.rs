// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Duration;

use serde_json::{Value, json};
use velnor_ci::error::CiError;
use velnor_ci::github::{GitHub, branch_path, runs_path};
use velnor_ci::image_deploy::gate::{Outcome, Wait, wait_for_required};

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
    sleeps: u64,
}

fn wait(first: &Recorded, then: &Recorded, total: u64, tip: bool, after: u64) -> Run {
    let live = Live(RefCell::new(first.0.clone()));
    let slept = Cell::new(0);
    let sleep = |duration: Duration| {
        slept.set(slept.get() + 1);
        if slept.get() >= after {
            *live.0.borrow_mut() = then.0.clone();
        }
        assert_eq!(duration, Duration::from_secs(30));
    };
    let outcome = wait_for_required(
        &live,
        &repo(),
        GREEN,
        tip,
        Wait {
            total: Duration::from_secs(total),
            interval: Duration::from_secs(30),
        },
        &sleep,
        None,
    );
    Run {
        outcome,
        sleeps: slept.get(),
    }
}

#[test]
fn a_running_required_check_is_waited_for_until_it_passes() {
    let green = at_tip(Recorded::new(), GREEN);
    let run = wait(&running(), &green, 2400, true, 2);
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.sleeps, 2);
}

#[test]
fn a_check_that_never_finishes_fails_after_the_wait_and_a_zero_wait_looks_once() {
    let run = wait(&running(), &running(), 60, true, 99);
    let message = run.outcome.err().map(|error| error.to_string());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("not all green after 60 seconds")),
        "{message:?}"
    );
    assert_eq!(run.sleeps, 2);
    let once = wait(&running(), &running(), 0, false, 99);
    assert!(once.outcome.is_err());
    assert_eq!(once.sleeps, 0);
    let ready = wait(&at_tip(Recorded::new(), GREEN), &running(), 0, false, 99);
    assert!(ready.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(ready.sleeps, 0);
}

#[test]
fn a_failed_required_check_is_an_error_naming_it_after_the_short_grace_for_a_run_that_has_not_started()
 {
    let run = wait(&failing(), &failing(), 2400, true, 99);
    let message = run.outcome.err().map(|error| error.to_string());
    assert!(
        message
            .as_deref()
            .is_some_and(|text| text.contains("it concluded failure")),
        "{message:?}"
    );
    assert_eq!(run.sleeps, 3);
}

#[test]
fn a_lookup_that_fails_before_the_checks_exist_is_retried_within_the_grace() {
    let mut early = Recorded::new();
    early.0.remove(&runs_path(&repo(), GREEN, 1));
    let run = wait(
        &at_tip(early, GREEN),
        &at_tip(Recorded::new(), GREEN),
        2400,
        true,
        1,
    );
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.sleeps, 1);
}

#[test]
fn a_commit_that_stopped_being_the_tip_is_superseded_and_never_waits() {
    let behind = at_tip(Recorded::new(), AHEAD);
    let run = wait(&behind, &behind, 2400, true, 99);
    let outcome = run.outcome.ok();
    assert!(outcome.as_ref().is_some_and(|outcome| !outcome.ready));
    assert!(
        outcome
            .as_ref()
            .is_some_and(|outcome| outcome.report.contains("no longer the tip of main"))
    );
    assert_eq!(run.sleeps, 0);
    let without = wait(&behind, &behind, 2400, false, 99);
    assert!(without.outcome.is_ok_and(|outcome| outcome.ready));
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
    let outcome = wait_for_required(
        &github,
        &repo(),
        GREEN,
        true,
        Wait {
            total: Duration::from_secs(60),
            interval: Duration::from_secs(30),
        },
        &|_| {},
        None,
    );
    assert!(outcome.is_ok_and(|outcome| !outcome.ready));
}
