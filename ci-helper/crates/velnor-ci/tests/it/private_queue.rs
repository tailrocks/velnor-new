// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;

use velnor_ci::error::CiError;
use velnor_ci::github::GitHub;
use velnor_ci::private_token::{check, parse_queue_ref};

const REPOSITORY: &str = "acme/app";
const OURS: &str = "1111111111111111111111111111111111111111";
const OURS_GROUP: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const AHEAD: &str = "2222222222222222222222222222222222222222";
const AHEAD_GROUP: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const BEHIND: &str = "3333333333333333333333333333333333333333";
const BEHIND_GROUP: &str = "cccccccccccccccccccccccccccccccccccccccc";

struct Fake(HashMap<String, String>);

impl GitHub for Fake {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| CiError::new(format!("no recorded response for {path}")))
    }
}

fn head_ref(number: u32, sha: &str) -> String {
    format!("refs/heads/gh-readonly-queue/main/pr-{number}-{sha}")
}

fn pull(number: u32, repository: &str, sha: &str) -> (String, String) {
    (
        format!("/repos/{REPOSITORY}/pulls/{number}"),
        format!("{{\"head\":{{\"sha\":\"{sha}\",\"repo\":{{\"full_name\":\"{repository}\"}}}}}}"),
    )
}

fn comparison(page: u32, total: usize, shas: &[&str]) -> (String, String) {
    let commits: Vec<String> = shas
        .iter()
        .map(|sha| format!("{{\"sha\":\"{sha}\"}}"))
        .collect();
    (
        format!("/repos/{REPOSITORY}/compare/main...{OURS_GROUP}?per_page=100&page={page}"),
        format!(
            "{{\"total_commits\":{total},\"commits\":[{}]}}",
            commits.join(",")
        ),
    )
}

/// The checkout holds the given queue groups beyond `main`.
fn compare(holds: &[&str]) -> (String, String) {
    comparison(1, holds.len(), holds)
}

fn queue(entries: &[(u32, &str, &str)], extra: Vec<(String, String)>) -> Fake {
    let refs: Vec<String> = entries
        .iter()
        .map(|(number, sha, group)| {
            format!(
                "{{\"ref\":\"{}\",\"object\":{{\"sha\":\"{group}\"}}}}",
                head_ref(*number, sha)
            )
        })
        .collect();
    let mut routes = HashMap::from([(
        format!("/repos/{REPOSITORY}/git/matching-refs/heads/gh-readonly-queue/main/?per_page=100"),
        format!("[{}]", refs.join(",")),
    )]);
    routes.extend(extra);
    Fake(routes)
}

fn error(result: Result<(), CiError>) -> String {
    result.err().map(|e| e.to_string()).unwrap_or_default()
}

#[test]
fn a_queue_entry_stacked_on_a_fork_pull_request_is_refused() {
    let github = queue(
        &[(42, OURS, OURS_GROUP), (41, AHEAD, AHEAD_GROUP)],
        vec![
            pull(42, REPOSITORY, OURS),
            pull(41, "mallory/app", AHEAD),
            compare(&[AHEAD_GROUP, OURS_GROUP]),
        ],
    );
    let message = error(check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &github));
    assert!(message.contains("mallory/app"), "{message}");
}

#[test]
fn a_queue_entry_stacked_only_on_same_repository_pull_requests_is_allowed() {
    let github = queue(
        &[(42, OURS, OURS_GROUP), (41, AHEAD, AHEAD_GROUP)],
        vec![
            pull(42, REPOSITORY, OURS),
            pull(41, REPOSITORY, AHEAD),
            compare(&[AHEAD_GROUP, OURS_GROUP]),
        ],
    );
    assert_eq!(
        check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &github)
            .err()
            .map(|e| e.to_string()),
        None
    );
}

#[test]
fn a_fork_pull_request_queued_behind_this_entry_is_not_part_of_its_checkout() {
    let github = queue(
        &[(42, OURS, OURS_GROUP), (43, BEHIND, BEHIND_GROUP)],
        vec![
            pull(42, REPOSITORY, OURS),
            pull(43, "mallory/app", BEHIND),
            compare(&[OURS_GROUP]),
        ],
    );
    assert_eq!(
        check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &github)
            .err()
            .map(|e| e.to_string()),
        None
    );
}

