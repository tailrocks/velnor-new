// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo};

fn run(changed: &str, event: Option<&str>, name: &str) -> (String, String) {
    let repo = plan_repo(name);
    let outputs = repo.root().join("github-output");
    let summary = repo.root().join("step-summary");
    let mut command = plan_command(&repo);
    command
        .args(["plan", "--changed", changed])
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_STEP_SUMMARY", &summary);
    if let Some(event) = event {
        command.env("GITHUB_EVENT_NAME", event);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    (
        fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}")),
        fs::read_to_string(&summary).unwrap_or_else(|e| unreachable!("{e}")),
    )
}

#[test]
fn the_summary_lists_each_selected_package_with_its_reason_and_lanes() {
    let (_, summary) = run("a/src/lib.rs", None, "summary-some");
    assert_eq!(
        summary,
        "Cargo.toml: some packages\n\n| Package | Reason | Lanes |\n|---|---|---|\n| a | changed:a/src/lib.rs | lint, test |\n\n**all:most-packages:1** (tools/Cargo.toml)\n\n| Package | Reason | Lanes |\n|---|---|---|\n| t | depends-on:a | lint, test, docs |\n\n"
    );
}

#[test]
fn a_root_that_selects_everything_starts_with_its_trigger_in_bold() {
    let (_, summary) = run("rust-toolchain.toml", None, "summary-all");
    assert!(
        summary.starts_with("**all:toolchain-file:rust-toolchain.toml** (Cargo.toml)\n\n"),
        "{summary}"
    );
    assert!(summary.contains("| a | every package runs | lint, test |"));
}

#[test]
fn a_root_that_selects_nothing_says_so_and_expects_no_tests() {
    let (outputs, summary) = run("docs/goals.md", None, "summary-none");
    assert!(summary.contains("no package selected"), "{summary}");
    assert!(outputs.contains("rust_root_tests=skip\n"), "{outputs}");
    assert!(outputs.contains("rust_tools_tests=skip\n"), "{outputs}");
}

#[test]
fn a_selected_root_expects_its_tests() {
    let (outputs, _) = run("a/src/lib.rs", None, "summary-tests");
    assert!(outputs.contains("rust_root_tests=run\n"), "{outputs}");
}

#[test]
fn plan_says_a_doctest_is_known_only_when_a_fenced_example_was_found() {
    let repo = plan_repo("doctests-known");
    let known = |repo: &Repo| {
        let outputs = repo.root().join("known");
        fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
        let output = plan_command(repo)
            .args(["plan", "--changed", "rust-toolchain.toml"])
            .env("GITHUB_OUTPUT", &outputs)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        fs::read_to_string(&outputs)
            .unwrap_or_else(|e| unreachable!("{e}"))
            .lines()
            .filter(|line| line.contains("_doctests"))
            .flat_map(|line| [line, " "])
            .collect::<String>()
    };
    repo.write("a/src/lib.rs", "/// Adds one.\npub fn f() {}\n");
    assert_eq!(
        known(&repo),
        "rust_root_doctests=skip rust_root_doctests_known=false rust_tools_doctests=skip rust_tools_doctests_known=false "
    );
    repo.write(
        "tools/Cargo.toml",
        "[package]\nname = \"t\"\nedition = \"2024\"\n\n[dependencies]\na = { path = \"../a\" }\nserde_derive = \"1\"\n",
    );
    repo.write(
        "a/src/lib.rs",
        "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n",
    );
    assert!(
        known(&repo).contains("rust_root_doctests=run rust_root_doctests_known=true "),
        "{}",
        known(&repo)
    );
}
