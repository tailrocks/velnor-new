// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::{Head, RunEvent};

use crate::gen_support::{pull, push, queue, run};

fn path(run: &velnor_cache_rules::Run) -> Option<String> {
    run.publishes().map(|scope| scope.path())
}

#[test]
fn trusted_push_to_the_default_branch_publishes_main() {
    assert_eq!(
        path(&push("main", 1)),
        Some(format!("main/{}", "01".repeat(20)))
    );
}

#[test]
fn unprotected_push_publishes_nothing() {
    let mut unprotected = push("main", 1);
    unprotected.protected_ref = false;
    assert_eq!(path(&unprotected), None);
}

#[test]
fn push_to_another_branch_publishes_nothing() {
    assert_eq!(path(&push("feature", 1)), None);
}

#[test]
fn merge_group_publishes_mg_of_its_commit() {
    assert_eq!(path(&queue(2)), Some(format!("mg/{}", "02".repeat(20))));
}

#[test]
fn same_repository_pull_request_publishes_a_new_name_per_run_and_attempt() {
    assert_eq!(path(&pull(7, 3)), Some("pr7/900-2".to_owned()));
}

#[test]
fn fork_pull_request_publishes_nothing() {
    let fork = run(
        RunEvent::PullRequest {
            number: 7,
            base_branch: "main".to_owned(),
            head: Head::Fork,
        },
        3,
    );
    assert_eq!(path(&fork), None);
}

#[test]
fn tag_publishes_nothing() {
    let tag = run(
        RunEvent::Tag {
            name: "v1".to_owned(),
        },
        4,
    );
    assert_eq!(path(&tag), None);
}
