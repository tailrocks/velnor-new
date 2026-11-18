// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Command;

use crate::io::Repo;
use crate::io_plan::plan_command;
use crate::plan_main::{app, docs, rev, two_commits};
use crate::support::fresh_dir;

const CONFIG: &str = "[[test_groups]]
name = 'evt'
filter = 'package(b)'
events = ['schedule']
root = '.'

[[test_groups]]
name = 'touched'
filter = 'package(b)'
paths = ['b/**']
root = '.'

[[test_groups]]
name = 'untouched'
filter = 'package(a)'
paths = ['a/migrations/**']
events = ['schedule']
root = '.'
";

const SPEC: &str = "--plan rust-plan --job rust-plan=always --job rust-test-evt=lane:root+group:root:evt --job rust-test-touched=lane:root+group:root:touched --job rust-test-untouched=lane:root+group:root:untouched";
const GROUPS: [&str; 3] = ["evt", "touched", "untouched"];

struct Planned {
    outputs: String,
}

impl Planned {
    fn group(&self, name: &str) -> &str {
        let key = format!("rust_root_group_{name}=");
        self.outputs
            .lines()
            .find_map(|line| line.strip_prefix(&key))
            .unwrap_or("missing")
    }

    fn value(&self, key: &str) -> &str {
        let prefix = format!("{key}=");
        self.outputs
            .lines()
            .find_map(|line| line.strip_prefix(&prefix))
            .unwrap_or("missing")
    }

    fn needs(&self, results: &[(&str, &str)]) -> String {
        let outputs: Vec<String> = self
            .outputs
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| format!("\"{key}\":\"{value}\""))
            .collect();
        let mut needs = vec![format!(
            "\"rust-plan\":{{\"result\":\"success\",\"outputs\":{{{}}}}}",
            outputs.join(",")
        )];
        needs.extend(results.iter().map(|(name, result)| {
            format!("\"rust-test-{name}\":{{\"result\":\"{result}\",\"outputs\":{{}}}}")
        }));
        format!("{{{}}}", needs.join(","))
    }
}

