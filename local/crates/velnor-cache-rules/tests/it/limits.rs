// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{ENTRY_LIFETIME_SECONDS, Entry, REPOSITORY_LIMIT_BYTES, Timestamp, evict};

use crate::support::{id, sized};

const NOW: u64 = 10_000_000;
const MAIN: &str = "refs/heads/main";
const FOUR_GB: u64 = 4_000_000_000;
const SEVEN_DAYS: u64 = 604_800;
const TEN_GB: u64 = 10_000_000_000;

fn evicted(entries: &[Entry], limit_bytes: u64) -> Vec<String> {
    evict(entries, Timestamp(NOW), limit_bytes)
        .into_iter()
        .map(|entry| entry.key)
        .collect()
}

#[test]
fn rule_5_an_entry_not_accessed_for_over_seven_days_expires() {
    let table = [
        ("accessed just now", NOW, vec![]),
        ("accessed exactly seven days ago", NOW - SEVEN_DAYS, vec![]),
        (
            "accessed seven days and one second ago",
            NOW - SEVEN_DAYS - 1,
            vec!["a"],
        ),
        ("accessed in the future", NOW + 5, vec![]),
    ];
    for (name, last_access, expected) in table {
        let entries = [sized(MAIN, "a", 1, last_access)];
        assert_eq!(evicted(&entries, TEN_GB), expected, "{name}");
    }
}

#[test]
fn rule_5_the_repository_limit_is_ten_gigabytes() {
    let at_limit = [
        sized(MAIN, "a", FOUR_GB, NOW - 3),
        sized(MAIN, "b", FOUR_GB, NOW - 2),
        sized(MAIN, "c", TEN_GB - 2 * FOUR_GB, NOW - 1),
    ];
    assert_eq!(evicted(&at_limit, TEN_GB), Vec::<String>::new());
    let over_limit = [
        sized(MAIN, "a", FOUR_GB, NOW - 3),
        sized(MAIN, "b", FOUR_GB, NOW - 2),
        sized(MAIN, "c", FOUR_GB, NOW - 1),
    ];
    assert_eq!(evicted(&over_limit, TEN_GB), vec!["a"]);
}

#[test]
fn rule_5_least_recently_used_entries_are_evicted_first_until_the_total_fits() {
    let table = [
        (
            "oldest access goes first, then the next until the total fits",
            vec![
                sized(MAIN, "b", 60, 20),
                sized(MAIN, "a", 60, 10),
                sized(MAIN, "c", 60, NOW - 1),
            ],
            100,
            vec!["a", "b"],
        ),
        (
            "last access decides, not creation",
            vec![
                Entry {
                    created: Timestamp(1),
                    ..sized(MAIN, "recent-use", 60, NOW - 1)
                },
                Entry {
                    created: Timestamp(500),
                    ..sized(MAIN, "stale-use", 60, NOW - 100)
                },
            ],
            100,
            vec!["stale-use"],
        ),
        (
            "entries of every scope compete in one pool",
            vec![
                sized("refs/pull/7/merge", "pr", 60, NOW - 50),
                sized(MAIN, "main", 60, NOW - 10),
            ],
            100,
            vec!["pr"],
        ),
        (
            "expired entries leave first and count toward the room they free",
            vec![
                sized(MAIN, "expired", 90, NOW - SEVEN_DAYS - 5),
                sized(MAIN, "b", 60, NOW - 20),
                sized(MAIN, "c", 60, NOW - 10),
            ],
            130,
            vec!["expired"],
        ),
        (
            "equal access times fall back to the older entry",
            vec![
                Entry {
                    created: Timestamp(9),
                    ..sized(MAIN, "younger", 60, NOW - 5)
                },
                Entry {
                    created: Timestamp(3),
                    ..sized(MAIN, "older", 60, NOW - 5)
                },
            ],
            100,
            vec!["older"],
        ),
        ("no entries", vec![], 100, vec![]),
        (
            "a limit of zero evicts everything, oldest first",
            vec![sized(MAIN, "b", 1, 30), sized(MAIN, "a", 1, 20)],
            0,
            vec!["a", "b"],
        ),
    ];
    for (name, entries, limit, expected) in table {
        assert_eq!(evicted(&entries, limit), expected, "{name}");
    }
}

#[test]
fn rule_5_eviction_names_the_entry_by_scope_key_and_version() {
    let entries = [sized("refs/pull/7/merge", "pr", 10, 1)];
    assert_eq!(
        evict(&entries, Timestamp(NOW), 0),
        vec![id("refs/pull/7/merge", "pr", "v1")]
    );
}

#[test]
fn rule_5_the_exported_limits_are_seven_days_and_ten_gigabytes() {
    assert_eq!(ENTRY_LIFETIME_SECONDS, SEVEN_DAYS);
    assert_eq!(REPOSITORY_LIMIT_BYTES, TEN_GB);
}
