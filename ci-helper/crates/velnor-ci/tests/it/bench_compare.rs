// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use velnor_ci::io::write_metrics;
use velnor_ci::metrics::RunMetrics;

use crate::support::{fresh_dir, recorded_metrics};

const EVENT_CAVEAT: &str = "> Different workflow or event populations: A is the `CI` workflow on `push`, B is the `CI` workflow on `pull_request`. The workloads differ, so these numbers are not like for like.";

fn metrics_file(directory: &Path, name: &str, metrics: &RunMetrics) -> PathBuf {
    let path = directory.join(name);
    write_metrics(&path, metrics).unwrap_or_else(|e| unreachable!("{e}"));
    path
}

fn compare(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("bench")
        .arg("compare")
        .args(arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn runs_of_different_events_are_compared_when_the_caller_opts_in() {
    let directory = fresh_dir("velnor-ci-bench-compare", "events");
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.event = "pull_request".to_owned();
    let first = metrics_file(&directory, "push.json", &a);
    let second = metrics_file(&directory, "pull-request.json", &b);

    let output = compare(&[
        first.to_str().unwrap_or_else(|| unreachable!()),
        second.to_str().unwrap_or_else(|| unreachable!()),
        "--allow-different-populations",
    ]);

    let stdout = stdout_of(&output);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(stdout.contains(EVENT_CAVEAT), "{stdout}");
    assert!(
        stdout.contains("| Wall seconds | 963 | 963 | +0 |"),
        "{stdout}"
    );
}

#[test]
fn runs_of_different_workflows_are_compared_when_the_caller_opts_in() {
    let directory = fresh_dir("velnor-ci-bench-compare", "workflows");
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.workflow = "Release".to_owned();
    let first = metrics_file(&directory, "ci.json", &a);
    let second = metrics_file(&directory, "release.json", &b);

    let output = compare(&[
        first.to_str().unwrap_or_else(|| unreachable!()),
        second.to_str().unwrap_or_else(|| unreachable!()),
        "--allow-different-populations",
    ]);

    let stdout = stdout_of(&output);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(
        stdout.contains("B is the `Release` workflow on `push`"),
        "{stdout}"
    );
    assert!(
        stdout.contains("| Runner seconds | 936 | 936 | +0 |"),
        "{stdout}"
    );
}

#[test]
fn the_opt_in_leaves_a_comparison_of_the_same_population_unchanged() {
    let directory = fresh_dir("velnor-ci-bench-compare", "same");
    let a = recorded_metrics();
    let first = metrics_file(&directory, "first.json", &a);
    let second = metrics_file(&directory, "second.json", &a);

    let output = compare(&[
        first.to_str().unwrap_or_else(|| unreachable!()),
        second.to_str().unwrap_or_else(|| unreachable!()),
        "--allow-different-populations",
    ]);

    let stdout = stdout_of(&output);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(stdout.contains("| Metric | A | B | Change |"), "{stdout}");
    assert!(!stdout.contains("not like for like"), "{stdout}");
}

#[test]
fn runs_of_different_repositories_are_not_compared_across_populations() {
    let directory = fresh_dir("velnor-ci-bench-compare", "repositories");
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.repo = "tailrocks/other".to_owned();
    b.event = "pull_request".to_owned();
    let first = metrics_file(&directory, "first.json", &a);
    let second = metrics_file(&directory, "other.json", &b);

    let output = compare(&[
        first.to_str().unwrap_or_else(|| unreachable!()),
        second.to_str().unwrap_or_else(|| unreachable!()),
        "--allow-different-populations",
    ]);

    let stderr = stderr_of(&output);
    assert!(!output.status.success(), "{}", stdout_of(&output));
    assert!(stderr.contains("cannot compare"), "{stderr}");
    assert!(stderr.contains("tailrocks/other"), "{stderr}");
    assert!(stdout_of(&output).is_empty(), "{}", stdout_of(&output));
}

#[test]
fn runs_of_different_events_are_not_compared_without_the_opt_in() {
    let directory = fresh_dir("velnor-ci-bench-compare", "without-opt-in");
    let a = recorded_metrics();
    let mut b = recorded_metrics();
    b.event = "pull_request".to_owned();
    let first = metrics_file(&directory, "push.json", &a);
    let second = metrics_file(&directory, "pull-request.json", &b);

    let output = compare(&[
        first.to_str().unwrap_or_else(|| unreachable!()),
        second.to_str().unwrap_or_else(|| unreachable!()),
    ]);

    let stderr = stderr_of(&output);
    assert!(!output.status.success(), "{}", stdout_of(&output));
    assert!(
        stderr.contains("cannot compare runs of different events"),
        "{stderr}"
    );
    assert!(stdout_of(&output).is_empty(), "{}", stdout_of(&output));
}
