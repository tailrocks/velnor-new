// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Event, Scope};

use crate::support::{VERSION, access, entry, merge_group, restored, scopes, tag};

#[test]
fn rule_6_tags_and_merge_groups_by_event() {
    let table: [(&str, Event, Vec<&str>, &str); 2] = [
        (
            "tag reads its own tag, then the default branch, and writes its tag",
            tag("v1.0.0"),
            vec!["refs/tags/v1.0.0", "refs/heads/main"],
            "refs/tags/v1.0.0",
        ),
        (
            "merge group reads its queue branch, then the default branch, and writes its queue branch",
            merge_group("gh-readonly-queue/main/pr-7-abc123"),
            vec![
                "refs/heads/gh-readonly-queue/main/pr-7-abc123",
                "refs/heads/main",
            ],
            "refs/heads/gh-readonly-queue/main/pr-7-abc123",
        ),
    ];
    for (name, event, read, write) in table {
        let access = access(&event);
        assert_eq!(access.read, scopes(&read), "{name}: read");
        assert_eq!(access.write, Scope(write.to_owned()), "{name}: write");
    }
}

#[test]
fn rule_6_a_run_cannot_restore_the_caches_of_a_different_tag_or_queue_branch() {
    let table: [(&str, Event, &str, bool); 6] = [
        (
            "tag reads its own entry",
            tag("release-a"),
            "refs/tags/release-a",
            true,
        ),
        (
            "tag cannot read another tag",
            tag("release-b"),
            "refs/tags/release-a",
            false,
        ),
        (
            "tag reads the default branch",
            tag("release-b"),
            "refs/heads/main",
            true,
        ),
        (
            "merge group reads the default branch",
            merge_group("gh-readonly-queue/main/pr-7-abc"),
            "refs/heads/main",
            true,
        ),
        (
            "merge group cannot read another queue branch",
            merge_group("gh-readonly-queue/main/pr-8-def"),
            "refs/heads/gh-readonly-queue/main/pr-7-abc",
            false,
        ),
        (
            "merge group cannot read a pull request merge ref",
            merge_group("gh-readonly-queue/main/pr-7-abc"),
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
