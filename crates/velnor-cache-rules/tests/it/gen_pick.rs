// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{GenKey, Generation, Run, Want, pick};

use crate::gen_support::{fork, generation, key, main, mg, pr, pull, push, queue, rust, sha};

fn picked(
    run: &Run,
    key: &GenKey,
    lock: &str,
    base: Option<u8>,
    all: &[Generation],
) -> Option<String> {
    let base = base.map(sha);
    let want = Want {
        run,
        key,
        lock,
        base: base.as_ref(),
    };
    pick(&want, all).map(|g| g.scope.path())
}

#[test]
fn main_pick_takes_the_base_commit_over_a_newer_generation() {
    let k = rust();
    let all = [
        generation(&k, main(1), "L1", 10),
        generation(&k, main(2), "L1", 20),
    ];
    let got = picked(&push("main", 9), &k, "L1", Some(1), &all);
    assert_eq!(got, Some(main(1).path()));
}

#[test]
fn main_pick_without_the_base_takes_the_newest_with_the_same_lock() {
    let k = rust();
    let all = [
        generation(&k, main(1), "L1", 10),
        generation(&k, main(3), "L2", 30),
        generation(&k, main(2), "L1", 20),
    ];
    let got = picked(&push("main", 9), &k, "L1", Some(7), &all);
    assert_eq!(got, Some(main(2).path()));
}

#[test]
fn main_pick_prefers_a_newer_same_lock_generation_over_a_base_with_another_lock() {
    let k = rust();
    let all = [
        generation(&k, main(1), "L2", 10),
        generation(&k, main(2), "L1", 20),
    ];
    let got = picked(&push("main", 9), &k, "L1", Some(1), &all);
    assert_eq!(got, Some(main(2).path()));
}

#[test]
fn main_pick_with_no_lock_match_takes_the_base_then_the_newest() {
    let k = rust();
    let all = [
        generation(&k, main(1), "L2", 10),
        generation(&k, main(3), "L2", 30),
    ];
    assert_eq!(
        picked(&push("main", 9), &k, "L1", Some(1), &all),
        Some(main(1).path())
    );
    assert_eq!(
        picked(&push("main", 9), &k, "L1", Some(7), &all),
        Some(main(3).path())
    );
    assert_eq!(
        picked(&push("main", 9), &k, "L1", None, &all),
        Some(main(3).path())
    );
}

#[test]
fn equal_publish_times_pick_deterministically() {
    let k = rust();
    let forward = [
        generation(&k, main(1), "L1", 10),
        generation(&k, main(2), "L1", 10),
    ];
    let reverse = [
        generation(&k, main(2), "L1", 10),
        generation(&k, main(1), "L1", 10),
    ];
    let a = picked(&push("main", 9), &k, "L1", None, &forward);
    assert_eq!(a, picked(&push("main", 9), &k, "L1", None, &reverse));
    assert_eq!(a, Some(main(2).path()));
}

#[test]
fn no_generation_of_the_wanted_key_means_no_pick() {
    let k = rust();
    let all = [
        generation(&key("lint", "root", "0123456789abcdef"), main(1), "L1", 10),
        generation(&key("rust", "web", "0123456789abcdef"), main(1), "L1", 10),
        generation(&key("rust", "root", "fedcba9876543210"), main(1), "L1", 10),
    ];
    assert_eq!(picked(&push("main", 9), &k, "L1", Some(1), &all), None);
}

#[test]
fn pull_request_reads_its_own_pr_before_a_newer_main_generation() {
    let k = rust();
    let all = [
        generation(&k, pr(7, 10, 1), "L1", 10),
        generation(&k, main(1), "L1", 50),
    ];
    assert_eq!(
        picked(&pull(7, 9), &k, "L1", Some(1), &all),
        Some("pr7/10-1".to_owned())
    );
}

#[test]
fn pull_request_takes_the_highest_run_and_attempt_whatever_the_publish_order() {
    let k = rust();
    let all = [
        generation(&k, pr(7, 10, 3), "L1", 30),
        generation(&k, pr(7, 11, 1), "L1", 5),
        generation(&k, pr(7, 10, 2), "L1", 40),
    ];
    assert_eq!(
        picked(&pull(7, 9), &k, "L1", None, &all),
        Some("pr7/11-1".to_owned())
    );
}

#[test]
fn pull_request_prefers_the_same_lock_then_any() {
    let k = rust();
    let all = [
        generation(&k, pr(7, 5, 1), "L1", 10),
        generation(&k, pr(7, 6, 1), "L2", 20),
    ];
    assert_eq!(
        picked(&pull(7, 9), &k, "L1", None, &all),
        Some("pr7/5-1".to_owned())
    );
    assert_eq!(
        picked(&pull(7, 9), &k, "L3", None, &all),
        Some("pr7/6-1".to_owned())
    );
}

#[test]
fn pull_request_never_reads_another_pull_request() {
    let k = rust();
    let only_pr7 = [generation(&k, pr(7, 10, 1), "L1", 10)];
    assert_eq!(picked(&pull(8, 9), &k, "L1", None, &only_pr7), None);
    let with_main = [only_pr7[0].clone(), generation(&k, main(1), "L1", 5)];
    assert_eq!(
        picked(&pull(8, 9), &k, "L1", None, &with_main),
        Some(main(1).path())
    );
}

#[test]
fn push_and_merge_group_never_read_a_pull_request() {
    let k = rust();
    let all = [generation(&k, pr(7, 10, 1), "L1", 10)];
    assert_eq!(picked(&push("main", 9), &k, "L1", None, &all), None);
    assert_eq!(picked(&queue(9), &k, "L1", None, &all), None);
}

#[test]
fn merge_group_reads_main_and_never_mg() {
    let k = rust();
    let all = [generation(&k, mg(9), "L1", 10)];
    assert_eq!(picked(&queue(9), &k, "L1", None, &all), None);
    let with_main = [all[0].clone(), generation(&k, main(1), "L1", 5)];
    assert_eq!(
        picked(&queue(9), &k, "L1", None, &with_main),
        Some(main(1).path())
    );
}

#[test]
fn push_reads_mg_of_its_own_sha_before_main() {
    let k = rust();
    let all = [
        generation(&k, mg(9), "L1", 10),
        generation(&k, mg(8), "L1", 20),
        generation(&k, main(1), "L1", 50),
    ];
    let got = picked(&push("main", 9), &k, "L1", Some(1), &all);
    assert_eq!(got, Some(format!("mg/{}", "09".repeat(20))));
}

#[test]
fn push_never_reads_mg_of_another_sha() {
    let k = rust();
    let all = [generation(&k, mg(8), "L1", 10)];
    assert_eq!(picked(&push("main", 9), &k, "L1", None, &all), None);
}

#[test]
fn push_to_another_branch_never_reads_mg() {
    let k = rust();
    let all = [generation(&k, mg(9), "L1", 10)];
    assert_eq!(picked(&push("feature", 9), &k, "L1", None, &all), None);
}

#[test]
fn fork_pull_request_reads_nothing() {
    let k = rust();
    let all = [
        generation(&k, main(1), "L1", 10),
        generation(&k, pr(7, 1, 1), "L1", 20),
    ];
    assert_eq!(picked(&fork(7, 9), &k, "L1", Some(1), &all), None);
}
