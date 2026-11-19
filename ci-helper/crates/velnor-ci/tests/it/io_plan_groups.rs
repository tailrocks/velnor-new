// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::io::Repo;
use crate::io_plan::{plan_command, plan_repo};

const CONFIG: &str = "[[test_groups]]
name = 'database'
filter = 'package(a) and test(~db_)'
needs = ['docker']
paths = ['a/migrations/**']
events = ['push', 'schedule']
root = '.'

[[test_groups]]
name = 'always'
filter = 'package(b)'
root = '.'
";

struct Planned {
    outputs: String,
    summary: String,
    explained: String,
}

fn repo(name: &str) -> Repo {
    let repo = plan_repo(name);
    repo.write_all(&[(".velnor/rust-ci.toml", CONFIG)]);
    repo
}

fn run(name: &str, changed: &[&str], event: Option<&str>) -> Planned {
    let repo = repo(name);
    let outputs = repo.root().join("github-output");
    let summary = repo.root().join("step-summary");
    let go = |explain: bool| {
        let mut command = plan_command(&repo);
        command
            .arg("plan")
            .args(["--changed"])
            .args(changed)
            .env("GITHUB_OUTPUT", &outputs)
            .env("GITHUB_STEP_SUMMARY", &summary);
        if explain {
            command.arg("--explain");
        }
        if let Some(event) = event {
            command.env("GITHUB_EVENT_NAME", event);
        }
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).into_owned()
    };
    let explained = go(true);
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&summary, "").unwrap_or_else(|e| unreachable!("{e}"));
    go(false);
    Planned {
        outputs: fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}")),
        summary: fs::read_to_string(&summary).unwrap_or_else(|e| unreachable!("{e}")),
        explained,
    }
}

fn group_lines(outputs: &str) -> Vec<&str> {
    outputs
        .lines()
        .filter(|line| line.contains("_group_"))
        .collect()
}

#[test]
fn plan_outputs_one_run_or_skip_per_group_and_root() {
    let skipped = run("groups-skip", &["a/src/lib.rs"], None);
    assert_eq!(
        group_lines(&skipped.outputs),
        [
            "rust_root_group_database=skip",
            "rust_root_group_always=run"
        ]
    );
    let by_path = run("groups-path", &["a/migrations/0001.sql"], None);
    assert_eq!(
        group_lines(&by_path.outputs),
        ["rust_root_group_database=run", "rust_root_group_always=run"]
    );
    let by_event = run("groups-event", &["a/src/lib.rs"], Some("schedule"));
    assert!(
        group_lines(&by_event.outputs).contains(&"rust_root_group_database=run"),
        "{}",
        by_event.outputs
    );
}

#[test]
fn nothing_selected_runs_no_group_and_the_other_root_has_no_group_lines() {
    let none = run("groups-none", &["docs/goals.md"], None);
    assert_eq!(
        group_lines(&none.outputs),
        [
            "rust_root_group_database=skip",
            "rust_root_group_always=skip"
        ]
    );
}

#[test]
fn the_explanation_and_the_summary_say_why_each_group_ran_or_was_skipped() {
    let skipped = run("groups-explain", &["a/src/lib.rs"], None);
    assert!(
        skipped.explained.contains(
            "test group database (Cargo.toml): skip: event pull_request is not listed in events and no changed path matches paths\n"
        ),
        "{}",
        skipped.explained
    );
    assert!(
        skipped.explained.contains(
            "test group always (Cargo.toml): run: paths and events are not set, so the group runs with its root\n"
        ),
        "{}",
        skipped.explained
    );
    assert!(
        skipped.summary.contains(
            "| database | Cargo.toml | skip | event pull_request is not listed in events and no changed path matches paths |"
        ),
        "{}",
        skipped.summary
    );
    let by_path = run("groups-summary", &["a/migrations/0001.sql"], None);
    assert!(
        by_path.summary.contains(
            "| database | Cargo.toml | run | changed path a/migrations/0001.sql matches paths a/migrations/\\*\\* |"
        ),
        "{}",
        by_path.summary
    );
}
