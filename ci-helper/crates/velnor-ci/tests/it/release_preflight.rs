// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::github::{commit_pulls_path, compare_path, pull_commits_path};
use velnor_ci::release_preflight::check_release_source;

use crate::support::{FakeGitHub, repo};

const VERIFIED: &str = "1111111111111111111111111111111111111111";
const PR_HEAD: &str = "2222222222222222222222222222222222222222";
const THIRTIETH: &str = "3333333333333333333333333333333333333333";
const UNPAGED: &str = "/repos/tailrocks/velnor-new/pulls/7/commits";
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

fn compare(fake: &mut FakeGitHub, sha: &str, status: &str) {
    fake.respond(
        compare_path(&repo(), sha, VERIFIED),
        &format!(r#"{{"status":"{status}"}}"#),
    );
}

fn page_ending_with(last: &str) -> Vec<String> {
    let mut shas: Vec<String> = (1..30).map(|n| format!("{n:040x}")).collect();
    shas.push(last.to_owned());
    shas
}

fn commits(fake: &mut FakeGitHub, number: u64, shas: &[String]) {
    let body = shas
        .iter()
        .map(|sha| format!(r#"{{"sha":"{sha}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    fake.respond(pull_commits_path(&repo(), number), &format!("[{body}]"));
}

fn check(fake: &FakeGitHub) -> Result<(), String> {
    check_release_source(fake, &repo(), VERIFIED, &["release-plz-".to_owned()])
        .map_err(|e| e.to_string())
}

#[test]
fn a_merge_commit_release_pr_is_refused() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(7, "release-plz-2026", PR_HEAD)]);
    commits(&mut fake, 7, &[PR_HEAD.to_owned()]);
    compare(&mut fake, PR_HEAD, "ahead");
    let error = check(&fake).err().unwrap_or_default();
    assert!(
        error.contains("#7") && error.contains(PR_HEAD) && error.contains(REFUSAL),
        "{error}"
    );
}

#[test]
fn a_release_pr_whose_unpaged_commits_end_on_an_ancestor_is_refused() {
    let mut fake = FakeGitHub::default();
    assert_eq!(pull_commits_path(&repo(), 7), UNPAGED);
    pulls(&mut fake, &[(7, "release-plz-2026", VERIFIED)]);
    commits(&mut fake, 7, &page_ending_with(THIRTIETH));
    compare(&mut fake, THIRTIETH, "ahead");
    let error = check(&fake).err().unwrap_or_default();
    assert!(
        error.contains("#7") && error.contains(THIRTIETH) && error.contains(REFUSAL),
        "{error}"
    );
}

#[test]
fn a_squash_merged_release_pr_is_accepted() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(7, "release-plz-2026", PR_HEAD)]);
    commits(&mut fake, 7, &[PR_HEAD.to_owned()]);
    compare(&mut fake, PR_HEAD, "diverged");
    assert_eq!(check(&fake), Ok(()));
}

#[test]
fn a_release_pr_whose_last_commit_is_the_verified_commit_is_accepted() {
    let mut fake = FakeGitHub::default();
    pulls(&mut fake, &[(7, "release-plz-2026", VERIFIED)]);
    let first = 1;
    let earlier = format!("{first:040x}");
    let shas = page_ending_with(VERIFIED);
    assert_eq!(shas.first().map(String::as_str), Some(earlier.as_str()));
    compare(&mut fake, &earlier, "ahead");
    commits(&mut fake, 7, &shas);
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
fn an_api_error_fails_closed() {
    let fake = FakeGitHub::default();
    assert!(check(&fake).is_err());

    let mut missing_compare = FakeGitHub::default();
    pulls(&mut missing_compare, &[(7, "release-plz-2026", PR_HEAD)]);
    commits(&mut missing_compare, 7, &[PR_HEAD.to_owned()]);
    assert!(check(&missing_compare).is_err());

    let mut missing_commits = FakeGitHub::default();
    pulls(&mut missing_commits, &[(7, "release-plz-2026", VERIFIED)]);
    assert!(check(&missing_commits).is_err());
}

#[test]
fn a_sha_that_is_not_forty_lowercase_hex_digits_is_refused() {
    let fake = FakeGitHub::default();
    assert!(check_release_source(&fake, &repo(), "main", &["release-plz-".to_owned()]).is_err());
}