#[test]
fn an_unreadable_queue_listing_or_a_truncated_comparison_is_refused() {
    let missing = queue(
        &[(41, AHEAD, AHEAD_GROUP)],
        vec![pull(42, REPOSITORY, OURS)],
    );
    let message = error(check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &missing));
    assert!(message.contains("queue"), "{message}");
    let unknown = queue(
        &[(42, OURS, OURS_GROUP), (41, AHEAD, AHEAD_GROUP)],
        vec![
            pull(42, REPOSITORY, OURS),
            pull(41, REPOSITORY, AHEAD),
            comparison(1, 9, &[AHEAD_GROUP]),
        ],
    );
    assert!(check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &unknown).is_err());
    let no_listing = Fake(HashMap::from([pull(42, REPOSITORY, OURS)]));
    assert!(check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &no_listing).is_err());
}

#[test]
fn a_malformed_head_ref_is_not_a_queue_entry() {
    for reference in [
        "",
        "refs/heads/main",
        "refs/heads/gh-readonly-queue/main/pr-42",
        "refs/heads/gh-readonly-queue/main/pr-x-0123456789abcdef0123456789abcdef01234567",
        "refs/heads/gh-readonly-queue/main/pr-42-0123",
        "refs/heads/gh-readonly-queue/main/pr-42-0123456789abcdef0123456789abcdef0123456789",
        "refs/heads/gh-readonly-queue/main/pr-42-g123456789abcdef0123456789abcdef01234567",
        "refs/heads/feature/pr-42-0123456789abcdef0123456789abcdef01234567",
        "gh-readonly-queue//pr-42-0123456789abcdef0123456789abcdef01234567",
    ] {
        assert!(parse_queue_ref(reference).is_err(), "{reference}");
    }
}

#[test]
fn a_pull_request_answer_without_a_head_is_refused() {
    let github = Fake(HashMap::from([(
        format!("/repos/{REPOSITORY}/pulls/42"),
        "{}".to_owned(),
    )]));
    assert!(check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &github).is_err());
}

#[test]
fn a_queue_ref_rebuilt_since_the_event_is_refused() {
    let github = queue(&[(42, OURS, OURS_GROUP)], vec![pull(42, REPOSITORY, OURS)]);
    let message = error(check(&head_ref(42, OURS), REPOSITORY, AHEAD_GROUP, &github));
    assert!(message.contains("rebuilt"), "{message}");
}

struct Counting {
    inner: Fake,
    calls: std::cell::RefCell<Vec<String>>,
}

impl GitHub for Counting {
    fn get(&self, path: &str) -> Result<String, CiError> {
        self.calls.borrow_mut().push(path.to_owned());
        self.inner.get(path)
    }
}

#[test]
fn a_full_queue_is_authenticated_with_one_comparison_not_one_per_entry() {
    let mut routes = vec![pull(42, REPOSITORY, OURS)];
    let mut entries = vec![(42, OURS, OURS_GROUP)];
    let shas: Vec<String> = (0..20).map(|n| format!("{n:040x}")).collect();
    let groups: Vec<String> = (0..20).map(|n| format!("{:040x}", n + 100)).collect();
    for (n, (sha, group)) in shas.iter().zip(&groups).enumerate() {
        let number = 100 + n as u32;
        routes.push(pull(number, REPOSITORY, sha));
        entries.push((number, sha.as_str(), group.as_str()));
    }
    let held: Vec<&str> = groups.iter().map(String::as_str).collect();
    routes.push(compare(&held));
    let github = Counting {
        inner: queue(&entries, routes),
        calls: Default::default(),
    };
    assert_eq!(
        check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &github)
            .err()
            .map(|e| e.to_string()),
        None
    );
    let calls = github.calls.borrow();
    let compares = calls
        .iter()
        .filter(|path| path.contains("/compare/"))
        .count();
    assert_eq!(compares, 1, "{calls:?}");
}

#[test]
fn a_comparison_spanning_pages_is_read_to_the_end() {
    let github = queue(
        &[(42, OURS, OURS_GROUP), (41, AHEAD, AHEAD_GROUP)],
        vec![
            pull(42, REPOSITORY, OURS),
            pull(41, "mallory/app", AHEAD),
            comparison(1, 2, &[OURS_GROUP]),
            comparison(2, 2, &[AHEAD_GROUP]),
        ],
    );
    let message = error(check(&head_ref(42, OURS), REPOSITORY, OURS_GROUP, &github));
    assert!(message.contains("mallory/app"), "{message}");
}
