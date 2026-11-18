// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_lock, plan_repo, selected};
use crate::plan_api::event;

const EVERYTHING: &str =
    "Cargo.toml: all:base-unavailable\ntools/Cargo.toml: all:base-unavailable\n";

fn git(repo: &Repo, args: &[&str]) {
    let mut full = vec![
        "-c",
        "user.name=lab",
        "-c",
        "user.email=lab@example.com",
        "-c",
        "commit.gpgsign=false",
    ];
    full.extend_from_slice(args);
    repo.git(&full);
}

fn merged_pull_request(name: &str, edit: impl FnOnce(&Repo)) -> Repo {
    let repo = plan_repo(name);
    repo.write("a/src/old.rs", "pub fn old() {}\n");
    repo.write("tools/src/gone.rs", "pub fn gone() {}\n");
    repo.write("Cargo.lock", &plan_lock("1.0.5"));
    repo.commit();
    repo.git(&["branch", "-M", "main"]);
    repo.git(&["checkout", "--quiet", "-b", "topic"]);
    edit(&repo);
    repo.git(&["add", "--all"]);
    repo.commit();
    repo.git(&["checkout", "--quiet", "main"]);
    repo.write("a/src/later.rs", "pub fn later() {}\n");
    repo.commit();
    git(
        &repo,
        &["merge", "--quiet", "--no-ff", "--message", "merge", "topic"],
    );
    repo
}

fn rev(repo: &Repo, name: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", name])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn pull_request_at(repo: &Repo, sha: &str) -> Output {
    plan_command(repo)
        .args(["plan", "--explain"])
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_EVENT_PATH", event(repo, "main"))
        .env("GITHUB_SHA", sha)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn pull_request(repo: &Repo) -> Output {
    pull_request_at(repo, &rev(repo, "HEAD"))
}

fn remove(repo: &Repo, path: &str) {
    fs::remove_file(repo.root().join(path)).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_renamed_and_a_deleted_file_each_select_their_package_with_no_token() {
    let repo = merged_pull_request("git-merge", |repo| {
        repo.write("b/src/moved.rs", "pub fn old() {}\n");
        remove(repo, "a/src/old.rs");
        remove(repo, "tools/src/gone.rs");
    });
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: all:most-packages:2\n  a changed:a/src/old.rs\n  b changed:b/src/moved.rs\ntools/Cargo.toml: all:most-packages:1\n  t changed:tools/src/gone.rs\n"
    );
}

#[test]
fn a_modified_file_selects_its_package_with_no_token() {
    let repo = merged_pull_request("git-modified", |repo| {
        repo.write("b/src/lib.rs", "pub fn changed() {}\n");
    });
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  b changed:b/src/lib.rs\ntools/Cargo.toml: none\n"
    );
}

fn docs_only_merge(name: &str) -> Repo {
    merged_pull_request(name, |repo| repo.write("docs/readme.md", "docs\n"))
}

#[test]
fn an_unstaged_source_edit_after_a_docs_only_merge_selects_everything() {
    let repo = docs_only_merge("git-dirty-worktree");
    let clean = pull_request(&repo);
    assert!(clean.status.success(), "{clean:?}");
    assert_eq!(
        selected(&clean),
        "Cargo.toml: none\ntools/Cargo.toml: none\n"
    );
    repo.write("b/src/lib.rs", "pub fn broken( {\n");
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(selected(&output), EVERYTHING);
}

#[test]
fn a_staged_source_edit_after_a_docs_only_merge_selects_everything() {
    let repo = docs_only_merge("git-dirty-index");
    repo.write("b/src/lib.rs", "pub fn broken( {\n");
    repo.git(&["add", "b/src/lib.rs"]);
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(selected(&output), EVERYTHING);
}

#[test]
fn a_changed_lockfile_is_compared_with_its_text_at_the_first_parent() {
    let repo = merged_pull_request("git-lock", |repo| {
        repo.write("Cargo.lock", &plan_lock("1.0.6"));
    });
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: some\n  b lock:fnv\ntools/Cargo.toml: none\n"
    );
}

#[test]
fn a_checkout_that_is_not_a_merge_commit_selects_everything() {
    let repo = plan_repo("git-plain");
    repo.commit();
    repo.write("a/src/lib.rs", "pub fn changed() {}\n");
    repo.commit();
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(selected(&output), EVERYTHING);
}

#[test]
fn a_checkout_whose_head_is_not_the_events_merge_commit_selects_everything() {
    let repo = merged_pull_request("git-other-sha", |repo| {
        repo.write("a/src/lib.rs", "pub fn changed() {}\n");
    });
    let base = rev(&repo, "HEAD^1");
    let output = pull_request_at(&repo, &base);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(selected(&output), EVERYTHING);
}

#[test]
fn a_checkout_with_no_git_history_selects_everything() {
    let repo = plan_repo("git-none");
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(selected(&output), EVERYTHING);
}

#[test]
fn a_rejected_git_path_is_escaped_when_plan_selects_everything() {
    let hostile = "notes.txt\n::add-mask::super-secret\ntail";
    let repo = plan_repo("git-hostile-path");
    repo.write(hostile, "old\n");
    repo.commit();
    repo.git(&["branch", "-M", "main"]);
    repo.git(&["checkout", "--quiet", "-b", "topic"]);
    remove(&repo, hostile);
    repo.git(&["add", "--all"]);
    repo.commit();
    repo.git(&["checkout", "--quiet", "main"]);
    repo.write("a/src/later.rs", "pub fn later() {}\n");
    repo.commit();
    git(
        &repo,
        &["merge", "--quiet", "--no-ff", "--message", "merge", "topic"],
    );
    let output = pull_request(&repo);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(selected(&output), EVERYTHING);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("notes.txt%0A::add-mask::super-secret%0Atail"),
        "{stderr}"
    );
    assert!(
        stderr.lines().all(|line| !line.starts_with("::")),
        "{stderr}"
    );
}

#[test]
fn the_outputs_a_job_reads_from_git_name_only_the_changed_packages_and_skip_policy_tools() {
    let repo = merged_pull_request("git-outputs", |repo| {
        repo.write("a/src/new.rs", "pub fn new() {}\n");
    });
    let outputs = repo.root().join("github-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(&repo)
        .arg("plan")
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_EVENT_PATH", event(&repo, "main"))
        .env("GITHUB_SHA", rev(&repo, "HEAD"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let text = fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(text.contains("rust_root_packages=a\n"), "{text}");
    assert!(text.contains("rust_root_package_args=-p a\n"), "{text}");
    assert!(
        text.contains("rust_deny=skip\nrust_machete=skip\n"),
        "{text}"
    );
}
