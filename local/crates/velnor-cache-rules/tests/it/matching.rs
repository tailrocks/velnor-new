// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::Entry;

use crate::support::{VERSION, entry, pull_request, push, restored};

struct Case {
    name: &'static str,
    entries: Vec<Entry>,
    key: &'static str,
    restore_keys: &'static [&'static str],
    expected: Option<(&'static str, &'static str)>,
}

fn main_entry(key: &str, created: u64) -> Entry {
    entry("refs/heads/main", key, VERSION, created)
}

#[test]
fn rule_3_exact_key_then_key_prefix_within_one_scope() {
    let table = [
        Case {
            name: "exact key beats a newer key that merely starts with it",
            entries: vec![main_entry("deps-1", 1), main_entry("deps-1-extra", 9)],
            key: "deps-1",
            restore_keys: &[],
            expected: Some(("refs/heads/main", "deps-1")),
        },
        Case {
            name: "key as prefix finds the newest entry when there is no exact match",
            entries: vec![
                main_entry("deps-1", 1),
                main_entry("deps-2", 5),
                main_entry("deps-3", 3),
            ],
            key: "deps-",
            restore_keys: &[],
            expected: Some(("refs/heads/main", "deps-2")),
        },
        Case {
            name: "key prefix is tried before restore keys",
            entries: vec![main_entry("npm-feature-old", 1), main_entry("npm-new", 9)],
            key: "npm-feature-",
            restore_keys: &["npm-"],
            expected: Some(("refs/heads/main", "npm-feature-old")),
        },
    ];
    for case in table {
        let hit = restored(
            &case.entries,
            &push("main"),
            case.key,
            case.restore_keys,
            VERSION,
        );
        let expected = case
            .expected
            .map(|(scope, key)| (scope.to_owned(), key.to_owned()));
        assert_eq!(hit, expected, "{}", case.name);
    }
}

#[test]
fn rule_3_restore_keys_in_order_and_newest_wins() {
    let table = [
        Case {
            name: "restore keys are tried in order, not by age",
            entries: vec![main_entry("npm-feature-a", 1), main_entry("npm-b", 9)],
            key: "npm-feature-zzz",
            restore_keys: &["npm-feature-", "npm-"],
            expected: Some(("refs/heads/main", "npm-feature-a")),
        },
        Case {
            name: "a later restore key matches when the first matches nothing",
            entries: vec![main_entry("npm-b", 9)],
            key: "npm-feature-zzz",
            restore_keys: &["npm-feature-", "npm-"],
            expected: Some(("refs/heads/main", "npm-b")),
        },
        Case {
            name: "newest created wins among several matches of one restore key",
            entries: vec![
                main_entry("npm-b", 5),
                main_entry("npm-c", 8),
                main_entry("npm-a", 2),
            ],
            key: "other",
            restore_keys: &["npm-"],
            expected: Some(("refs/heads/main", "npm-c")),
        },
        Case {
            name: "last access does not decide which match is newest",
            entries: vec![
                Entry {
                    last_access: velnor_cache_rules::Timestamp(99),
                    ..main_entry("npm-old", 1)
                },
                main_entry("npm-new", 2),
            ],
            key: "other",
            restore_keys: &["npm-"],
            expected: Some(("refs/heads/main", "npm-new")),
        },
        Case {
            name: "a restore key matches only the start of a key",
            entries: vec![main_entry("linux-npm-1", 1)],
            key: "other",
            restore_keys: &["npm-"],
            expected: None,
        },
        Case {
            name: "no entry matches",
            entries: vec![main_entry("deps-1", 1)],
            key: "other",
            restore_keys: &["cargo-"],
            expected: None,
        },
    ];
    for case in table {
        let hit = restored(
            &case.entries,
            &push("main"),
            case.key,
            case.restore_keys,
            VERSION,
        );
        let expected = case
            .expected
            .map(|(scope, key)| (scope.to_owned(), key.to_owned()));
        assert_eq!(hit, expected, "{}", case.name);
    }
}

#[test]
fn rule_3_version_must_match_exactly() {
    let entries = vec![
        entry("refs/heads/main", "deps-1", "zstd-a", 5),
        entry("refs/heads/main", "deps-2", "gzip-b", 9),
    ];
    let table = [
        (
            "exact key, same version",
            "deps-1",
            "zstd-a",
            Some("deps-1"),
        ),
        ("exact key, other version", "deps-1", "zstd-c", None),
        (
            "exact key exists only in another version",
            "deps-2",
            "zstd-a",
            None,
        ),
        (
            "prefix picks only the entries of the version",
            "deps-",
            "zstd-a",
            Some("deps-1"),
        ),
        ("prefix in a version nothing has", "deps-", "tar-z", None),
    ];
    for (name, key, version, expected) in table {
        let hit = restored(&entries, &push("main"), key, &[], version);
        assert_eq!(hit.map(|hit| hit.1).as_deref(), expected, "{name}");
    }
}

#[test]
fn rule_3_scopes_are_searched_in_order_before_the_next_scope_is_tried() {
    let entries = vec![
        entry("refs/heads/main", "npm-feature-d5ea", VERSION, 1),
        entry("refs/pull/7/merge", "npm-feature-0000", VERSION, 2),
        entry("refs/heads/main", "npm-9", VERSION, 9),
    ];
    let table = [
        (
            "restore-key hit in the merge ref beats an exact hit on the default branch",
            "npm-feature-d5ea",
            vec!["npm-feature-"],
            ("refs/pull/7/merge", "npm-feature-0000"),
        ),
        (
            "default branch answers when the merge ref has no match",
            "npm-9",
            vec![],
            ("refs/heads/main", "npm-9"),
        ),
    ];
    for (name, key, restore_keys, (scope, found)) in table {
        let hit = restored(
            &entries,
            &pull_request(7, "main"),
            key,
            &restore_keys,
            VERSION,
        );
        assert_eq!(hit, Some((scope.to_owned(), found.to_owned())), "{name}");
    }
}
