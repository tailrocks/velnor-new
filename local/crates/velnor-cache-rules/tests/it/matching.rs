// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::support::{MAIN, PR7, VERSION, entry, found, pr, push};

type Saved = &'static [(&'static str, u64)];
type Row = (
    &'static str,
    Saved,
    &'static str,
    &'static [&'static str],
    Option<&'static str>,
);

fn matched_on_main(rows: &[Row]) {
    for (name, saved, key, restore_keys, expected) in rows {
        let entries: Vec<_> = saved
            .iter()
            .map(|(key, created)| entry(MAIN, key, VERSION, *created))
            .collect();
        let hit = found(&entries, &push("main"), key, restore_keys, VERSION);
        assert_eq!(hit.map(|hit| hit.1).as_deref(), *expected, "{name}");
    }
}

#[test]
fn rule_3_exact_key_then_key_as_prefix() {
    matched_on_main(&[
        (
            "exact beats a newer longer key",
            &[("d-1", 1), ("d-1-x", 9)],
            "d-1",
            &[],
            Some("d-1"),
        ),
        (
            "prefix takes the newest",
            &[("d-1", 1), ("d-2", 5), ("d-3", 3)],
            "d-",
            &[],
            Some("d-2"),
        ),
        (
            "prefix comes before restore keys",
            &[("n-f-old", 1), ("n-new", 9)],
            "n-f-",
            &["n-"],
            Some("n-f-old"),
        ),
        ("no match", &[("d-1", 1)], "other", &["cargo-"], None),
    ]);
}

#[test]
fn rule_3_restore_keys_in_order_and_newest_wins() {
    matched_on_main(&[
        (
            "in order, not by age",
            &[("n-f-a", 1), ("n-b", 9)],
            "zzz",
            &["n-f-", "n-"],
            Some("n-f-a"),
        ),
        (
            "later key when the first has none",
            &[("n-b", 9)],
            "zzz",
            &["n-f-", "n-"],
            Some("n-b"),
        ),
        (
            "newest of several",
            &[("n-b", 5), ("n-c", 8), ("n-a", 2)],
            "zzz",
            &["n-"],
            Some("n-c"),
        ),
        (
            "only the start of a key",
            &[("linux-n-1", 1)],
            "zzz",
            &["n-"],
            None,
        ),
    ]);
}

#[test]
fn rule_3_version_must_match_exactly() {
    let entries = [
        entry(MAIN, "d-1", "zstd-a", 5),
        entry(MAIN, "d-2", "gzip-b", 9),
    ];
    let table = [
        ("same version", "d-1", "zstd-a", Some("d-1")),
        ("other version", "d-1", "zstd-c", None),
        ("key of another version", "d-2", "zstd-a", None),
        ("prefix within the version", "d-", "zstd-a", Some("d-1")),
    ];
    for (name, key, version, expected) in table {
        let hit = found(&entries, &push("main"), key, &[], version);
        assert_eq!(hit.map(|hit| hit.1).as_deref(), expected, "{name}");
    }
}

#[test]
fn rule_3_each_scope_is_searched_completely_before_the_next() {
    let entries = [
        entry(MAIN, "n-f-d5ea", VERSION, 1),
        entry(PR7, "n-f-0000", VERSION, 2),
    ];
    let hit = found(&entries, &pr(7, "main"), "n-f-d5ea", &["n-f-"], VERSION);
    assert_eq!(hit, Some((PR7.to_owned(), "n-f-0000".to_owned())));
    let hit = found(&entries, &pr(7, "main"), "n-f-d5ea", &[], VERSION);
    assert_eq!(hit, Some((MAIN.to_owned(), "n-f-d5ea".to_owned())));
}
