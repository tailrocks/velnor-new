// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Event, Scope};

use crate::support::{MAIN, PR7, access, pr, push};

#[test]
fn rule_2_write_scope_by_event() {
    let table: [(&str, Event, &str); 4] = [
        ("push to default", push("main"), MAIN),
        (
            "push to feature",
            push("feature/login"),
            "refs/heads/feature/login",
        ),
        ("pr on default", pr(7, "main"), PR7),
        (
            "pr on feature, never the base",
            pr(42, "feature-a"),
            "refs/pull/42/merge",
        ),
    ];
    for (name, event, expected) in table {
        assert_eq!(access(&event).write, Scope(expected.to_owned()), "{name}");
    }
}
