// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Event, Scope};

use crate::support::{access, pull_request, push};

#[test]
fn rule_2_write_scope_by_event() {
    let table: [(&str, Event, &str); 5] = [
        (
            "push to the default branch",
            push("main"),
            "refs/heads/main",
        ),
        (
            "push to a feature branch",
            push("feature/login"),
            "refs/heads/feature/login",
        ),
        (
            "pull request against the default branch",
            pull_request(7, "main"),
            "refs/pull/7/merge",
        ),
        (
            "pull request against a feature branch",
            pull_request(42, "feature-a"),
            "refs/pull/42/merge",
        ),
        (
            "another pull request has another scope",
            pull_request(8, "main"),
            "refs/pull/8/merge",
        ),
    ];
    for (name, event, expected) in table {
        assert_eq!(access(&event).write, Scope(expected.to_owned()), "{name}");
    }
}
