// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::{Command, Output};

use velnor_ci::cache::born_expired;
use velnor_ci::time::date_text;

use crate::io::Repo as Checkout;

const DAY: i64 = 86_400;
const NOW: i64 = 1_794_873_600;
const COMPILED: &str =
    "   Compiling app v0.1.0 (/work/lab/app)\n    Finished `dev` profile in 3.20s\n";
const WARM: &str = "    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.40s\n";

fn decide(repo: &Checkout, log: Option<&str>) -> (Output, String) {
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

fn seed(repo: &Checkout) {
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    repo.write_unstaged("target/debug/incremental/app-1/work", "x");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("nextest-archive.tar.zst", "archive");
}

#[test]
fn a_run_that_compiled_a_crate_saves_after_dropping_what_must_not_be_cached() {
    let repo = Checkout::new("decide-compiled");
    seed(&repo);
    let (output, written) = decide(&repo, Some(COMPILED));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert!(stdout.contains("1 crates compiled"), "{stdout}");
    assert!(stdout.contains(" bytes"), "{stdout}");
    assert!(!repo.root().join("target/debug/incremental").exists());
    assert!(!repo.root().join("nextest-archive.tar.zst").exists());
    assert!(repo.root().join("target/debug/deps/libapp.rlib").is_file());
}

#[test]
fn a_build_script_incremental_directory_survives_while_cargo_incremental_state_is_removed() {
    let repo = Checkout::new("decide-build-script-incremental");
    seed(&repo);
    repo.write_unstaged("target/debug/build/app-hash/out/incremental/asset", "keep");
    repo.write_unstaged(
        "target/x86_64-unknown-linux-gnu/release/incremental/app/work",
        "cross",
    );
    let (output, written) = decide(&repo, Some(COMPILED));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert!(!repo.root().join("target/debug/incremental").exists());
    assert!(
        !repo
            .root()
            .join("target/x86_64-unknown-linux-gnu/release/incremental")
            .exists()
    );
    assert!(
        repo.root()
            .join("target/debug/build/app-hash/out/incremental/asset")
            .is_file()
    );
    assert!(repo.root().join("target/debug/deps/libapp.rlib").is_file());
}

#[test]
fn a_warm_run_saves_nothing_and_removes_nothing() {
    let log = Some(WARM);
    let repo = Checkout::new("decide-warm");
    seed(&repo);
    let (output, written) = decide(&repo, log);
    assert!(output.status.success());
    assert_eq!(written, "save=false\nsources=true\n");
    assert!(repo.root().join("target/debug/incremental").is_dir());
    assert!(repo.root().join("nextest-archive.tar.zst").is_file());
    assert!(!repo.root().join("target/.velnor/born").exists());
}

#[test]
fn a_cold_build_writes_the_born_date_and_a_later_save_carries_it_forward() {
    let repo = Checkout::new("decide-born");
    seed(&repo);
    decide(&repo, Some(COMPILED));
    let born = fs::read_to_string(repo.root().join("target/.velnor/born"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(born.len(), "2026-11-17\n".len());
    repo.write_unstaged("target/.velnor/born", "2026-01-02\n");
    decide(&repo, Some(COMPILED));
    let kept = fs::read_to_string(repo.root().join("target/.velnor/born"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(kept, "2026-01-02\n");
}

fn restored(repo: &Checkout, days: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command.arg("restored").current_dir(repo.root());
    if let Some(days) = days {
        command.env("VELNOR_BASE_MAX_AGE_DAYS", days);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn aged(name: &str, born: &str) -> Checkout {
    let repo = Checkout::new(name);
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    repo.write_unstaged("target/.velnor/born", born);
    repo
}

#[test]
fn restored_removes_a_build_directory_born_more_than_the_limit_ago() {
    let old = date_text(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0))
            - 8 * DAY,
    );
    let repo = aged("expire-old", &format!("{old}\n"));
    let output = restored(&repo, Some("7"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("builds cold"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    assert!(repo.root().join("src/lib.rs").is_file());
}

#[test]
fn restored_keeps_a_young_build_directory() {
    let today = date_text(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(0)),
    );
    let young = aged("expire-young", &format!("{today}\n"));
    assert!(restored(&young, Some("7")).status.success());
    assert!(young.root().join("target/debug/deps/libapp.rlib").is_file());
}

#[test]
fn restored_keeps_an_old_build_directory_when_no_limit_is_set() {
    let unlimited = aged("expire-unlimited", "2020-01-01\n");
    for days in [None, Some("0")] {
        assert!(restored(&unlimited, days).status.success());
        assert!(
            unlimited
                .root()
                .join("target/debug/deps/libapp.rlib")
                .is_file()
        );
    }
}

#[test]
fn the_born_date_expires_after_exactly_the_limit_and_an_unreadable_one_expires_at_once() {
    let born = date_text(NOW);
    assert!(!born_expired(&born, NOW + 7 * DAY, 7));
    assert!(born_expired(&born, NOW + 8 * DAY, 7));
    assert!(born_expired("garbage", NOW, 7));
    assert!(born_expired("", NOW, 7));
}

#[test]
fn dates_are_written_as_the_utc_calendar_day() {
    assert_eq!(date_text(0), "1970-01-01");
    assert_eq!(date_text(951_782_400 + 3_600), "2000-02-29");
    assert_eq!(date_text(NOW - 1), "2026-11-16");
    assert_eq!(date_text(NOW), "2026-11-17");
}

#[test]
fn restored_treats_a_build_directory_saved_without_a_born_file_as_expired() {
    let repo = Checkout::new("expire-unborn");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    let output = restored(&repo, Some("7"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("builds cold"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    let unlimited = restored(&repo, None);
    assert!(unlimited.status.success());
}

#[test]
fn restored_keeps_a_build_directory_without_a_born_file_when_no_limit_is_set() {
    let repo = Checkout::new("keep-unborn");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    repo.write_unstaged("target/debug/deps/libapp.rlib", "rlib");
    for days in [None, Some("0")] {
        assert!(restored(&repo, days).status.success());
        assert!(repo.root().join("target/debug/deps/libapp.rlib").is_file());
    }
}

#[test]
fn trim_without_a_token_fails() {
    let repo = Checkout::new("trim-no-token");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["cache", "trim", "--prefix", "velnor-tgt-rust-root-"])
        .env("GITHUB_REPOSITORY", "owner/name")
        .env("GITHUB_REF", "refs/heads/main")
        .env_remove("GITHUB_TOKEN")
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{stdout}{stderr}");
    assert!(stderr.contains("GITHUB_TOKEN"), "{stderr}");
}

#[test]
fn a_non_utf8_child_log_still_saves_compiled_crates() {
    let repo = Checkout::new("decide-non-utf8");
    seed(&repo);
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let mut bytes = vec![0xff, b'\n'];
    bytes.extend_from_slice(COMPILED.as_bytes());
    fs::write(temp.join("velnor-run.log"), bytes).unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide(&repo, None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
}

#[test]
fn cache_decision_rechecks_changed_lock_sources_before_saving() {
    for source in [
        "registry+https://github.com/rust-lang/crates.io-index",
        "sparse+https://index.crates.io/",
    ] {
        check_source(source, true);
    }
}

#[test]
fn cache_decision_refuses_git_sources() {
    check_source("git+https://github.com/owner/private#123", false);
}

#[test]
fn cache_decision_refuses_alternate_registries() {
    check_source("registry+https://private.example/index", false);
}

fn check_source(source: &str, public: bool) {
    let repo = Checkout::new("decide-private");
    seed(&repo);
    repo.write_unstaged(
        "Cargo.lock",
        &format!(
            "version = 4\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = {source:?}\n"
        ),
    );
    let (output, written) = decide(&repo, Some(COMPILED));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        written,
        format!("save={public}\nsources={public}\n"),
        "{output:?}"
    );
    if !public {
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("not proven public"),
            "{output:?}"
        );
    }
}

#[test]
fn missing_run_log_fails_without_publishing_a_decision() {
    log_error(false);
}

#[test]
fn unreadable_run_log_fails_without_publishing_a_decision() {
    log_error(true);
}

fn log_error(unreadable: bool) {
    let repo = Checkout::new("decide-log-error");
    seed(&repo);
    if unreadable {
        fs::create_dir_all(repo.root().join("runner-temp/velnor-run.log"))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let (output, written) = decide(&repo, None);
    assert!(!output.status.success(), "{output:?}");
    assert!(written.is_empty(), "{written}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("velnor-run.log"),
        "{output:?}"
    );
    assert!(repo.root().join("target/debug/incremental").exists());
}

#[test]
fn cache_decision_refuses_locks_that_cannot_prove_public_sources() {
    for mode in ["missing", "malformed", "unreadable", "unused-private-patch"] {
        let repo = Checkout::new("decide-lock-refusal");
        seed(&repo);
        repo.git(&["add", "Cargo.lock"]);
        match mode {
            "missing" => fs::remove_file(repo.root().join("Cargo.lock"))
                .unwrap_or_else(|e| unreachable!("{e}")),
            "malformed" => repo.write_unstaged("Cargo.lock", "not a lock file"),
            "unreadable" => {
                fs::remove_file(repo.root().join("Cargo.lock"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
                fs::create_dir(repo.root().join("Cargo.lock"))
                    .unwrap_or_else(|e| unreachable!("{e}"));
            }
            _ => repo.write_unstaged("Cargo.lock", "version = 4\n[[patch.unused]]\nname = \"secret\"\nversion = \"1.0.0\"\nsource = \"git+https://example.com/private#123\"\n"),
        }
        let (output, written) = decide(&repo, Some(COMPILED));
        assert!(output.status.success(), "{mode}: {output:?}");
        assert_eq!(written, "save=false\nsources=false\n", "{mode}: {output:?}");
        assert!(repo.root().join("target/debug/incremental").is_dir());
    }
}

fn decide_within(repo: &Checkout, budget: &str) -> (Output, String) {
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

#[test]
fn a_target_directory_larger_than_the_cache_budget_is_not_saved() {
    let repo = Checkout::new("decide-over-budget");
    seed(&repo);
    repo.write_unstaged("target/debug/deps/libbig.rlib", &"x".repeat(4096));
    let (output, written) = decide_within(&repo, "1000");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=false\nsources=true\n");
    assert!(
        stdout.contains("more than the repository cache budget of 1000 bytes"),
        "{stdout}"
    );
    assert!(!repo.root().join("target/.velnor/born").exists());
    fs::remove_file(repo.root().join("github-output")).unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide_within(&repo, "100000");
    assert!(output.status.success());
    assert_eq!(written, "save=true\nsources=true\n");
}

#[test]
fn an_unreadable_cache_budget_fails_the_decision_instead_of_saving() {
    let repo = Checkout::new("decide-bad-budget");
    seed(&repo);
    let (output, written) = decide_within(&repo, "eight");
    assert!(!output.status.success());
    assert_eq!(written, "");
}
