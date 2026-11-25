// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::{Cell, RefCell};
use std::time::Duration;

use serde_json::json;
use velnor_ci::error::CiError;
use velnor_ci::image_deploy::gate::{Clock, Request, Wait, exemptions_only};

use super::GREEN;
use super::image_gate::{Flaky, Live, at_tip, drive_on, tip};
use super::pr_only::{CHECK_RULESET, check_ruleset, protected, suites_path};
use crate::support::repo;

const DCO: &str = "DCO";

fn request(pr_only: &[String], total: u64, verify: bool) -> Request<'_> {
    Request {
        sha: GREEN,
        require_tip: false,
        wait: Wait {
            total: Duration::from_secs(total),
            interval: Duration::from_secs(30),
        },
        run_id: None,
        pr_only_checks: pr_only,
        verify_exemptions: verify,
        ignore_jobs: &[],
        require_published: None,
    }
}

fn pending_suite() -> super::support::Recorded {
    at_tip(protected(), GREEN).set(
        suites_path(1),
        json!({"total_count": 1, "check_suites": [{"id": 9, "status": "in_progress", "app": {"id": 343_649}}]}),
    )
}

fn without_bypass_list() -> super::support::Recorded {
    let mut detail = check_ruleset();
    detail
        .as_object_mut()
        .map(|fields| fields.remove("bypass_actors"));
    let mut pull_request = super::pr_only::ruleset();
    pull_request
        .as_object_mut()
        .map(|fields| fields.remove("bypass_actors"));
    at_tip(protected(), GREEN)
        .set(CHECK_RULESET.to_owned(), detail)
        .set(super::pr_only::RULESET.to_owned(), pull_request)
}

#[test]
fn an_unfinished_suite_of_another_app_makes_the_gate_wait_and_never_fails_it() {
    let names = [DCO.to_owned()];
    let live = Live(RefCell::new(pending_suite().0));
    let run = drive_on(
        &live,
        &request(&names, 600, false),
        &|count| {
            if count >= 3 {
                *live.0.borrow_mut() = at_tip(protected(), GREEN).0;
            }
        },
        &Cell::new(Duration::ZERO),
    );
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.pauses.len(), 3);
    let stuck = Live(RefCell::new(pending_suite().0));
    let run = drive_on(
        &stuck,
        &request(&names, 100, false),
        &|_| {},
        &Cell::new(Duration::ZERO),
    );
    let message = run
        .outcome
        .err()
        .map_or_else(String::new, |error| error.to_string());
    assert!(
        message.contains("not all green after 100 seconds"),
        "{message}"
    );
    assert!(message.contains("unfinished check suite 9"), "{message}");
}

#[test]
fn without_the_admin_token_the_exemptions_are_trusted_and_with_it_they_are_verified() {
    let names = [DCO.to_owned()];
    let hidden = Live(RefCell::new(without_bypass_list().0));
    let trusted = drive_on(
        &hidden,
        &request(&names, 0, false),
        &|_| {},
        &Cell::new(Duration::ZERO),
    );
    assert!(trusted.outcome.is_ok_and(|outcome| outcome.ready));
    let verified = drive_on(
        &hidden,
        &request(&names, 0, true),
        &|_| {},
        &Cell::new(Duration::ZERO),
    );
    let message = verified
        .outcome
        .err()
        .map_or_else(String::new, |error| error.to_string());
    assert!(message.contains("bypass_actors"), "{message}");
}

#[test]
fn the_one_shot_verification_accepts_a_non_bypassable_ruleset_and_refuses_a_hidden_one() {
    let names = [DCO.to_owned()];
    let clock = |elapsed: &Cell<Duration>| {
        let now = || elapsed.get();
        let sleep = |duration: Duration| elapsed.set(elapsed.get() + duration);
        exemptions_only(
            &Live(RefCell::new(protected().0)),
            &repo(),
            &names,
            &Clock {
                now: &now,
                sleep: &sleep,
            },
        )
    };
    assert!(clock(&Cell::new(Duration::ZERO)).is_ok_and(|outcome| outcome.ready));
    let elapsed = Cell::new(Duration::ZERO);
    let now = || elapsed.get();
    let sleep = |duration: Duration| elapsed.set(elapsed.get() + duration);
    let hidden = exemptions_only(
        &Live(RefCell::new(without_bypass_list().0)),
        &repo(),
        &names,
        &Clock {
            now: &now,
            sleep: &sleep,
        },
    );
    assert!(hidden.is_err());
}

#[test]
fn a_zero_wait_gate_and_the_tip_check_still_retry_transient_errors_for_a_minute() {
    let live = Live(RefCell::new(
        at_tip(super::support::Recorded::new(), GREEN).0,
    ));
    let failures = Cell::new(2);
    let github = Flaky {
        live: &live,
        failures: &failures,
        error: CiError::transient("http status: 503"),
    };
    let run = drive_on(
        &github,
        &request(&[], 0, false),
        &|_| {},
        &Cell::new(Duration::ZERO),
    );
    assert!(run.outcome.is_ok_and(|outcome| outcome.ready));
    assert_eq!(run.pauses.len(), 2);
    let always = Cell::new(u32::MAX);
    let github = Flaky {
        live: &live,
        failures: &always,
        error: CiError::transient("http status: 503"),
    };
    let run = drive_on(
        &github,
        &request(&[], 0, false),
        &|_| {},
        &Cell::new(Duration::ZERO),
    );
    assert!(run.outcome.err().is_some_and(|error| error.is_transient()));
    assert!(run.pauses.iter().sum::<u64>() >= 55);
    let few = Cell::new(3);
    let flaky = Flaky {
        live: &live,
        failures: &few,
        error: CiError::transient("http status: 502"),
    };
    assert!(tip(&flaky).is_ok_and(|outcome| outcome.ready));
}
