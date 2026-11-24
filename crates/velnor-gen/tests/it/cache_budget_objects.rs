// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use velnor_rust::{CacheEntry, CacheFamily, budget_plan, cache_family};

const DAY: i64 = 86_400;
const NOW: i64 = 20 * DAY;
const MAIN: &str = "refs/heads/main";
const PR_7: &str = "refs/pull/7/merge";
const PR_8: &str = "refs/pull/8/merge";
const RUST_A: &str = "rust-0123456789ab";
const RUST_B: &str = "rust-ba9876543210";

fn push_key(job: &str, toolchain: &str, commit: char) -> String {
    format!(
        "linux-x64-mbx-v1-{job}-dir-{toolchain}-{}",
        commit.to_string().repeat(40)
    )
}

fn pull_key(job: &str, toolchain: &str, commit: char, run: u64) -> String {
    format!("{}-run-{run}-1", push_key(job, toolchain, commit))
}

fn entry(id: u64, key: String, reference: &str, size: u64) -> CacheEntry {
    CacheEntry {
        id,
        key,
        reference: reference.to_owned(),
        size_in_bytes: size,
        created_at: i64::try_from(id).unwrap_or(0),
        last_accessed_at: NOW - 2 * DAY,
    }
}

fn plan(entries: &[CacheEntry], open: &[u64], budget: u64) -> Vec<u64> {
    let open: BTreeSet<u64> = open.iter().copied().collect();
    budget_plan(entries, &open, NOW, budget)
}

#[test]
fn the_keys_of_the_mbx_action_objects_cache_are_one_family() {
    for key in [
        push_key("rust-test", RUST_A, 'a'),
        pull_key("rust-test", RUST_A, 'a', 99),
        push_key("rust-test", "norust", 'a'),
        format!(
            "linux-arm64-mbx-v1-rust-lint-dir-{RUST_A}-{}",
            "b".repeat(40)
        ),
    ] {
        assert_eq!(cache_family(&key), CacheFamily::Objects, "{key}");
    }
    for key in [
        format!("linux-x64-mbx-v1-dir-{RUST_A}-{}", "a".repeat(40)),
        format!("linux-x64-mbx-v2-rust-test-dir-{RUST_A}-{}", "a".repeat(40)),
        format!("linux-x64-mbx-v1-rust-test-dir-{RUST_A}-{}", "a".repeat(39)),
        format!("linux-x64-mbx-v1-rust-test-dir-rust-xyz-{}", "a".repeat(40)),
        format!("linux-x64-mbx-v1-rust-test-dir-{RUST_A}"),
        format!("linux-x64-mbx-v1-rust-test-{RUST_A}-{}", "a".repeat(40)),
    ] {
        assert_eq!(cache_family(&key), CacheFamily::Unknown, "{key}");
    }
}

#[test]
fn objects_of_a_closed_pull_request_go_even_under_the_budget() {
    let entries = [
        entry(1, pull_key("rust-test", RUST_A, 'a', 1), PR_7, 10),
        entry(2, pull_key("rust-test", RUST_A, 'a', 2), PR_8, 10),
        entry(3, push_key("rust-test", RUST_A, 'b'), MAIN, 10),
    ];
    assert_eq!(plan(&entries, &[8], 1_000), vec![1]);
}

#[test]
fn objects_of_a_replaced_toolchain_go_but_another_job_is_untouched() {
    let entries = [
        entry(1, push_key("rust-test", RUST_A, 'a'), MAIN, 10),
        entry(2, push_key("rust-test", RUST_B, 'b'), MAIN, 10),
        entry(3, push_key("rust-lint", RUST_A, 'a'), MAIN, 10),
        entry(4, pull_key("rust-test", RUST_A, 'a', 5), PR_8, 10),
    ];
    assert_eq!(plan(&entries, &[8], 1_000), vec![1]);
}

#[test]
fn old_objects_make_room_for_the_budget_and_the_newest_two_stay() {
    let mut entries: Vec<CacheEntry> = (1..=4)
        .map(|id| {
            let commit = char::from(b'a' + u8::try_from(id).unwrap_or(0));
            entry(id, push_key("rust-test", RUST_A, commit), MAIN, 100)
        })
        .collect();
    assert_eq!(plan(&entries, &[], 1_000), Vec::<u64>::new());
    assert_eq!(plan(&entries, &[], 350), vec![1]);
    assert_eq!(plan(&entries, &[], 100), vec![1, 2]);
    entries.push(entry(5, "velnor-tgt-unrecognised".to_owned(), MAIN, 1_000));
    assert_eq!(plan(&entries, &[], 100), vec![1, 2]);
}
