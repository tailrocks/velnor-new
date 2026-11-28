// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use velnor_ci::runner_drift::{Drift, LATEST_PATH, compare, latest_tag};

use crate::support::FakeGitHub;

fn behind(reported: &str, latest: &str) -> Drift {
    Drift::Behind {
        reported: reported.to_owned(),
        latest: latest.to_owned(),
    }
}

#[test]
fn runner_drift_current_when_the_latest_tag_is_the_reported_version() {
    let found = compare("2.338.2", "v2.338.2").unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found, Drift::Current("2.338.2".to_owned()));
    assert_eq!(found.line(), "runner protocol current: 2.338.2");
    assert!(!found.behind());
}

#[test]
fn runner_drift_behind_when_a_newer_release_exists_and_prints_both_without_v() {
    let found = compare("2.338.2", "v9.9.9").unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found, behind("2.338.2", "9.9.9"));
    assert_eq!(found.line(), "runner protocol behind: 2.338.2 < 9.9.9");
    assert!(found.behind());
}

#[test]
fn runner_drift_compares_integers_not_text() {
    assert_eq!(
        compare("2.338.9", "v2.338.10").unwrap_or_else(|e| unreachable!("{e}")),
        behind("2.338.9", "2.338.10")
    );
    assert!(
        !compare("2.338.10", "v2.338.9")
            .unwrap_or_else(|e| unreachable!("{e}"))
            .behind()
    );
}

#[test]
fn runner_drift_counts_a_newer_reported_build_as_current() {
    assert!(
        !compare("2.339.0", "v2.338.2")
            .unwrap_or_else(|e| unreachable!("{e}"))
            .behind()
    );
}

#[test]
fn runner_drift_refuses_a_tag_that_is_not_three_integers() {
    for tag in [
        "",
        "v",
        "v2.338",
        "v2.338.2.1",
        "v2.338.2-rc1",
        "v2.338.x",
        "v02.338.2",
        "latest",
    ] {
        assert!(compare("2.338.2", tag).is_err(), "{tag:?}");
    }
}

#[test]
fn runner_drift_reads_the_tag_of_the_latest_release() {
    let mut github = FakeGitHub::default();
    github.respond(
        "/repos/actions/runner/releases/latest".to_owned(),
        &json!({"tag_name": "v7.8.9", "prerelease": false}).to_string(),
    );
    assert_eq!(
        latest_tag(&github).unwrap_or_else(|e| unreachable!("{e}")),
        "v7.8.9"
    );
}

#[test]
fn runner_drift_fails_on_an_empty_body_and_on_a_failed_lookup() {
    let mut empty = FakeGitHub::default();
    empty.respond(LATEST_PATH.to_owned(), "");
    assert!(latest_tag(&empty).is_err());
    assert!(latest_tag(&FakeGitHub::default()).is_err());
}
