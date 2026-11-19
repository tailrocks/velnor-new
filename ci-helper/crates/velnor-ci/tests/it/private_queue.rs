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

fn compare(group: &str, status: &str) -> (String, String) {
    (
        format!("/repos/{REPOSITORY}/compare/{group}...{OURS_GROUP}?per_page=1"),
        format!("{{\"status\":\"{status}\"}}"),
    )
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
            compare(AHEAD_GROUP, "ahead"),
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
            compare(AHEAD_GROUP, "ahead"),
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
            compare(BEHIND_GROUP, "behind"),
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
fn an_unreadable_queue_listing_or_comparison_is_refused() {
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
            compare(AHEAD_GROUP, "teapot"),
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
