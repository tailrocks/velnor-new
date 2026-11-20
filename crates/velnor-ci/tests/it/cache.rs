// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use crate::io::Repo as Checkout;

mod born;
mod public_declared;
mod public_git;
mod public_guards;
mod public_verdicts;
mod support;

use support::{COMPILED, WARM, decide, decide_target_within, decide_within, seed};

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

#[test]
fn an_over_budget_notice_keeps_a_target_path_with_a_line_break_on_one_line() {
    let repo = Checkout::new("decide-over-budget-path");
    let target = "target\n::stop-commands::token";
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    repo.write_unstaged(
        &format!("{target}/debug/deps/libbig.rlib"),
        &"x".repeat(4096),
    );
    let (output, written) = decide_target_within(&repo, target, "1000");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=false\nsources=true\n");
    assert!(
        stdout.contains("more than the repository cache budget"),
        "{stdout}"
    );
    assert!(
        stdout
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stdout}"
    );
    assert!(
        stdout.contains("target%0A::stop-commands::token"),
        "{stdout}"
    );
}

#[test]
fn an_inventory_notice_keeps_an_entry_name_with_a_line_break_on_one_line() {
    let repo = Checkout::new("decide-inventory-name");
    seed(&repo);
    let odd = "target/debug/deps/odd\n::stop-commands::token";
    repo.write_unstaged(odd, "sealed");
    fs::set_permissions(repo.root().join(odd), fs::Permissions::from_mode(0o000))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide(&repo, Some(COMPILED));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert!(
        stdout.contains("pull requests will read the whole restored base"),
        "{stdout}"
    );
    assert!(
        stdout
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stdout}"
    );
    assert!(stdout.contains("odd%0A::stop-commands::token"), "{stdout}");
}

#[test]
fn a_saved_target_path_with_a_line_break_stays_on_one_line() {
    let repo = Checkout::new("decide-saved-path");
    let target = "target\n::stop-commands::token";
    repo.write_unstaged("Cargo.lock", "version = 4\n");
    repo.write_unstaged(&format!("{target}/debug/deps/small.rlib"), "x");
    let (output, written) = decide_target_within(&repo, target, "100000");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert_eq!(written, "save=true\nsources=true\n");
    assert!(
        stdout
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stdout}"
    );
    assert!(
        stdout.contains("target%0A::stop-commands::token"),
        "{stdout}"
    );
}

#[test]
fn a_refused_manifest_path_with_a_line_break_stays_on_one_line() {
    let repo = Checkout::new("decide-refused-path");
    seed(&repo);
    let manifest = "dir\n::stop-commands::token/Cargo.toml";
    let temp = repo.root().join("runner-temp");
    fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("velnor-run.log"), COMPILED).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = repo.root().join("github-output");
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args([
            "cache",
            "decide",
            "--target-dir",
            "target",
            "--manifest-path",
        ])
        .arg(manifest)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(&outputs).unwrap_or_default(),
        "save=false\nsources=false\n"
    );
    assert!(stdout.contains("not proven public"), "{stdout}");
    assert!(
        stdout
            .lines()
            .all(|line| !line.starts_with("::stop-commands::")),
        "{stdout}"
    );
    assert!(stdout.contains("dir%0A::stop-commands::token"), "{stdout}");
}
