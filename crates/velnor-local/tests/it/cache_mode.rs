// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use velnor_local::cache_mode::{notice, strip};
use velnor_local::io::local_ci::pin_workflows;
use velnor_local::timeout::TimeoutScale;

use crate::local_ci::parsed;
use crate::support::temp_dir;

const WORKFLOW: &str = "name: ci
on: push
jobs:
  rust:
    runs-on: ubuntu-26.04
    permissions:
      contents: read
    cache-mode: write
    steps:
      - name: Check out
        uses: actions/checkout@v6
      - name: Test
        run: cargo nextest run
  lint:
    runs-on: ubuntu-26.04
    cache-mode: read
    steps:
      - name: Lint
        run: cargo clippy
";

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
    std::fs::write(repo.join(".github/workflows/ci.yml"), WORKFLOW)
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
async fn the_run_copy_has_no_job_level_cache_mode_and_the_notice_counts_the_keys() {
    let repo = checkout("cache-mode-copy");
    let args = parsed(&repo, "plain", &[]);
    pin_workflows(&args, &repo, "t", TimeoutScale::default())
        .await
        .unwrap_or_else(|e| unreachable!("{e}"));

    let text = std::fs::read_to_string(repo.join(".github/workflows/ci.yml"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!text.contains("cache-mode"), "{text}");
    assert!(text.contains("    runs-on: ubuntu-26.04\n"), "{text}");
    assert!(text.contains("      - name: Lint\n"), "{text}");
    assert_eq!(git(&repo, &["rev-list", "--count", "HEAD"]), "2");

    let (stripped, count) = strip(WORKFLOW);
    assert_eq!(count, 2);
    assert_eq!(stripped, text);
    assert_eq!(
        notice("t", ".github/workflows/ci.yml", count),
        "velnor-local t: dropped 2 job-level cache-mode key(s) from .github/workflows/ci.yml: the stand-in does not model GitHub's cache-mode; cache writes are governed by velnor-local's own cache server"
    );
}
