// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;
use std::fs;
use std::process::{Command, Output};
use velnor_test_support::{real_cargo, real_cargo_str};

fn helper(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .env("RUNNER_TEMP", repo.root().join("temp"))
        .env("CARGO_TERM_COLOR", "never")
        .env("GITHUB_OUTPUT", repo.root().join("outputs"))
        .env("CARGO_HOME", repo.root().join("cargo-home"))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env("RUSTC", real_cargo().with_file_name("rustc"))
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
}

fn start_job(repo: &Repo) {
    for file in ["velnor-run.log", "velnor-invocations.jsonl"] {
        repo.write_unstaged(&format!("temp/{file}"), "");
    }
}

#[test]
fn a_quiet_cargo_configuration_cannot_hide_compilation_from_the_cache_decision() {
    let repo = Repo::new("cache-quiet");
    repo.write(
        "Cargo.toml",
        "[package]\nname = 'leaf'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n",
    );
    repo.write("src/lib.rs", "pub fn leaf() -> u8 { 1 }\n");
    repo.write(".cargo/config.toml", "[term]\nquiet = true\n");
    start_job(&repo);
    success(&helper(
        &repo,
        &["run", "--", &real_cargo_str(), "build", "--offline"],
    ));
    let output = helper(&repo, &["cache", "decide", "--target-dir", "target"]);
    success(&output);
    let written = fs::read_to_string(repo.root().join("outputs")).unwrap_or_default();
    assert!(written.contains("save=true\n"), "{output:?}: {written}");
}

#[test]
fn a_default_branch_build_can_be_reused_by_a_pr_and_an_eight_day_base_builds_cold() {
    use std::time::{SystemTime, UNIX_EPOCH};
    use velnor_ci::time::date_text;
    let repo = Repo::new("cache-lifecycle");
    repo.write(
        "Cargo.toml",
        "[package]\nname = 'leaf'\nversion = '0.1.0'\nedition = '2024'\n[workspace]\n",
    );
    repo.write("src/lib.rs", "pub fn leaf() -> u8 { 1 }\n");
    start_job(&repo);
    let cargo = real_cargo_str();
    let build = ["run", "--", cargo.as_str(), "build", "--offline"];
    success(&helper(&repo, &build));
    success(&helper(&repo, &["restored", "--target-dir", "target"]));
    success(&helper(&repo, &["inputs", "save"]));
    let decided = helper(&repo, &["cache", "decide", "--target-dir", "target"]);
    success(&decided);
    assert!(
        String::from_utf8_lossy(&decided.stdout).contains("1 crates compiled"),
        "{decided:?}"
    );
    assert!(String::from_utf8_lossy(&decided.stdout).contains(" bytes"));
    let pr = repo.fork("cache-pr-restored");
    pr.set_mtime("src/lib.rs", SystemTime::now());
    start_job(&pr);
    success(&helper(&pr, &["restored", "--target-dir", "target"]));
    success(&helper(&pr, &["inputs", "restore"]));
    success(&helper(&pr, &build));
    let warm = helper(&pr, &["cache", "decide", "--target-dir", "target"]);
    success(&warm);
    assert!(
        String::from_utf8_lossy(&warm.stdout).contains("no crate was compiled"),
        "{warm:?}"
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let old = i64::try_from(now).unwrap_or_default() - 8 * 86_400;
    repo.write_unstaged("target/.velnor/born", &date_text(old));
    let expired = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["restored", "--target-dir", "target"])
        .env("VELNOR_BASE_MAX_AGE_DAYS", "7")
        .env("GITHUB_OUTPUT", repo.root().join("expired"))
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    success(&expired);
    assert!(!repo.root().join("target").exists(), "{expired:?}");
    start_job(&repo);
    success(&helper(&repo, &build));
    success(&helper(&repo, &["inputs", "save"]));
    let cold = helper(&repo, &["cache", "decide", "--target-dir", "target"]);
    success(&cold);
    assert!(
        String::from_utf8_lossy(&cold.stdout).contains("1 crates compiled"),
        "{cold:?}"
    );
    assert_ne!(
        fs::read_to_string(repo.root().join("target/.velnor/born"))
            .unwrap_or_default()
            .trim(),
        date_text(old)
    );
}

#[test]
fn docs_only_selection_reaches_no_saving_lane() {
    let repo = crate::io_plan::plan_repo("cache-docs-only");
    repo.write("docs/guide.md", "a guide\n");
    let output = crate::io_plan::plan_command(&repo)
        .args(["plan", "--changed", "docs/guide.md"])
        .env("GITHUB_OUTPUT", repo.root().join("outputs"))
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    success(&output);
    let outputs = fs::read_to_string(repo.root().join("outputs")).unwrap_or_default();
    for root in ["root", "tools"] {
        assert!(
            outputs.contains(&format!("rust_{root}_mode=none\n")),
            "{outputs}"
        );
    }
    assert!(!repo.root().join("target/.velnor/born").exists());
}
