// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use velnor_rust::{CacheEntry, budget_plan};

const DAY: i64 = 86_400;
const NOW: i64 = 20 * DAY;

fn entry(id: u64, key: String, age: i64) -> CacheEntry {
    CacheEntry {
        id,
        key,
        reference: "refs/pull/1/merge".to_owned(),
        size_in_bytes: 10,
        created_at: i64::try_from(id).unwrap_or(0),
        last_accessed_at: NOW - age,
    }
}

fn plan(entries: &[CacheEntry]) -> Vec<u64> {
    budget_plan(entries, &BTreeSet::from([1]), NOW, 0)
}

#[test]
fn fixed_anchor_keys_of_helper_0_1_109_are_pruned_as_one_group() {
    let fixed = |id: u64, age: i64| {
        entry(
            id,
            format!("velnor-tools-anchor-Linux-X64-mise2026.11.10-{id:064x}"),
            age,
        )
    };
    assert_eq!(plan(&[fixed(1, 8 * DAY), fixed(2, 8 * DAY)]), vec![1]);
    assert_eq!(plan(&[fixed(1, DAY), fixed(2, DAY)]), Vec::<u64>::new());
}

#[test]
fn an_anchor_is_ranked_apart_from_the_tools_archive_it_names() {
    let anchor = entry(
        1,
        format!(
            "velnor-tools-anchor-Linux-X64-mise2026.11.10-{}-1-job",
            "a".repeat(64)
        ),
        8 * DAY,
    );
    let archive = entry(
        2,
        format!("velnor-tools-Linux-X64-mise2026.11.10-{:064x}", 2),
        8 * DAY,
    );
    assert_eq!(plan(&[anchor, archive]), Vec::<u64>::new());
}
