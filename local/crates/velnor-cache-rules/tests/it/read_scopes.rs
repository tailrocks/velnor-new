// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Event, access_for};

use crate::support::{MAIN, PR7, VERSION, entry, found, pr, push, scopes};

const FEATURE_A: &str = "refs/heads/feature-a";

#[test]
fn rule_1_read_scopes_in_lookup_order() {
    let table: [(&str, Event, &str, &[&str]); 5] = [
        ("push to default", push("main"), "main", &[MAIN]),
        (
            "push to feature",
            push("b"),
            "main",
            &["refs/heads/b", MAIN],
        ),
        ("pr on default", pr(7, "main"), "main", &[PR7, MAIN]),
        (
            "pr on feature",
            pr(7, "feature-a"),
            "main",
            &[PR7, FEATURE_A, MAIN],
        ),
        (
            "other default",
            pr(12, "trunk"),
            "trunk",
            &["refs/pull/12/merge", "refs/heads/trunk"],
        ),
    ];
    for (name, event, default_branch, expected) in table {
        assert_eq!(
            access_for(&event, default_branch).read,
            scopes(expected),
            "{name}"
        );
    }
}

#[test]
fn rule_1_sibling_child_and_other_pull_request_scopes_are_not_readable() {
    let table: [(&str, Event, &str, bool); 10] = [
        ("own branch", push("b"), "refs/heads/b", true),
        ("default branch", push("b"), MAIN, true),
        ("sibling branch", push("b"), "refs/heads/c", false),
        ("child branch", push("main"), "refs/heads/b", false),
        ("a pull request from a branch", push("b"), PR7, false),
        ("own merge ref", pr(7, "main"), PR7, true),
        ("another pull request", pr(8, "main"), PR7, false),
        ("head branch", pr(7, "main"), "refs/heads/b", false),
        ("base branch", pr(7, "feature-a"), FEATURE_A, true),
        (
            "unrelated branch",
            pr(7, "feature-a"),
            "refs/heads/z",
            false,
        ),
    ];
    for (name, event, saved_in, readable) in table {
        let entries = [entry(saved_in, "deps", VERSION, 1)];
        let hit = found(&entries, &event, "deps", &[], VERSION);
        assert_eq!(hit.is_some(), readable, "{name}");
    }
}

#[test]
fn rule_1_nearer_scope_wins_even_when_older() {
    let table: [(&str, &[&str], &str); 3] = [
        ("merge ref", &[MAIN, FEATURE_A, PR7], PR7),
        ("base", &[MAIN, FEATURE_A], FEATURE_A),
        ("default", &[MAIN], MAIN),
    ];
    for (name, saved_in, expected) in table {
        let entries: Vec<_> = (0..)
            .zip(saved_in)
            .map(|(age, scope)| entry(scope, "deps", VERSION, 100 - age))
            .collect();
        let hit = found(&entries, &pr(7, "feature-a"), "deps", &[], VERSION);
        assert_eq!(hit.map(|hit| hit.0).as_deref(), Some(expected), "{name}");
    }
}
