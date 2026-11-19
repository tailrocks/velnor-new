// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `velnor-ci write-request` against what the legacy `write-request-v1` op wrote for the
//! same inputs. `data/write_request/<case>/` holds the inputs and the recorded result
//! (`expected`, or `expected_error`); `record.sh` re-records it from the engine.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::support::fresh_dir;

fn cases() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/it/data/write_request");
    let mut dirs: Vec<PathBuf> = fs::read_dir(root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()))
}

/// Run write-request for a case, giving every context value as an argument.
fn run_case(case: &Path, temp: &Path, by_flags: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .env_clear()
        .env("RUNNER_TEMP", temp)
        .arg("write-request");
    let context = read(&case.join("context"));
    let pairs: Vec<(&str, &str)> = context.lines().filter_map(|l| l.split_once('=')).collect();
    let event = read(&case.join("event_name"));
    let payload = case.join("payload.json");
    let flags = [
        ("sha", "GITHUB_SHA", "--sha"),
        ("repository", "GITHUB_REPOSITORY", "--repository"),
        ("ref", "GITHUB_REF", "--ref"),
        ("ref_protected", "GITHUB_REF_PROTECTED", "--ref-protected"),
        ("workflow_ref", "GITHUB_WORKFLOW_REF", "--workflow-ref"),
        ("workflow_sha", "GITHUB_WORKFLOW_SHA", "--workflow-sha"),
        ("run_id", "GITHUB_RUN_ID", "--run-id"),
        ("run_attempt", "GITHUB_RUN_ATTEMPT", "--run-attempt"),
    ];
    if by_flags {
        command
            .args(["--event-name", &event])
            .arg("--event-path")
            .arg(&payload);
    } else {
        command
            .env("GITHUB_EVENT_NAME", &event)
            .env("GITHUB_EVENT_PATH", &payload);
    }
    for (key, variable, flag) in flags {
        if let Some((_, value)) = pairs.iter().find(|(name, _)| *name == key) {
            if by_flags {
                command.args([flag, value]);
            } else {
                command.env(variable, value);
            }
        }
    }
    for (file, flag) in [
        ("named_check_lanes.json", "--named-check-lanes"),
        ("lane_inventory.json", "--lane-inventory"),
    ] {
        if case.join(file).is_file() {
            command.args([flag, &read(&case.join(file))]);
        }
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn check(case: &Path, by_flags: bool) {
    let name = case.file_name().and_then(|n| n.to_str()).unwrap_or("case");
    let temp = fresh_dir("velnor-ci-write-request", name);
    let output = run_case(case, &temp, by_flags);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let request = temp.join("velnor/request/plan-v1-request.json");
    if case.join("expected").is_file() {
        assert!(output.status.success(), "{name}: {stderr}");
        assert_eq!(
            fs::read(&request).unwrap_or_else(|e| unreachable!("{name}: {e}")),
            fs::read(case.join("expected")).unwrap_or_else(|e| unreachable!("{e}")),
            "{name}"
        );
    } else {
        let code = read(&case.join("expected_error"));
        assert!(!output.status.success(), "{name} wrote a request");
        assert!(
            stderr
                .trim_end()
                .ends_with(&format!("internal request failed: {code}")),
            "{name}: expected {code}, got {stderr}"
        );
        assert!(!request.exists(), "{name} left a request file");
    }
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn every_recorded_case_matches_the_legacy_op_with_the_context_in_the_environment() {
    let cases = cases();
    assert!(cases.len() >= 40, "{} cases", cases.len());
    for case in &cases {
        check(case, false);
    }
}

#[test]
fn every_recorded_case_matches_the_legacy_op_with_the_context_as_arguments() {
    for case in &cases() {
        check(case, true);
    }
}

#[test]
fn the_five_diff_shapes_are_recorded() {
    let names: Vec<String> = cases()
        .iter()
        .filter_map(|case| case.file_name()?.to_str().map(str::to_owned))
        .collect();
    for shape in [
        "push-rust-only",
        "push-docs-only",
        "push-workflow-change",
        "push-homebrew-formula",
        "dispatch-qualification-cold",
    ] {
        assert!(names.iter().any(|name| name == shape), "{shape}");
    }
}

fn plain_case(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/it/data/write_request")
        .join(name)
}

#[test]
fn an_existing_request_is_never_overwritten() {
    let case = plain_case("push-rust-only");
    let temp = fresh_dir("velnor-ci-write-request", "twice");
    assert!(run_case(&case, &temp, false).status.success());
    let first = fs::read(temp.join("velnor/request/plan-v1-request.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let again = run_case(&plain_case("push-docs-only"), &temp, false);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("request_exists"));
    assert_eq!(
        fs::read(temp.join("velnor/request/plan-v1-request.json"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        first
    );
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_request_outside_runner_temp_or_through_a_symlink_is_refused() {
    let case = plain_case("push-rust-only");
    let temp = fresh_dir("velnor-ci-write-request", "escape");
    let elsewhere = fresh_dir("velnor-ci-write-request", "elsewhere");
    let outside = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_EVENT_NAME", read(&case.join("event_name")))
        .env("GITHUB_EVENT_PATH", case.join("payload.json"))
        .args(["write-request", "--output"])
        .arg(elsewhere.join("request.json"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(String::from_utf8_lossy(&outside.stderr).contains("anchor_escape"));
    std::os::unix::fs::symlink(&elsewhere, temp.join("velnor"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let linked = run_case(&case, &temp, false);
    assert!(!linked.status.success());
    assert!(String::from_utf8_lossy(&linked.stderr).contains("symlink_refused"));
    assert!(!elsewhere.join("request").exists());
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&elsewhere).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_payload_that_is_a_symlink_is_refused() {
    let temp = fresh_dir("velnor-ci-write-request", "payload-link");
    let link = temp.join("event.json");
    std::os::unix::fs::symlink(plain_case("push-rust-only").join("payload.json"), &link)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .env_clear()
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_EVENT_NAME", "push")
        .env("GITHUB_EVENT_PATH", &link)
        .arg("write-request")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unreadable_event_payload"));
    fs::remove_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn no_runner_temp_or_payload_is_named_in_the_error() {
    for (temp, event_path, code) in [
        (false, true, "missing_runner_temp"),
        (true, false, "missing_event_payload"),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command.env_clear().arg("write-request");
        if temp {
            command.env("RUNNER_TEMP", std::env::temp_dir());
        }
        if event_path {
            command.env("GITHUB_EVENT_PATH", "/nonexistent");
        }
        let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{code}"
        );
    }
}
