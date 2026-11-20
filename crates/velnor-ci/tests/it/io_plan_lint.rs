// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo};

const CONFIG: &str = "[[lint]]\nkind = 'mise-task'\nid = 'hk'\ntask = 'hk'\ninputs = ['hk.pkl', 'mise.toml']\n\n[[lint]]\nkind = 'mise-task'\nid = 'plain'\ntask = 'hk'\n";

fn lint_repo(name: &str) -> Repo {
    let repo = plan_repo(name);
    repo.write_all(&[
        (".velnor/rust-ci.toml", CONFIG),
        (
            "mise.toml",
            "[tools]\nrust = \"1.99.1\"\n\n[tasks.hk]\nrun = \"true\"\n",
        ),
    ]);
    repo
}

fn lint_lines(repo: &Repo, event: Option<&str>, changed: &[&str]) -> String {
    let outputs = repo.root().join("lint-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let mut command = plan_command(repo);
    command.arg("plan");
    if !changed.is_empty() {
        command.arg("--changed").args(changed);
    }
    command.env("GITHUB_OUTPUT", &outputs);
    if let Some(event) = event {
        command.env("GITHUB_EVENT_NAME", event);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    fs::read_to_string(&outputs)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .lines()
        .filter(|line| line.starts_with("lint_"))
        .flat_map(|line| [line, " "])
        .collect()
}

#[test]
fn plan_skips_a_lint_on_a_pull_request_that_changes_none_of_its_inputs() {
    let repo = lint_repo("lint-skip");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs"]),
        "lint_hk=skip "
    );
}

#[test]
fn plan_runs_a_lint_on_a_pull_request_that_changes_one_of_its_inputs() {
    let repo = lint_repo("lint-run");
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["a/src/lib.rs", "hk.pkl"]),
        "lint_hk=run "
    );
    assert_eq!(
        lint_lines(&repo, Some("pull_request"), &["mise.toml"]),
        "lint_hk=run "
    );
}

#[test]
fn plan_runs_a_lint_on_push_merge_group_schedule_and_dispatch() {
    let repo = lint_repo("lint-events");
    for event in ["push", "merge_group", "schedule", "workflow_dispatch"] {
        assert_eq!(
            lint_lines(&repo, Some(event), &["a/src/lib.rs"]),
            "lint_hk=run ",
            "{event}"
        );
    }
}

#[test]
fn plan_runs_a_lint_when_it_cannot_list_the_changed_files() {
    let repo = lint_repo("lint-open");
    assert_eq!(lint_lines(&repo, Some("pull_request"), &[]), "lint_hk=run ");
}

#[test]
fn plan_writes_no_lint_output_for_a_repository_without_lint_inputs() {
    let repo = plan_repo("lint-none");
    assert_eq!(lint_lines(&repo, Some("pull_request"), &["README.md"]), "");
}
