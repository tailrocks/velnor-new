// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use velnor_local::io::local_ci::pin_workflows;
use velnor_local::timeout::{TimeoutScale, parse_timeout_scale, scale};

use crate::local_ci::parsed;
use crate::support::temp_dir;

fn factor(text: &str) -> TimeoutScale {
    parse_timeout_scale(text).unwrap_or_else(|e| unreachable!("{text}: {e}"))
}

fn workflow() -> String {
    [
        "name: ci",
        "jobs:",
        "  test:",
        "    runs-on: ubuntu-26.04",
        "    timeout-minutes: 30",
        "    steps:",
        "      - name: Check out",
        "        uses: actions/checkout@v6",
        "      - name: Test",
        "        timeout-minutes: 10",
        "        run: cargo nextest run",
        "      - name: Lint",
        "        timeout-minutes: 7 # slow disk",
        "      - name: Build",
        "        timeout-minutes: ${{ inputs.minutes }}",
        "      - name: Quoted",
        "        timeout-minutes: \"5\"",
        "      - name: Other key",
        "        timeout-minutes-hint: 5",
        "",
    ]
    .join("\n")
}

#[test]
fn a_factor_multiplies_every_numeric_value_at_job_and_step_level() {
    let (text, count) = scale(&workflow(), factor("3"));
    assert_eq!(count, 3, "{text}");
    assert!(text.contains("    timeout-minutes: 90\n"), "{text}");
    assert!(text.contains("        timeout-minutes: 30\n"), "{text}");
    assert!(
        text.contains("        timeout-minutes: 21 # slow disk\n"),
        "{text}"
    );
}

#[test]
fn a_fractional_factor_rounds_each_value_up_to_whole_minutes() {
    let (text, _) = scale(&workflow(), factor("1.5"));
    assert!(text.contains("    timeout-minutes: 45\n"), "{text}");
    assert!(text.contains("        timeout-minutes: 15\n"), "{text}");
    assert!(
        text.contains("        timeout-minutes: 11 # slow disk\n"),
        "{text}"
    );
}

#[test]
fn a_factor_with_a_decimal_is_exact_before_rounding_up() {
    let (text, _) = scale("    timeout-minutes: 10\n", factor("1.1"));
    assert_eq!(text, "    timeout-minutes: 11\n");
}

#[test]
fn the_default_factor_leaves_the_workflow_byte_for_byte_unchanged() {
    let original = workflow();
    let (text, count) = scale(&original, TimeoutScale::default());
    assert_eq!(count, 0);
    assert_eq!(text, original);
    assert_eq!(factor("1"), TimeoutScale::default());
}

#[test]
fn non_numeric_values_and_other_keys_are_left_alone() {
    let (text, _) = scale(&workflow(), factor("3"));
    assert!(
        text.contains("        timeout-minutes: ${{ inputs.minutes }}\n"),
        "{text}"
    );
    assert!(text.contains("        timeout-minutes: \"5\"\n"), "{text}");
    assert!(text.contains("        timeout-minutes-hint: 5\n"), "{text}");
}

#[test]
fn a_factor_below_one_or_malformed_is_refused() {
    for bad in [
        "0", "0.5", "0.999", "", "abc", "-2", "1.2345", "1.", ".5", "inf",
    ] {
        assert!(parse_timeout_scale(bad).is_err(), "{bad} was accepted");
    }
    for good in ["1", "1.5", "2.125", "10"] {
        assert!(parse_timeout_scale(good).is_ok(), "{good} was refused");
    }
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(["-C", &repo.display().to_string()])
        .args(args)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn checkout(name: &str) -> std::path::PathBuf {
    let repo = temp_dir(name);
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(repo.join(".github/workflows")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::write(repo.join(".github/workflows/ci.yml"), workflow())
        .unwrap_or_else(|e| unreachable!("{e}"));
    git(&repo, &["add", "-A"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@localhost",
            "commit",
            "-q",
            "-m",
            "workflow",
        ],
    );
    repo
}

#[tokio::test]
async fn the_run_copy_gets_the_scaled_timeouts_and_a_default_run_leaves_it_alone() {
    let repo = checkout("timeout-scale-copy");
    let args = parsed(&repo, "plain", &[]);
    pin_workflows(&args, &repo, "t", TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "1");

    pin_workflows(&args, &repo, "t", factor("3"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = std::fs::read_to_string(repo.join(".github/workflows/ci.yml"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(text.contains("    timeout-minutes: 90\n"), "{text}");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(git(&repo, &["status", "--porcelain"]), "");
}
