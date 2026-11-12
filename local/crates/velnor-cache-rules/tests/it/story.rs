// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_cache_rules::AlreadyExists;

use crate::support::{Repository, id, pull_request, push};

fn hit(scope: &str, key: &str) -> (String, String) {
    (scope.to_owned(), key.to_owned())
}

#[test]
fn story_push_saves_pull_request_restores_saves_and_restores_again() {
    let mut repository = Repository::new();
    let main = push("main");
    let first = pull_request(7, "main");
    let second = pull_request(8, "main");

    assert_eq!(repository.restore(&main, "deps-main-1", &["deps-"]), None);
    assert_eq!(repository.save(&main, "deps-main-1"), Ok(()));

    assert_eq!(
        repository.restore(&first, "deps-pr7-1", &["deps-"]),
        Some(hit("refs/heads/main", "deps-main-1"))
    );
    assert_eq!(repository.save(&first, "deps-pr7-1"), Ok(()));

    assert_eq!(
        repository.restore(&first, "deps-pr7-1", &["deps-"]),
        Some(hit("refs/pull/7/merge", "deps-pr7-1"))
    );
    assert_eq!(
        repository.save(&first, "deps-pr7-1"),
        Err(AlreadyExists(id("refs/pull/7/merge", "deps-pr7-1", "v1")))
    );

    assert_eq!(repository.restore(&second, "deps-pr7-1", &[]), None);
    assert_eq!(
        repository.restore(&second, "deps-pr8-1", &["deps-"]),
        Some(hit("refs/heads/main", "deps-main-1"))
    );
    assert_eq!(repository.save(&second, "deps-pr7-1"), Ok(()));

    assert_eq!(repository.restore(&main, "deps-pr7-1", &["deps-pr"]), None);
}
