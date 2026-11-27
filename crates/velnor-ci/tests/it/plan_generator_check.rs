// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_lock, plan_repo};
use crate::plan_api::event;

const CONFIG: &str =
    "[[lint]]\nkind = 'generated-check'\nfrom_source = true\ngate_on_plan = true\n";
const WORKSPACE: &str =
    "[workspace]\nmembers = [\"a\", \"b\", \"gen\"]\nexclude = [\"tools\"]\nresolver = \"3\"\n";
const GENERATOR: &str = "[package]\nname = \"velnor-gen\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\n";

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

fn merged(name: &str, edit: impl FnOnce(&Repo)) -> Repo {
    let repo = plan_repo(name);
    repo.write_all(&[
        (".velnor/rust-ci.toml", CONFIG),
        ("Cargo.toml", WORKSPACE),
        ("Cargo.lock", &plan_lock("1.0.5")),
        ("gen/Cargo.toml", GENERATOR),
        ("gen/src/main.rs", "fn main() {}\n"),
        ("b/src/old.rs", "pub fn old() {}\n"),
        ("b/src/plain.rs", "pub fn plain() {}\n"),
        (
            "b/src/embeds.rs",
            "pub const X: &str = include_str!(\"../../docs/a.md\");\n",
        ),
        ("docs/readme.md", "docs\n"),
        ("docs/a.md", "a\n"),
    ]);
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

fn head(repo: &Repo) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn decision(repo: &Repo, changed: &[&str]) -> String {
    let outputs = repo.root().join("decision-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command.arg("plan");
    if !changed.is_empty() {
        command.arg("--changed").args(changed);
    }
    let output = command
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_EVENT_NAME", "pull_request")
        .env("GITHUB_EVENT_PATH", event(repo, "main"))
        .env("GITHUB_SHA", head(repo))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .find_map(|line| line.strip_prefix("lint_generated_check="))
        .unwrap_or("missing")
        .to_owned()
}

fn plain_edit(repo: &Repo) {
    repo.write("b/src/plain.rs", "pub fn plain() { let _ = 1; }\n");
}

#[test]
fn an_edited_plain_rust_file_outside_the_generator_skips() {
    let repo = merged("generator-skip", plain_edit);
    assert_eq!(decision(&repo, &[]), "skip");
}

#[test]
fn a_markdown_only_edit_skips() {
    let repo = merged("generator-skip-docs", |repo| {
        repo.write("docs/readme.md", "more docs\n");
    });
    assert_eq!(decision(&repo, &[]), "skip");
}

#[test]
fn an_added_rust_file_runs_the_generated_check() {
    let repo = merged("generator-added", |repo| {
        repo.write("b/src/new.rs", "pub fn new() {}\n");
    });
    assert_eq!(decision(&repo, &[]), "run");
}

#[test]
fn a_deleted_rust_file_runs_the_generated_check() {
    let repo = merged("generator-deleted", |repo| {
        repo.git(&["rm", "--quiet", "b/src/old.rs"]);
    });
    assert_eq!(decision(&repo, &[]), "run");
}

#[test]
fn a_renamed_rust_file_runs_the_generated_check() {
    let repo = merged("generator-renamed", |repo| {
        repo.git(&["mv", "b/src/old.rs", "b/src/moved.rs"]);
    });
    assert_eq!(decision(&repo, &[]), "run");
}

#[test]
fn an_edit_that_adds_an_include_runs_the_generated_check() {
    let repo = merged("generator-include-added", |repo| {
        repo.write(
            "b/src/plain.rs",
            "pub const X: &str = include_str!(\"../../docs/a.md\");\n",
        );
    });
    assert_eq!(decision(&repo, &[]), "run");
}

#[test]
fn an_edit_that_removes_an_include_runs_the_generated_check() {
    let repo = merged("generator-include-removed", |repo| {
        repo.write("b/src/embeds.rs", "pub const X: &str = \"a\";\n");
    });
    assert_eq!(decision(&repo, &[]), "run");
}

#[test]
fn an_explicit_changed_list_with_a_plain_rust_file_runs_the_generated_check() {
    let repo = merged("generator-changed", plain_edit);
    assert_eq!(decision(&repo, &["b/src/plain.rs"]), "run");
}
