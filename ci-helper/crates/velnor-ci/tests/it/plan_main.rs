// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo, selected};

const EVERYTHING: &str =
    "Cargo.toml: all:base-unavailable\ntools/Cargo.toml: all:base-unavailable\n";
const NOTHING: &str = "Cargo.toml: none\ntools/Cargo.toml: none\n";
const APP: &str = "Cargo.toml: some\n  b changed:b/src/lib.rs\ntools/Cargo.toml: none\n";
const ZEROS: &str = "0000000000000000000000000000000000000000";

pub(crate) fn rev(repo: &Repo, name: &str) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", name])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

pub(crate) fn two_commits(name: &str, config: &str, edit: impl FnOnce(&Repo)) -> Repo {
    let repo = plan_repo(name);
    repo.write(".velnor/rust-ci.toml", config);
    repo.commit();
    repo.git(&["branch", "-M", "main"]);
    edit(&repo);
    repo.commit();
    repo
}

fn run(repo: &Repo, name: &str, payload: &str, sha: &str) -> Output {
    let file = repo.root().join("event.json");
    fs::write(&file, payload).unwrap_or_else(|e| unreachable!("{e}"));
    plan_command(repo)
        .args(["plan", "--explain"])
        .env("GITHUB_EVENT_NAME", name)
        .env("GITHUB_EVENT_PATH", file)
        .env("GITHUB_SHA", sha)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn push(repo: &Repo, before: &str) -> Output {
    let head = rev(repo, "HEAD");
    let payload = format!(
        "{{\"before\":\"{before}\",\"after\":\"{head}\",\"repository\":{{\"default_branch\":\"main\"}}}}"
    );
    run(repo, "push", &payload, &head)
}

fn merge_group(repo: &Repo, base: &str, head: &str) -> Output {
    let payload = format!(
        "{{\"merge_group\":{{\"base_sha\":\"{base}\",\"head_sha\":\"{head}\"}},\"repository\":{{\"default_branch\":\"main\"}}}}"
    );
    run(repo, "merge_group", &payload, &rev(repo, "HEAD"))
}

pub(crate) fn docs(repo: &Repo) {
    repo.write("docs/guide.md", "guide\n");
}

pub(crate) fn app(repo: &Repo) {
    repo.write("b/src/lib.rs", "pub fn changed() {}\n");
}

fn selection(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    selected(output)
}

#[test]
fn a_push_to_the_default_branch_selects_what_the_commits_since_before_changed() {
    let repo = two_commits("main-push-docs", "", docs);
    assert_eq!(selection(&push(&repo, &rev(&repo, "HEAD^1"))), NOTHING);
    let repo = two_commits("main-push-app", "", app);
    assert_eq!(selection(&push(&repo, &rev(&repo, "HEAD^1"))), APP);
}

#[test]
fn a_push_selects_everything_when_before_is_unusable() {
    let repo = two_commits("main-push-before", "", app);
    repo.git(&["checkout", "--quiet", "-b", "side", "HEAD^1"]);
    repo.write("side.txt", "side\n");
    repo.commit();
    let side = rev(&repo, "HEAD");
    repo.git(&["checkout", "--quiet", "main"]);
    let missing = "1234567890abcdef1234567890abcdef12345678";
    for before in [ZEROS, side.as_str(), missing, "--output=x", "HEAD^1"] {
        assert_eq!(selection(&push(&repo, before)), EVERYTHING, "{before}");
    }
}

#[test]
fn a_push_selects_everything_when_head_is_not_the_pushed_commit() {
    let repo = two_commits("main-push-head", "", app);
    let before = rev(&repo, "HEAD^1");
    let payload = format!(
        "{{\"before\":\"{before}\",\"after\":\"{before}\",\"repository\":{{\"default_branch\":\"main\"}}}}"
    );
    let output = run(&repo, "push", &payload, &rev(&repo, "HEAD"));
    assert_eq!(selection(&output), EVERYTHING);
}

#[test]
fn a_push_runs_every_lane_when_on_main_is_everything() {
    let repo = two_commits("main-push-all", "on_main = \"everything\"\n", docs);
    let output = push(&repo, &rev(&repo, "HEAD^1"));
    assert_eq!(
        selection(&output),
        "Cargo.toml: all:event\ntools/Cargo.toml: all:event\n"
    );
}

#[test]
fn a_merge_group_selects_what_its_base_and_head_differ_by() {
    let repo = two_commits("main-group-docs", "", docs);
    let (base, head) = (rev(&repo, "HEAD^1"), rev(&repo, "HEAD"));
    assert_eq!(selection(&merge_group(&repo, &base, &head)), NOTHING);
    let repo = two_commits("main-group-app", "", app);
    let (base, head) = (rev(&repo, "HEAD^1"), rev(&repo, "HEAD"));
    assert_eq!(selection(&merge_group(&repo, &base, &head)), APP);
    let other = rev(&repo, "HEAD^1");
    assert_eq!(selection(&merge_group(&repo, &base, &other)), EVERYTHING);
}

#[test]
fn a_merge_group_runs_every_lane_when_on_main_is_everything() {
    let repo = two_commits("main-group-all", "on_main = \"everything\"\n", docs);
    let (base, head) = (rev(&repo, "HEAD^1"), rev(&repo, "HEAD"));
    assert_eq!(
        selection(&merge_group(&repo, &base, &head)),
        "Cargo.toml: all:event\ntools/Cargo.toml: all:event\n"
    );
}

const LOCK: &str = "version = 4\n\n[[package]]\nname = \"a\"\nversion = \"0.0.0\"\n\n[[package]]\nname = \"b\"\nversion = \"0.0.0\"\n";

fn lock_edit(name: &str, padding: &str) -> Repo {
    let repo = plan_repo(name);
    repo.write("Cargo.lock", &format!("{LOCK}{padding}"));
    repo.commit();
    repo.git(&["branch", "-M", "main"]);
    repo.write("Cargo.lock", &format!("{LOCK}{padding}# edited\n"));
    repo.commit();
    repo
}

#[test]
fn a_push_reads_the_base_lock_while_it_fits_and_selects_everything_past_the_byte_budget() {
    let small = lock_edit("main-push-lock-small", "# padding\n");
    assert_eq!(selection(&push(&small, &rev(&small, "HEAD^1"))), NOTHING);
    let large = lock_edit("main-push-lock-large", &"# padding\n".repeat(450_000));
    let text = selection(&push(&large, &rev(&large, "HEAD^1")));
    assert!(text.starts_with("Cargo.toml: all:"), "{text}");
}
