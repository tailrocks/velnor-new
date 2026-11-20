// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use crate::io::Repo as Checkout;

pub(super) const DAY: i64 = 86_400;

pub(super) const NOW: i64 = 1_794_873_600;

pub(super) const COMPILED: &str =
    "   Compiling app v0.1.0 (/work/lab/app)\n    Finished `dev` profile in 3.20s\n";

pub(super) const WARM: &str =
    "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s\n";

pub(super) fn decide(repo: &Checkout, log: Option<&str>) -> (Output, String) {
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    if let Some(log) = log {
        fs::write(temp.join("velnor-run.log"), log).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "cache",
            "decide",
            "--target-dir",
            "target",
            "--archive",
            "nextest-archive.tar.zst",
        ])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    (output, written)
}

pub(super) fn seed(repo: &Checkout) {
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    repo.write_unstaged("target/debug/incremental/app-1/work", "x");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("nextest-archive.tar.zst", "archive");
}

pub(super) fn restored(repo: &Checkout, days: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.arg("restored").current_dir(repo.root());
    if let Some(days) = days {
        command.env("VELNOR_BASE_MAX_AGE_DAYS", days);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn aged(name: &str, born: &str) -> Checkout {
    let repo = Checkout::new(name);
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("target/.velnor/born", born);
    repo
}

pub(super) fn decide_within(repo: &Checkout, budget: &str) -> (Output, String) {
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), COMPILED).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "decide", "--target-dir", "target"])
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env("VELNOR_CACHE_BUDGET_BYTES", budget)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    (output, fs::read_to_string(&outputs).unwrap_or_default())
}
