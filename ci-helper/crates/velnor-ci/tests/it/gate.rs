// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::gate::release_gate;
use velnor_ci::github::{check_runs_path, compare_path};

use crate::support::{FakeGitHub, repo};

pub(crate) const ON_MAIN_GREEN: &str = "87919076157ac0b3a611ce4f18c8301c1a569e9e";
pub(crate) const OFF_MAIN_GREEN: &str = "a68e6204fc4171264e247825bbf5468b3bc8330d";
pub(crate) const ON_MAIN_RED: &str = "f4a062683f7e31e3c654c3beebaa6751463f6459";
pub(crate) const OFF_MAIN_RED: &str = "ce626ef859cf255461f3eec883f18f65e583d792";

fn verdict(fake: &FakeGitHub, sha: &str) -> Result<String, String> {
    release_gate(fake, &repo(), sha).map_err(|e| e.to_string())
}

fn refusal(fake: &FakeGitHub, sha: &str) -> String {
    match verdict(fake, sha) {
        Ok(said) => unreachable!("released: {said}"),
        Err(said) => said,
    }
}

fn with_runs(sha: &str, runs: &str) -> FakeGitHub {
    let mut fake = FakeGitHub::gate();
    fake.respond(
        check_runs_path(&repo(), sha, "Required"),
        &format!("{{\"check_runs\": {runs}}}"),
    );
    fake
}

#[test]
fn a_commit_on_main_with_a_passing_required_check_is_released() {
    let said = verdict(&FakeGitHub::gate(), ON_MAIN_GREEN);
    assert_eq!(
        said,
        Ok(format!(
            "commit {ON_MAIN_GREEN} is on main\ncheck Required passed on commit {ON_MAIN_GREEN}\n"
        ))
    );
}

#[test]
fn a_commit_that_is_not_on_the_default_branch_is_refused_whatever_its_checks_say() {
    let said = refusal(&FakeGitHub::gate(), OFF_MAIN_GREEN);
    assert_eq!(
        said,
        format!("commit {OFF_MAIN_GREEN} is not on the default branch main (comparison: diverged)")
    );
}

#[test]
fn a_commit_whose_required_check_failed_is_refused_even_on_main() {
    let said = refusal(&FakeGitHub::gate(), ON_MAIN_RED);
    assert_eq!(
        said,
        format!("check Required did not pass on commit {ON_MAIN_RED}: it concluded failure")
    );
}

#[test]
fn a_commit_that_fails_both_facts_is_refused_with_both_reasons() {
    let said = refusal(&FakeGitHub::gate(), OFF_MAIN_RED);
    assert_eq!(said.lines().count(), 2, "{said}");
    assert!(said.contains("not on the default branch main"), "{said}");
    assert!(said.contains("it concluded failure"), "{said}");
}

#[test]
fn only_a_finished_successful_run_of_required_by_github_actions_counts() {
    let run = |name: &str, status: &str, conclusion: &str, app: &str| {
        format!(
            "[{{\"name\": \"{name}\", \"status\": \"{status}\", \"conclusion\": {conclusion}, \"app\": {app}}}]"
        )
    };
    let actions = "{\"slug\": \"github-actions\"}";
    let cases = [
        ("[]".to_owned(), "no run of it by github-actions"),
        (
            run(
                "Required",
                "completed",
                "\"success\"",
                "{\"slug\": \"evil-app\"}",
            ),
            "no run of it by github-actions",
        ),
        (
            run("Required", "completed", "\"success\"", "null"),
            "no run of it by github-actions",
        ),
        (
            run("Required verdict", "completed", "\"success\"", actions),
            "no run of it by github-actions",
        ),
        (
            run("Required", "in_progress", "null", actions),
            "it is in_progress",
        ),
        (
            run("Required", "completed", "\"cancelled\"", actions),
            "it concluded cancelled",
        ),
        (
            run("Required", "completed", "\"skipped\"", actions),
            "it concluded skipped",
        ),
        (
            run("Required", "completed", "null", actions),
            "it concluded without a result",
        ),
    ];
    for (runs, reason) in cases {
        let said = refusal(&with_runs(ON_MAIN_GREEN, &runs), ON_MAIN_GREEN);
        assert!(said.ends_with(reason), "{runs} gave {said}");
    }
    let passing = run("Required", "completed", "\"success\"", actions);
    assert!(verdict(&with_runs(ON_MAIN_GREEN, &passing), ON_MAIN_GREEN).is_ok());
}

#[test]
fn a_foreign_failing_run_cannot_pass_and_cannot_hide_a_failure() {
    let mixed = "[{\"name\": \"Required\", \"status\": \"completed\", \"conclusion\": \"success\", \"app\": {\"slug\": \"evil-app\"}}, {\"name\": \"Required\", \"status\": \"completed\", \"conclusion\": \"failure\", \"app\": {\"slug\": \"github-actions\"}}]";
    let said = refusal(&with_runs(ON_MAIN_GREEN, mixed), ON_MAIN_GREEN);
    assert!(said.ends_with("it concluded failure"), "{said}");
}

#[test]
fn a_value_that_is_not_a_full_commit_sha_never_reaches_the_api() {
    let upper = ON_MAIN_GREEN.to_uppercase();
    for sha in ["", "main", "87919076", upper.as_str(), "../x"] {
        let said = refusal(&FakeGitHub::default(), sha);
        assert!(said.contains("not a full lowercase commit SHA"), "{said}");
    }
}

#[test]
fn the_default_branch_is_whatever_the_repository_says() {
    let mut fake = FakeGitHub::gate();
    fake.respond(
        "/repos/tailrocks/velnor-new".to_owned(),
        "{\"default_branch\": \"trunk/a b\"}",
    );
    fake.respond(
        compare_path(&repo(), "trunk/a b", ON_MAIN_GREEN),
        "{\"status\": \"identical\"}",
    );
    assert_eq!(
        verdict(&fake, ON_MAIN_GREEN).map(|said| said.lines().next().map(str::to_owned)),
        Ok(Some(format!("commit {ON_MAIN_GREEN} is on trunk/a b")))
    );
}