fn plan(repo: &Repo, name: &str, payload: &str) -> Planned {
    let dir = fresh_dir("velnor-ci-groups", name);
    let (event, outputs) = (dir.join("event.json"), dir.join("outputs"));
    fs::write(&event, payload).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let output = plan_command(repo)
        .arg("plan")
        .env("GITHUB_EVENT_NAME", name)
        .env("GITHUB_EVENT_PATH", &event)
        .env("GITHUB_OUTPUT", &outputs)
        .env("GITHUB_SHA", rev(repo, "HEAD"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    Planned {
        outputs: fs::read_to_string(&outputs).unwrap_or_else(|e| unreachable!("{e}")),
    }
}

fn merge_group(repo: &Repo, base: &str, head: &str) -> Planned {
    let payload = format!(
        "{{\"merge_group\":{{\"base_sha\":\"{base}\",\"head_sha\":\"{head}\"}},\"repository\":{{\"default_branch\":\"main\"}}}}"
    );
    plan(repo, "merge_group", &payload)
}

fn push(repo: &Repo) -> Planned {
    let (before, head) = (rev(repo, "HEAD^1"), rev(repo, "HEAD"));
    let payload = format!(
        "{{\"before\":\"{before}\",\"after\":\"{head}\",\"repository\":{{\"default_branch\":\"main\"}}}}"
    );
    plan(repo, "push", &payload)
}

fn gate(planned: &Planned, results: &[(&str, &str)]) -> (Option<i32>, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["gate", "--spec-env", "GATE_SPEC"])
        .env("GATE_SPEC", SPEC)
        .env("NEEDS_JSON", planned.needs(results))
        .env("GITHUB_EVENT_NAME", "merge_group")
        .env_remove("GITHUB_STEP_SUMMARY")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn expect(planned: &Planned, wanted: [&str; 3]) {
    let seen: Vec<&str> = GROUPS.iter().map(|name| planned.group(name)).collect();
    assert_eq!(seen, wanted, "{}", planned.outputs);
    let results: Vec<(&str, &str)> = GROUPS
        .iter()
        .zip(wanted)
        .map(|(name, plan)| (*name, if plan == "run" { "success" } else { "skipped" }))
        .collect();
    let (code, table, stderr) = gate(planned, &results);
    assert_eq!(code, Some(0), "{table}{stderr}");
    for (name, plan) in GROUPS.iter().zip(wanted) {
        let row = if plan == "run" {
            format!("| rust-test-{name} | scheduled | success |")
        } else {
            format!("| rust-test-{name} | not scheduled | skipped |")
        };
        assert!(table.contains(&row), "{table}");
    }
    for (name, plan) in GROUPS.iter().zip(wanted).filter(|(_, plan)| *plan == "run") {
        let flipped: Vec<(&str, &str)> = results
            .iter()
            .map(|(other, result)| (*other, if other == name { "skipped" } else { result }))
            .collect();
        let (code, _, stderr) = gate(planned, &flipped);
        assert_eq!(code, Some(1), "{name} {plan}: {stderr}");
        assert!(
            stderr.contains(&format!("rust-test-{name} was scheduled by the plan")),
            "{stderr}"
        );
    }
}

#[test]
fn a_merge_group_runs_a_group_that_only_names_events() {
    let repo = two_commits("groups-mg-events", CONFIG, app);
    let planned = merge_group(&repo, &rev(&repo, "HEAD^1"), &rev(&repo, "HEAD"));
    assert_eq!(planned.group("evt"), "run", "{}", planned.outputs);
    expect(&planned, ["run", "run", "skip"]);
}

#[test]
fn a_merge_group_skips_a_group_whose_paths_the_diff_does_not_touch() {
    let repo = two_commits("groups-mg-untouched", CONFIG, app);
    let planned = merge_group(&repo, &rev(&repo, "HEAD^1"), &rev(&repo, "HEAD"));
    assert_eq!(planned.group("untouched"), "skip", "{}", planned.outputs);
    expect(&planned, ["run", "run", "skip"]);
}

#[test]
fn a_merge_group_runs_a_group_whose_paths_the_diff_touches() {
    let repo = two_commits("groups-mg-touched", CONFIG, |repo| {
        app(repo);
        repo.write("a/migrations/0001.sql", "select 1;\n");
    });
    let planned = merge_group(&repo, &rev(&repo, "HEAD^1"), &rev(&repo, "HEAD"));
    expect(&planned, ["run", "run", "run"]);
}

#[test]
fn a_merge_group_whose_diff_cannot_be_computed_runs_every_group() {
    let repo = two_commits("groups-mg-unknown", CONFIG, app);
    let base = rev(&repo, "HEAD^1");
    let planned = merge_group(&repo, &base, &base);
    expect(&planned, ["run", "run", "run"]);
}

#[test]
fn a_push_to_main_runs_every_group_whatever_its_paths_or_events() {
    let repo = two_commits("groups-push", CONFIG, app);
    expect(&push(&repo), ["run", "run", "run"]);
}

#[test]
fn on_a_push_to_main_the_groups_follow_the_selection_of_the_default_lanes() {
    let affected = two_commits(
        "groups-push-docs-affected",
        &format!("on_main = \"affected\"\n{CONFIG}"),
        docs,
    );
    let planned = push(&affected);
    assert_eq!(
        planned.value("rust_root_mode"),
        "none",
        "{}",
        planned.outputs
    );
    assert_eq!(planned.value("rust_root_tests"), "skip");
    expect(&planned, ["skip", "skip", "skip"]);
    let everything = two_commits(
        "groups-push-docs-everything",
        &format!("on_main = \"everything\"\n{CONFIG}"),
        docs,
    );
    let planned = push(&everything);
    assert_eq!(
        planned.value("rust_root_mode"),
        "all",
        "{}",
        planned.outputs
    );
    assert_eq!(planned.value("rust_root_tests"), "run");
    expect(&planned, ["run", "run", "run"]);
}
