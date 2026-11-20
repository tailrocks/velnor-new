// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use velnor_local::io::local_ci::{pin_branch, pin_workflows};
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
async fn scaling_a_branch_twice_scales_it_once() {
    let repo = checkout("review-1947-compound");
    let args = parsed(&repo, "plain", &[]);
    pin_workflows(&args, &repo, "t", factor("3"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    pin_branch(&repo, "t", None, factor("3"))
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = std::fs::read_to_string(repo.join(".github/workflows/ci.yml"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(text.contains("    timeout-minutes: 90\n"), "{text}");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");
}

#[test]
fn a_block_scalar_line_that_looks_like_the_key_is_left_alone() {
    let text = "jobs:\n  t:\n    steps:\n      - name: Write\n        run: |\n          cat <<EOF > c.yml\n          timeout-minutes: 10\n          EOF\n";
    let (out, count) = scale(text, factor("3")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(out, text);
    assert_eq!(count, 0);
}

#[test]
fn a_timeout_inside_a_flow_mapping_is_scaled() {
    let text = "jobs:\n  test: {runs-on: ubuntu-26.04, timeout-minutes: 30}\n";
    let (out, count) = scale(text, factor("3")).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(out.contains("timeout-minutes: 90}"), "{out}");
    assert_eq!(count, 1);
}

#[test]
fn a_huge_factor_gives_the_exact_product() {
    let (out, _) = scale(
        "jobs:\n  t:\n    timeout-minutes: 30\n",
        factor("10000000000000000"),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        out,
        "jobs:\n  t:\n    timeout-minutes: 300000000000000000\n"
    );
}

#[test]
fn a_product_that_overflows_is_refused() {
    let text = "jobs:\n  t:\n    timeout-minutes: 1000000\n";
    let error = scale(text, factor("18000000000000000"))
        .err()
        .unwrap_or_default();
    assert!(error.contains("64 bits"), "{error}");
}

#[test]
fn an_alias_or_an_anchor_for_a_timeout_is_refused_by_name() {
    for text in [
        "jobs:\n  t:\n    timeout-minutes: *t\n",
        "jobs:\n  t:\n    timeout-minutes: &t 30\n",
        "jobs:\n  t:\n    <<: *base\n",
    ] {
        let error = scale(text, factor("3")).err().unwrap_or_default();
        assert!(
            error.contains("alias") || error.contains("anchor"),
            "{text}: {error}"
        );
    }
}

#[test]
fn keys_outside_job_and_step_level_are_left_alone() {
    let text = "env:\n  timeout-minutes: 5\njobs:\n  t:\n    with:\n      timeout-minutes: 5\n    steps:\n      - with:\n          timeout-minutes: 7\n";
    let (out, count) = scale(text, factor("3")).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!((out.as_str(), count), (text, 0));
}
