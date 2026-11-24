// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::support::fresh_dir;

const RECORDED: &str = include_str!("data/qualification_runs.tsv");

fn needs(hosted: &str, scale_set: &str) -> String {
    format!(
        r#"{{"verify-hosted":{{"result":"{hosted}","outputs":{{}}}},"verify-scale-set":{{"result":"{scale_set}","outputs":{{}}}}}}"#
    )
}

fn compare(label: &str, needs: Option<&str>, extra: &[&str]) -> (Output, String) {
    let dir = fresh_dir("velnor-ci-qual", label);
    let summary = dir.join("summary");
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["qualification", "compare"])
        .args(extra)
        .env_remove("NEEDS_JSON")
        .env("GITHUB_STEP_SUMMARY", &summary);
    if let Some(needs) = needs {
        command.env("NEEDS_JSON", needs);
    }
    let output = command.output().unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&summary).unwrap_or_default())
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn two_successful_lanes_pass_and_the_table_reaches_the_step_summary() {
    let (output, summary) = compare("pass", Some(&needs("success", "success")), &[]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout, summary);
    assert!(
        stdout.contains("**Qualification compare: pass**"),
        "{stdout}"
    );
    assert!(stdout.contains("| verify-hosted | success |"), "{stdout}");
    assert!(
        stdout.contains("| verify-scale-set | success |"),
        "{stdout}"
    );
}

fn recorded(run: &str) {
    let line = RECORDED
        .lines()
        .find(|line| line.starts_with(run))
        .unwrap_or_else(|| unreachable!("{run}"));
    let [_, hosted, scale_set, legacy] = line.split('\t').collect::<Vec<_>>()[..] else {
        unreachable!("{line}");
    };
    let (output, _) = compare(run, Some(&needs(hosted, scale_set)), &[]);
    assert_eq!(output.status.success(), legacy == "success", "{output:?}");
}

#[test]
fn the_recorded_passing_run_passes() {
    recorded("37103831130");
}

#[test]
fn recorded_runs_with_a_cancelled_scale_set_lane_fail() {
    recorded("37311402761");
    recorded("40373809214");
}

#[test]
fn a_recorded_run_with_every_lane_skipped_fails() {
    recorded("44063585313");
}

fn lane_refused(label: &str, hosted: &str, scale_set: &str, lane: &str, result: &str) {
    let (output, summary) = compare(label, Some(&needs(hosted, scale_set)), &[]);
    assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
    let error = stderr(&output);
    assert!(
        error.contains(&format!("{lane} result is {result}")),
        "{label}: {error}"
    );
    assert!(
        summary.contains("**Qualification compare: fail**"),
        "{summary}"
    );
}

#[test]
fn a_failed_hosted_lane_fails_the_compare() {
    lane_refused(
        "failed-hosted",
        "failure",
        "success",
        "verify-hosted",
        "failure",
    );
}

#[test]
fn a_failed_scale_set_lane_fails_the_compare() {
    lane_refused(
        "failed-scale",
        "success",
        "failure",
        "verify-scale-set",
        "failure",
    );
}

#[test]
fn a_cancelled_lane_fails_the_compare() {
    lane_refused(
        "cancelled",
        "success",
        "cancelled",
        "verify-scale-set",
        "cancelled",
    );
}

#[test]
fn a_skipped_lane_fails_the_compare() {
    lane_refused("skipped", "skipped", "success", "verify-hosted", "skipped");
}

#[test]
fn an_unknown_lane_result_fails_the_compare() {
    lane_refused("unknown", "success", "weird", "verify-scale-set", "weird");
}

fn refused(label: &str, input: Option<&str>, reason: &str) {
    let (output, _) = compare(label, input, &[]);
    assert_eq!(output.status.code(), Some(1), "{label}: {output:?}");
    assert!(
        stderr(&output).contains(reason),
        "{label}: {}",
        stderr(&output)
    );
}

#[test]
fn an_unset_or_empty_needs_context_is_refused() {
    refused("unset", None, "NEEDS_JSON is not set");
    refused("empty", Some(""), "NEEDS_JSON is not set");
}

#[test]
fn a_needs_context_that_is_not_an_object_is_refused() {
    refused(
        "not-json",
        Some("not json"),
        "NEEDS_JSON is not the needs context",
    );
    refused("array", Some("[]"), "NEEDS_JSON is not the needs context");
}

#[test]
fn a_lane_missing_from_needs_is_refused() {
    let input = r#"{"verify-hosted":{"result":"success","outputs":{}}}"#;
    refused(
        "missing-lane",
        Some(input),
        "verify-scale-set is missing from needs",
    );
}

#[test]
fn a_lane_without_a_result_is_refused() {
    let input = r#"{"verify-hosted":{"result":null},"verify-scale-set":{"result":"success"}}"#;
    refused(
        "null-result",
        Some(input),
        "verify-hosted result is missing",
    );
}

#[test]
fn lanes_are_chosen_with_lane_flags_and_none_are_refused() {
    let input = r#"{"a":{"result":"success"},"b":{"result":"success"}}"#;
    let (output, _) = compare("custom", Some(input), &["--lane", "a", "--lane", "b"]);
    assert!(output.status.success(), "{output:?}");
    let (output, _) = compare("bad-id", Some(input), &["--lane", "has space"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
}
