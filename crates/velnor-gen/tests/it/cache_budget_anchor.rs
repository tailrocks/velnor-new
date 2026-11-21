// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use velnor_rust::{CacheEntry, CacheFamily, budget_plan, cache_family};

const DAY: i64 = 86_400;
const NOW: i64 = 20 * DAY;
const MAIN: &str = "refs/heads/main";

fn anchor_key(host: &str, lock: char, run: u64) -> String {
    format!(
        "velnor-tools-anchor-{host}-mise2026.11.10-{}-{run}-rust-plan",
        lock.to_string().repeat(64)
    )
}

fn anchor(id: u64, reference: &str, key: String) -> CacheEntry {
    CacheEntry {
        id,
        key,
        reference: reference.to_owned(),
        size_in_bytes: 1,
        created_at: i64::try_from(id).unwrap_or(0),
        last_accessed_at: NOW,
    }
}

fn plan(entries: &[CacheEntry], budget: u64) -> Vec<u64> {
    budget_plan(entries, &BTreeSet::new(), NOW, budget)
}

#[test]
fn per_run_anchor_keys_have_their_own_family() {
    assert_eq!(
        cache_family(&anchor_key("Linux-X64", 'a', 7)),
        CacheFamily::ToolsAnchor
    );
    let before_run_suffix = format!(
        "velnor-tools-anchor-macOS-ARM64-mise2026.11.10-{}",
        "a".repeat(64)
    );
    assert_eq!(cache_family(&before_run_suffix), CacheFamily::ToolsAnchor);
    for malformed in [
        format!(
            "velnor-tools-anchor-Linux-X64-mise2026.11.10-{}",
            "a".repeat(63)
        ),
        format!(
            "velnor-tools-anchor-Plan9-X64-mise2026.11.10-{}-7-j",
            "a".repeat(64)
        ),
        format!(
            "velnor-tools-anchor-Linux-X64-mise2026.11.10-{}-x-j",
            "a".repeat(64)
        ),
        format!(
            "velnor-tools-anchor-Linux-X64-mise2026.11.10-{}-7",
            "a".repeat(64)
        ),
        "velnor-tools-anchor-Linux-X64-mise-".to_owned(),
    ] {
        assert_eq!(
            cache_family(&malformed),
            CacheFamily::Unknown,
            "{malformed}"
        );
    }
}

#[test]
fn only_the_newest_two_anchors_of_a_host_and_ref_survive_whatever_the_budget() {
    let entries: Vec<_> = (1..=5)
        .map(|id| anchor(id, MAIN, anchor_key("Linux-X64", 'a', id)))
        .collect();
    assert_eq!(plan(&entries, u64::MAX), [1, 2, 3]);
    assert_eq!(plan(&entries[3..], u64::MAX), Vec::<u64>::new());
}

#[test]
fn anchors_of_older_lock_hashes_and_older_key_shapes_age_out_with_the_rest() {
    let old_shape = |id: u64, lock: char| {
        anchor(
            id,
            MAIN,
            format!(
                "velnor-tools-anchor-Linux-X64-mise2026.11.10-{}",
                lock.to_string().repeat(64)
            ),
        )
    };
    let entries = [
        old_shape(1, 'a'),
        old_shape(2, 'b'),
        anchor(3, MAIN, anchor_key("Linux-X64", 'c', 3)),
        anchor(4, MAIN, anchor_key("Linux-X64", 'd', 4)),
    ];
    assert_eq!(plan(&entries, u64::MAX), [1, 2]);
}

#[test]
fn anchor_generations_are_counted_per_host_and_per_ref() {
    let mut entries = Vec::new();
    for (scope, (host, reference)) in [
        ("Linux-X64", MAIN),
        ("Linux-ARM64", MAIN),
        ("Linux-X64", "refs/heads/release"),
    ]
    .into_iter()
    .enumerate()
    {
        for generation in 1..=3 {
            let id = u64::try_from(scope * 3 + generation).unwrap_or(0);
            entries.push(anchor(id, reference, anchor_key(host, 'a', id)));
        }
    }
    assert_eq!(plan(&entries, u64::MAX), [1, 4, 7]);
}

#[test]
fn anchors_do_not_displace_or_count_as_tools_entries() {
    let tools = |id: u64| CacheEntry {
        key: format!(
            "velnor-tools-Linux-X64-mise2026.11.10-{}-{id:064x}",
            "a".repeat(64)
        ),
        last_accessed_at: NOW - 10 * DAY,
        ..anchor(id, MAIN, String::new())
    };
    let entries = [
        tools(1),
        tools(2),
        anchor(3, MAIN, anchor_key("Linux-X64", 'a', 3)),
        anchor(4, MAIN, anchor_key("Linux-X64", 'a', 4)),
    ];
    assert_eq!(plan(&entries, 0), [1]);
    assert_eq!(plan(&entries, u64::MAX), Vec::<u64>::new());
}

#[test]
fn run_attempt_anchor_keys_are_classified_and_age_out_with_the_per_run_keys() {
    let attempt_key = |run: u64, attempt: u64| {
        format!(
            "velnor-tools-anchor-Linux-X64-mise2026.11.10-{}-{run}-{attempt}-rust-plan",
            "a".repeat(64)
        )
    };
    assert_eq!(cache_family(&attempt_key(7, 2)), CacheFamily::ToolsAnchor);
    let entries = [
        anchor(1, MAIN, anchor_key("Linux-X64", 'a', 1)),
        anchor(2, MAIN, attempt_key(2, 1)),
        anchor(3, MAIN, attempt_key(2, 2)),
        anchor(4, MAIN, attempt_key(3, 1)),
    ];
    assert_eq!(plan(&entries, u64::MAX), [1, 2]);
}
