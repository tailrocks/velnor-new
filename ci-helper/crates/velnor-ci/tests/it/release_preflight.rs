// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::github::{commit_pulls_path, compare_path};
use velnor_ci::release_preflight::check_release_source;

use crate::support::{FakeGitHub, repo};

const VERIFIED: &str = "1111111111111111111111111111111111111111";
const PR_HEAD: &str = "2222222222222222222222222222222222222222";
const REFUSAL: &str = "release PRs must be squash-merged (or merged through a merge queue with squash) so the tag names the verified commit";

fn pulls(fake: &mut FakeGitHub, items: &[(u64, &str, &str)]) {
    let list: Vec<String> = items
        .iter()
        .map(|(number, branch, sha)| {
            format!(r#"{{"number":{number},"head":{{"ref":"{branch}","sha":"{sha}"}}}}"#)
        })
        .collect();
    fake.respond(
        commit_pulls_path(&repo(), VERIFIED),
        &format!("[{}]", list.join(",")),
    );
}

fn compare(fake: &mut FakeGitHub, status: &str) {
    fake.respond(
        compare_path(&repo(), PR_HEAD, VERIFIED),
        &format!(r#"{{"status":"{status}"}}"#),
    );
}

fn check(fake: &FakeGitHub) -> Result<(), String> {
    check_release_source(fake, &repo(), VERIFIED, &["release-plz-".to_owned()])
        .map_err(|e| e.to_string())
}

#[test]
fn a_merge_commit_release_pr_is_refused() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(7, "release-plz-2026", PR_HEAD)]);
    compare(&mut fake, "ahead");
    let error = check(&fake).err().unwrap_or_default();
    assert!(error.contains("#7") && error.contains(REFUSAL), "{error}");
}

#[test]
fn a_squash_merged_release_pr_is_accepted() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(7, "release-plz-2026", PR_HEAD)]);
    compare(&mut fake, "diverged");
    assert_eq!(check(&fake), Ok(()));
}

#[test]
fn a_release_pr_whose_last_commit_is_the_verified_commit_is_accepted() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(7, "release-plz-2026", VERIFIED)]);
    assert_eq!(check(&fake), Ok(()));
}

#[test]
fn a_pull_request_that_is_not_a_release_pr_is_not_asked_about() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(8, "feature/x", PR_HEAD)]);
    assert_eq!(check(&fake), Ok(()));
    pulls(&mut fake, &[]);
    assert_eq!(check(&fake), Ok(()));
}

#[test]
fn a_full_page_of_associated_pull_requests_is_refused() {
    let mut fake = FakeGitHub::default();
    let items: Vec<_> = (1..=100)
        .map(|number| (number, "feature/x", VERIFIED))
        .collect();
    pulls(&mut fake, &items[..99]);
    assert_eq!(check(&fake), Ok(()));
    pulls(&mut fake, &items);
    assert_eq!(
        check(&fake),
        Err(format!(
            "100 or more pull requests are associated with {VERIFIED}; cannot tell which release pull request release-plz would tag"
        ))
    );
}

#[test]
fn an_api_error_fails_closed() {
    let mut fake = FakeGitHub::default();
    assert!(check(&fake).is_err());
    pulls(&mut fake, &[(7, "release-plz-2026", PR_HEAD)]);
    assert!(check(&fake).is_err());
}

#[test]
fn a_sha_that_is_not_forty_lowercase_hex_digits_is_refused() {
    let fake = FakeGitHub::default();
    assert!(check_release_source(&fake, &repo(), "main", &["release-plz-".to_owned()]).is_err());
}
