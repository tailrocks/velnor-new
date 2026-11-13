// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{ENTRY_LIFETIME_SECONDS, Entry, REPOSITORY_LIMIT_BYTES, Timestamp, evict};

use crate::support::{PR7, entry, id, sized};

const NOW: u64 = 10_000_000;
const SEVEN_DAYS: u64 = 604_800;
const TEN_GB: u64 = 10_000_000_000;
const FOUR_GB: u64 = 4_000_000_000;

fn evicted(entries: &[Entry], limit_bytes: u64) -> Vec<String> {
    let ids = evict(entries, Timestamp(NOW), limit_bytes);
    ids.into_iter().map(|id| id.key).collect()
}

#[test]
fn rule_5_an_entry_not_accessed_for_over_seven_days_expires() {
    let table = [
        ("just now", NOW, vec![]),
        ("exactly seven days ago", NOW - SEVEN_DAYS, vec![]),
        (
            "seven days and a second ago",
            NOW - SEVEN_DAYS - 1,
            vec!["a"],
        ),
        ("in the future", NOW + 5, vec![]),
    ];
    for (name, last_access, expected) in table {
        assert_eq!(
            evicted(&[sized("a", 1, last_access)], TEN_GB),
            expected,
            "{name}"
        );
    }
}

#[test]
fn rule_5_the_repository_limit_is_ten_gigabytes() {
    let table = [
        ("below", TEN_GB - 2 * FOUR_GB - 1, vec![]),
        ("at", TEN_GB - 2 * FOUR_GB, vec!["a"]),
        ("over", FOUR_GB, vec!["a"]),
    ];
    for (name, last_size, expected) in table {
        let entries = [
            sized("a", FOUR_GB, NOW - 3),
            sized("b", FOUR_GB, NOW - 2),
            sized("c", last_size, NOW - 1),
        ];
        assert_eq!(evicted(&entries, TEN_GB), expected, "{name} the limit");
    }
    assert_eq!(
        (ENTRY_LIFETIME_SECONDS, REPOSITORY_LIMIT_BYTES),
        (SEVEN_DAYS, TEN_GB)
    );
}

#[test]
fn rule_5_least_recently_used_entries_go_first_until_the_total_fits() {
    let created = |entry: Entry, created| Entry {
        created: Timestamp(created),
        ..entry
    };
    let expired = NOW - SEVEN_DAYS - 5;
    let table = [
        (
            "oldest access first",
            vec![sized("b", 60, 20), sized("a", 60, 10), sized("c", 60, NOW)],
            100,
            vec!["a", "b"],
        ),
        (
            "access, not creation",
            vec![
                created(sized("used", 60, NOW), 1),
                created(sized("idle", 60, NOW - 9), 500),
            ],
            100,
            vec!["idle"],
        ),
        (
            "every scope in one pool",
            vec![entry(PR7, "pr", "v1", NOW - 50), sized("main", 1, NOW - 10)],
            2,
            vec!["pr"],
        ),
        (
            "expired entries free room first",
            vec![
                sized("old", 90, expired),
                sized("b", 60, NOW - 20),
                sized("c", 60, NOW),
            ],
            130,
            vec!["old"],
        ),
        (
            "equal access, older entry first",
            vec![
                created(sized("young", 60, NOW), 9),
                created(sized("old", 60, NOW), 3),
            ],
            100,
            vec!["old"],
        ),
        (
            "limit of zero",
            vec![sized("b", 1, 30), sized("a", 1, 20)],
            0,
            vec!["a", "b"],
        ),
        ("no entries", vec![], 100, vec![]),
    ];
    for (name, entries, limit, expected) in table {
        assert_eq!(evicted(&entries, limit), expected, "{name}");
    }
    let pr_entry = [entry(PR7, "pr", "v1", 1)];
    assert_eq!(
        evict(&pr_entry, Timestamp(NOW), 0),
        vec![id(PR7, "pr", "v1")]
    );
}
