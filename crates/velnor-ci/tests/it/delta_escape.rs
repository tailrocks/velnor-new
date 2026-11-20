// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::process::{Command, Output};

use crate::io::Repo;

const ODD: &str = "odd\n::stop-commands::token";

fn delta(repo: &Repo, verb: &str, target: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["delta", verb, "--target-dir", target])
        .args(["--staging-dir", "delta", "--package", "app"])
        .current_dir(repo.root())
        .env("BASE_CACHE_KEY", "base")
        .env("GITHUB_OUTPUT", repo.root().join("delta-output"))
        .env("RUNNER_TEMP", repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn assert_no_runner_command(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(output.status.success(), "{output:?}");
    assert!(
        stdout
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stdout}"
    );
    assert!(stdout.contains("%0A"), "{stdout}");
    stdout
}

fn linked_target(name: &str) -> Repo {
    let repo = Repo::new(name);
    fs::create_dir_all(repo.root().join("real")).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(repo.root().join("real"), repo.root().join(ODD))
        .unwrap_or_else(|e| unreachable!("{e}"));
    repo
}

#[test]
fn an_apply_notice_keeps_a_symlinked_target_path_with_a_line_break_on_one_line() {
    let repo = linked_target("delta-escape-apply");
    let stdout = assert_no_runner_command(&delta(&repo, "apply", ODD));
    assert!(stdout.contains("::notice::"), "{stdout}");
    assert!(stdout.contains("skipping delta reuse"), "{stdout}");
}

#[test]
fn a_stage_notice_keeps_a_symlinked_target_path_with_a_line_break_on_one_line() {
    let repo = linked_target("delta-escape-stage");
    let stdout = assert_no_runner_command(&delta(&repo, "stage", ODD));
    assert!(stdout.contains("saving no delta"), "{stdout}");
}

#[test]
fn a_cold_apply_keeps_an_entry_name_with_a_line_break_on_one_line() {
    let repo = Repo::new("delta-escape-cold");
    repo.write_unstaged("target/debug/real", "x");
    symlink(
        repo.root().join("target/debug/real"),
        repo.root().join("target/debug").join(ODD),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = assert_no_runner_command(&delta(&repo, "apply", "target"));
    assert!(stdout.contains("delta apply failed"), "{stdout}");
}
