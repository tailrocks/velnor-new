// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Output};

const VERIFIED: &str = "1111111111111111111111111111111111111111";

fn preflight(prefix: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "preflight", "--repo=o/n"])
        .arg(format!("--verified-commit={VERIFIED}"))
        .args(prefix)
        .env_remove("GITHUB_TOKEN")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn ran_and_refused(output: &Output) -> bool {
    !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains("set GITHUB_TOKEN to a GitHub token")
}

#[test]
fn a_prefix_that_looks_like_help_still_runs_the_preflight_when_attached_to_its_option() {
    assert!(ran_and_refused(&preflight(&["--branch-prefix=-h"])));
}

#[test]
fn a_prefix_that_looks_like_help_still_runs_the_preflight_when_given_as_the_next_word() {
    assert!(ran_and_refused(&preflight(&["--branch-prefix", "-h"])));
}
