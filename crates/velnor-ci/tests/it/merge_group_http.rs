// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fs;
use std::process::Command;

use serde_json::{Value, json};
use velnor_ci::github::{compare_path, merge_group_runs_path};

use crate::io::{Repo, serve_with_status};
use crate::support::repo;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const BEFORE: &str = "89abcdef0123456789abcdef0123456789abcdef";
const ZEROS: &str = "0000000000000000000000000000000000000000";
const WORKFLOW_REF: &str = "tailrocks/velnor-new/.github/workflows/ci.yml@refs/heads/main";

struct Outcome {
    skip: String,
    stdout: String,
}

fn files(names: &[&str]) -> Value {
    names.iter().map(|name| json!({"filename": name})).collect()
}

fn compare(status: &str, files: &Value) -> String {
    json!({"status": status, "total_count": 1, "files": files}).to_string()
}

fn queue_run() -> (String, (u16, String)) {
    let run = json!({"id": 1, "event": "merge_group", "head_sha": SHA, "status": "completed", "conclusion": "success"});
    (
        merge_group_runs_path(&repo(), "ci.yml", SHA),
        (
            200,
            json!({"total_count": 1, "workflow_runs": [run]}).to_string(),
        ),
    )
}

fn page(before: &str, reply: (u16, String)) -> (String, (u16, String)) {
    (compare_path(&repo(), before, SHA), reply)
}

fn ahead(names: &[&str]) -> (u16, String) {
    (200, compare("ahead", &files(names)))
}

fn payload(before: Option<&str>) -> String {
    match before {
        Some(before) => json!({"before": before, "after": SHA}).to_string(),
        None => json!({"after": SHA}).to_string(),
    }
}

fn run_step(pages: Vec<(String, (u16, String))>, event: Option<String>) -> Outcome {
    let mut responses: BTreeMap<String, (u16, String)> = pages.into_iter().collect();
    let (path, reply) = queue_run();
    responses.insert(path, reply);
    let api = serve_with_status(responses);
    let scratch = Repo::empty("merge-group-http");
    let output = scratch.root().join("github-output");
    let event_path = scratch.root().join("event.json");
    if let Some(event) = event {
        fs::write(&event_path, event).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let ran = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("merge-group-passed")
        .current_dir(scratch.root())
        .env_remove("VELNOR_REQUIRED_CHECK")
        .env_remove("GITHUB_WORKSPACE")
        .env("GITHUB_API_URL", api)
        .env("GITHUB_TOKEN", "recorded")
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", SHA)
        .env("GITHUB_WORKFLOW_REF", WORKFLOW_REF)
        .env("GITHUB_EVENT_PATH", &event_path)
        .env("GITHUB_OUTPUT", &output)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(ran.status.code(), Some(0), "{ran:?}");
    Outcome {
        skip: fs::read_to_string(&output).unwrap_or_default(),
        stdout: String::from_utf8_lossy(&ran.stdout).into_owned(),
    }
}

fn push(pages: Vec<(String, (u16, String))>) -> Outcome {
    run_step(pages, Some(payload(Some(BEFORE))))
}

fn assert_everything(outcome: &Outcome) {
    assert_eq!(outcome.skip, "skip=false\n", "{}", outcome.stdout);
}

#[test]
fn a_multi_commit_push_whose_earlier_commit_bumps_the_lock_runs_everything() {
    let outcome = push(vec![page(BEFORE,
        (
            200,
            json!({"status": "ahead", "total_commits": 3, "files": files(&["src/lib.rs", "Cargo.lock"])}).to_string(),
        ),
    )]);
    assert_everything(&outcome);
    assert!(
        outcome.stdout.contains("Cargo.lock changed"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_nested_lock_runs_everything() {
    assert_everything(&push(vec![page(
        BEFORE,
        ahead(&["tools/xtask/Cargo.lock"]),
    )]));
}

#[test]
fn a_removed_lock_runs_everything() {
    let removed = json!([{"filename": "Cargo.lock", "status": "removed"}]);
    assert_everything(&push(vec![page(BEFORE, (200, compare("ahead", &removed)))]));
}

#[test]
fn a_rename_away_from_a_lock_runs_everything() {
    let renamed = json!([{"filename": "locks/old.txt", "previous_filename": "Cargo.lock", "status": "renamed"}]);
    assert_everything(&push(vec![page(BEFORE, (200, compare("ahead", &renamed)))]));
}

#[test]
fn a_lock_listed_last_of_299_files_runs_everything() {
    let mut names: Vec<String> = (0..298).map(|n| format!("src/file{n}.rs")).collect();
    names.push("Cargo.lock".to_owned());
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    assert_everything(&push(vec![page(BEFORE, ahead(&names))]));
}

#[test]
fn a_zero_before_runs_everything() {
    let clean = page(ZEROS, ahead(&["README.md"]));
    assert_everything(&run_step(vec![clean], Some(payload(Some(ZEROS)))));
}

#[test]
fn a_missing_before_or_payload_runs_everything() {
    let clean = page("", ahead(&["README.md"]));
    assert_everything(&run_step(vec![clean.clone()], Some(payload(None))));
    assert_everything(&run_step(vec![clean], None));
}

#[test]
fn a_compare_api_error_runs_everything() {
    let broken = page(BEFORE, (500, "{}".to_owned()));
    assert_everything(&push(vec![broken]));
}

#[test]
fn a_compare_that_is_not_ahead_runs_everything() {
    for status in ["diverged", "behind", "identical"] {
        let reply = (200, compare(status, &files(&["README.md"])));
        assert_everything(&push(vec![page(BEFORE, reply)]));
    }
}

#[test]
fn a_file_list_of_300_files_may_be_truncated_and_runs_everything() {
    let names: Vec<String> = (0..300).map(|n| format!("src/file{n}.rs")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let outcome = push(vec![page(BEFORE, ahead(&names))]);
    assert_everything(&outcome);
    assert!(
        outcome.stdout.contains("300 files or more"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn a_push_that_leaves_every_lock_unchanged_still_skips() {
    let names = [
        "README.md",
        "src/Cargo.lock.bak",
        "not-Cargo.lock",
        "Cargo.lock.d/x.rs",
    ];
    let outcome = push(vec![page(BEFORE, ahead(&names))]);
    assert_eq!(outcome.skip, "skip=true\n", "{}", outcome.stdout);
}

#[test]
fn a_path_in_the_note_cannot_inject_a_workflow_command() {
    let hostile = "evil\n::error::injected/Cargo.lock";
    let outcome = push(vec![page(BEFORE, ahead(&[hostile]))]);
    assert_everything(&outcome);
    assert!(
        !outcome.stdout.lines().any(|line| line.starts_with("::")),
        "{}",
        outcome.stdout
    );
    assert!(outcome.stdout.contains("%0A"), "{}", outcome.stdout);
}

#[test]
fn an_ahead_compare_without_a_file_list_runs_everything() {
    let bare = (200, json!({"status": "ahead"}).to_string());
    assert_everything(&push(vec![page(BEFORE, bare)]));
}
