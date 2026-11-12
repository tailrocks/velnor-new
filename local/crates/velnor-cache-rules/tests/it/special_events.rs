// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Event, Scope};

use crate::support::{MAIN, PR7, VERSION, access, entry, found, queue, scopes, tag};

const QUEUE_7: &str = "refs/heads/gh-readonly-queue/main/pr-7-abc";

#[test]
fn rule_6_tags_and_merge_groups_by_event() {
    let table: [(&str, Event, &[&str], &str); 2] = [
        (
            "tag",
            tag("v1.0.0"),
            &["refs/tags/v1.0.0", MAIN],
            "refs/tags/v1.0.0",
        ),
        (
            "merge group",
            queue("gh-readonly-queue/main/pr-7-abc"),
            &[QUEUE_7, MAIN],
            QUEUE_7,
        ),
    ];
    for (name, event, read, write) in table {
        let access = access(&event);
        assert_eq!(access.read, scopes(read), "{name}: read");
        assert_eq!(access.write, Scope(write.to_owned()), "{name}: write");
    }
}

#[test]
fn rule_6_another_tag_or_queue_branch_is_not_readable() {
    let table: [(&str, Event, &str, bool); 6] = [
        ("own tag", tag("a"), "refs/tags/a", true),
        ("another tag", tag("b"), "refs/tags/a", false),
        ("tag reads default", tag("b"), MAIN, true),
        (
            "queue reads default",
            queue("gh-readonly-queue/main/pr-8-def"),
            MAIN,
            true,
        ),
        (
            "another queue branch",
            queue("gh-readonly-queue/main/pr-8-def"),
            QUEUE_7,
            false,
        ),
        (
            "a pull request merge ref",
            queue("gh-readonly-queue/main/pr-8-def"),
            PR7,
            false,
        ),
    ];
    for (name, event, saved_in, readable) in table {
        let entries = [entry(saved_in, "deps", VERSION, 1)];
        let hit = found(&entries, &event, "deps", &[], VERSION);
        assert_eq!(hit.is_some(), readable, "{name}");
    }
}
