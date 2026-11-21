// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

use velnor_ci::app_token::mint;
use velnor_ci::error::CiError;
use velnor_ci::github::GitHub;
use velnor_ci::release_family::GitHubPost;

use super::{MINTED, minted_answer};

type Answer = Result<String, CiError>;

struct Scripted {
    lookups: RefCell<VecDeque<Answer>>,
    mints: RefCell<VecDeque<Answer>>,
    calls: RefCell<(usize, usize)>,
}

impl Scripted {
    fn new(lookups: Vec<Answer>, mints: Vec<Answer>) -> Self {
        Self {
            lookups: RefCell::new(lookups.into()),
            mints: RefCell::new(mints.into()),
            calls: RefCell::new((0, 0)),
        }
    }

    fn calls(&self) -> (usize, usize) {
        *self.calls.borrow()
    }
}

impl GitHub for Scripted {
    fn get(&self, _path: &str) -> Answer {
        self.calls.borrow_mut().0 += 1;
        self.lookups
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(CiError::new("no more scripted lookups")))
    }
}

impl GitHubPost for Scripted {
    fn post(&self, _path: &str, _json: &str) -> Answer {
        self.calls.borrow_mut().1 += 1;
        self.mints
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(CiError::new("no more scripted mints")))
    }
}

const FOUND: &str = r#"{"id":4242}"#;

fn bad_gateway() -> Answer {
    Err(CiError::transient("POST /x: http status: 502"))
}

fn rejected() -> Answer {
    Err(CiError::new("GET /x: http status: 401"))
}

fn run(app: &Scripted) -> (Result<String, CiError>, Vec<Duration>) {
    let pauses = RefCell::new(Vec::new());
    let minted = mint(app, "acme", &["shared-lib".to_owned()], &|d| {
        pauses.borrow_mut().push(d);
    });
    (minted.map(|token| token.to_string()), pauses.into_inner())
}

#[test]
fn a_transient_lookup_and_a_transient_mint_are_retried_with_a_bounded_pause() {
    let app = Scripted::new(
        vec![bad_gateway(), Ok(FOUND.to_owned())],
        vec![bad_gateway(), bad_gateway(), Ok(minted_answer())],
    );
    let (minted, pauses) = run(&app);
    assert_eq!(minted.as_deref(), Ok(MINTED));
    assert_eq!(app.calls(), (2, 3));
    assert_eq!(pauses.len(), 3);
    assert!(
        pauses
            .iter()
            .all(|p| *p > Duration::ZERO && *p <= Duration::from_secs(5))
    );
}

#[test]
fn a_transient_failure_stops_after_three_attempts() {
    let app = Scripted::new(
        vec![Ok(FOUND.to_owned())],
        vec![
            bad_gateway(),
            bad_gateway(),
            bad_gateway(),
            Ok(minted_answer()),
        ],
    );
    let (minted, pauses) = run(&app);
    assert!(minted.is_err());
    assert_eq!(app.calls(), (1, 3));
    assert_eq!(pauses.len(), 2);
    let app = Scripted::new(
        vec![
            bad_gateway(),
            bad_gateway(),
            bad_gateway(),
            Ok(FOUND.to_owned()),
        ],
        vec![],
    );
    assert!(run(&app).0.is_err());
    assert_eq!(app.calls(), (3, 0));
}

#[test]
fn a_client_error_fails_at_once_without_a_retry_or_a_pause() {
    let app = Scripted::new(vec![rejected(), Ok(FOUND.to_owned())], vec![]);
    let (minted, pauses) = run(&app);
    assert!(minted.is_err());
    assert_eq!(app.calls(), (1, 0));
    assert_eq!(pauses.len(), 0);
    let app = Scripted::new(
        vec![Ok(FOUND.to_owned())],
        vec![rejected(), Ok(minted_answer())],
    );
    let (minted, pauses) = run(&app);
    assert!(minted.is_err());
    assert_eq!(app.calls(), (1, 1));
    assert_eq!(pauses.len(), 0);
}

#[test]
fn a_token_over_the_length_cap_is_refused_and_one_at_the_cap_is_taken() {
    let answer = |len: usize| {
        Ok(serde_json::json!({ "token": format!("ghs_{}", "a".repeat(len - 4)) }).to_string())
    };
    let app = Scripted::new(
        vec![Ok(FOUND.to_owned()), Ok(FOUND.to_owned())],
        vec![answer(4096), answer(4097)],
    );
    let (minted, _) = run(&app);
    assert_eq!(minted.map(|token| token.len()), Ok(4096));
    let (minted, _) = run(&app);
    assert!(minted.is_err());
}
