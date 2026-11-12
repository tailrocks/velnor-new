// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Event, access_for};

use crate::support::{VERSION, entry, pull_request, push, restored, scopes};

#[test]
fn rule_1_read_scopes_in_lookup_order() {
    let table: [(&str, Event, &str, Vec<&str>); 6] = [
        (
            "push to the default branch reads only itself",
            push("main"),
            "main",
            vec!["refs/heads/main"],
        ),
        (
            "push to a feature branch reads itself, then the default branch",
            push("feature-b"),
            "main",
            vec!["refs/heads/feature-b", "refs/heads/main"],
        ),
        (
            "pull request against the default branch reads its merge ref, then the default branch",
            pull_request(7, "main"),
            "main",
            vec!["refs/pull/7/merge", "refs/heads/main"],
        ),
        (
            "pull request against a feature branch reads its merge ref, the base branch, then the default branch",
            pull_request(7, "feature-a"),
            "main",
            vec![
                "refs/pull/7/merge",
                "refs/heads/feature-a",
                "refs/heads/main",
            ],
        ),
        (
            "push with another default branch name",
            push("feature-b"),
            "trunk",
            vec!["refs/heads/feature-b", "refs/heads/trunk"],
        ),
        (
            "pull request against the default branch named trunk",
            pull_request(12, "trunk"),
            "trunk",
            vec!["refs/pull/12/merge", "refs/heads/trunk"],
        ),
    ];
    for (name, event, default_branch, expected) in table {
        assert_eq!(
            access_for(&event, default_branch).read,
            scopes(&expected),
            "{name}"
        );
    }
}

#[test]
fn rule_1_cannot_read_sibling_child_or_other_pull_request_scopes() {
    let table: [(&str, Event, &str, bool); 11] = [
        (
            "own branch",
            push("feature-b"),
            "refs/heads/feature-b",
            true,
        ),
        ("default branch", push("feature-b"), "refs/heads/main", true),
        (
            "sibling branch",
            push("feature-b"),
            "refs/heads/feature-c",
            false,
        ),
        (
            "child branch from the default branch",
            push("main"),
            "refs/heads/feature-b",
            false,
        ),
        (
            "pull request merge ref from a branch",
            push("feature-b"),
            "refs/pull/7/merge",
            false,
        ),
        (
            "own merge ref",
            pull_request(7, "main"),
            "refs/pull/7/merge",
            true,
        ),
        (
            "another pull request",
            pull_request(8, "main"),
            "refs/pull/7/merge",
            false,
        ),
        (
            "head branch of the pull request",
            pull_request(7, "main"),
            "refs/heads/feature-b",
            false,
        ),
        (
            "base branch of the pull request",
            pull_request(7, "feature-a"),
            "refs/heads/feature-a",
            true,
        ),
        (
            "another feature branch",
            pull_request(7, "feature-a"),
            "refs/heads/feature-z",
            false,
        ),
        (
            "pull request merge ref from the default branch",
            push("main"),
            "refs/pull/7/merge",
            false,
        ),
    ];
    for (name, event, saved_in, visible) in table {
        let entries = [entry(saved_in, "deps", VERSION, 1)];
        assert_eq!(
            restored(&entries, &event, "deps", &[], VERSION).is_some(),
            visible,
            "{name}"
        );
    }
}

#[test]
fn rule_1_nearer_scope_wins_over_farther_scope() {
    let table: [(&str, Vec<&str>, &str); 3] = [
        (
            "merge ref before base and default",
            vec![
                "refs/heads/main",
                "refs/heads/feature-a",
                "refs/pull/7/merge",
            ],
            "refs/pull/7/merge",
        ),
        (
            "base before default",
            vec!["refs/heads/main", "refs/heads/feature-a"],
            "refs/heads/feature-a",
        ),
        ("default alone", vec!["refs/heads/main"], "refs/heads/main"),
    ];
    for (name, saved_in, expected) in table {
        let entries: Vec<_> = saved_in
            .iter()
            .enumerate()
            .map(|(created, scope)| entry(scope, "deps", VERSION, 100 - created as u64))
            .collect();
        let hit = restored(
            &entries,
            &pull_request(7, "feature-a"),
            "deps",
            &[],
            VERSION,
        );
        assert_eq!(hit.map(|hit| hit.0).as_deref(), Some(expected), "{name}");
    }
}
