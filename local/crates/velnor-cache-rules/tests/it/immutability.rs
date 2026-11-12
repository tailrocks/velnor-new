// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{AlreadyExists, Event, reserve};

use crate::support::{VERSION, access, entry, id, pull_request, push};

type Reserved = Result<(), AlreadyExists>;

#[test]
fn rule_4_same_key_and_version_in_the_same_scope_cannot_be_created_again() {
    let entries = vec![
        entry("refs/heads/main", "deps-1", VERSION, 1),
        entry("refs/pull/7/merge", "deps-2", VERSION, 2),
    ];
    let table: [(&str, Event, &str, &str, Reserved); 8] = [
        (
            "push saving an existing key",
            push("main"),
            "deps-1",
            VERSION,
            Err(AlreadyExists(id("refs/heads/main", "deps-1", VERSION))),
        ),
        (
            "pull request saving the key of its own earlier save",
            pull_request(7, "main"),
            "deps-2",
            VERSION,
            Err(AlreadyExists(id("refs/pull/7/merge", "deps-2", VERSION))),
        ),
        (
            "same key, other version",
            push("main"),
            "deps-1",
            "zstd-b",
            Ok(()),
        ),
        (
            "same key and version in another scope",
            push("feature-b"),
            "deps-1",
            VERSION,
            Ok(()),
        ),
        (
            "pull request saving a key that exists on the default branch",
            pull_request(7, "main"),
            "deps-1",
            VERSION,
            Ok(()),
        ),
        (
            "another pull request saving the key of pull request 7",
            pull_request(8, "main"),
            "deps-2",
            VERSION,
            Ok(()),
        ),
        ("a new key", push("main"), "deps-3", VERSION, Ok(())),
        (
            "a key that is only a prefix of an existing key",
            push("main"),
            "deps",
            VERSION,
            Ok(()),
        ),
    ];
    for (name, event, key, version, expected) in table {
        assert_eq!(
            reserve(&entries, &access(&event), key, version),
            expected,
            "{name}"
        );
    }
}
