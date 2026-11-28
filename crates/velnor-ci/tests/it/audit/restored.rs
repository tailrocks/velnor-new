// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use super::own_units::link_script;
use super::{audit, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;
use velnor_ci::inputs::restored_units_file;

const TARGET: &str = "rust/target/debug";
const STALE: &str = "f00000000000000e";
const SCRIPT_STALE: &str = "f00000000000000f";
const RUN: &str = "f000000000000010";
const OTHER_COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

fn manifest_path() -> String {
    format!("rust/target/{}", restored_units_file("rust/"))
}

fn head(repo: &Repo) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn with_stale_unit() -> Repo {
    let repo = stale_units();
    repo.commit();
    repo
}

fn stale_units() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, TARGET, &[]);
    let stale = format!("{TARGET}/deps/app-{STALE}.d");
    unit(
        &repo,
        &stale,
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    unit(
        &repo,
        &format!("{TARGET}/.fingerprint/app-{STALE}/lib-app"),
        "",
    );
    repo.set_mtime(&stale, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    let script = format!("{TARGET}/build/app-{SCRIPT_STALE}/build_script_build-{SCRIPT_STALE}.d");
    unit(
        &repo,
        &script,
        "out.d: app/build.rs app/src/../../../shared/greeting.txt\n",
    );
    repo.set_mtime(&script, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    repo.set_mtime(
        "shared/greeting.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    repo
}

fn dep_info_of(hash: &str) -> String {
    if hash == SCRIPT_STALE {
        format!("{TARGET}/build/app-{SCRIPT_STALE}/build_script_build-{SCRIPT_STALE}.d")
    } else {
        format!("{TARGET}/deps/app-{hash}.d")
    }
}

fn manifest_of(repo: &Repo, header: &str, restored: &[&str]) {
    let lines: Vec<String> = restored
        .iter()
        .map(|hash| {
            let text = std::fs::read_to_string(repo.root().join(dep_info_of(hash)))
                .unwrap_or_else(|e| unreachable!("{e}"));
            format!(
                "restored {hash} {}",
                velnor_ci::restored_units::digest(&text)
            )
        })
        .collect();
    unit(
        repo,
        &manifest_path(),
        &format!("{header}\n{}\n", lines.join("\n")),
    );
}

fn manifest(repo: &Repo, restored: &[&str]) {
    let header = format!("velnor-restored-units 5 {}", head(repo));
    manifest_of(repo, &header, restored);
}

fn refused(repo: &Repo) {
    let output = audit(repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("found stale dep-info"),
        "{output:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_stale_dep_info_of_a_restored_unit_does_not_refuse_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &[STALE, SCRIPT_STALE]);

    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn rewrite(repo: &Repo, hash: &str) {
    let path = dep_info_of(hash);
    let text =
        std::fs::read_to_string(repo.root().join(&path)).unwrap_or_else(|e| unreachable!("{e}"));
    unit(repo, &path, &format!("{text}\n"));
    repo.set_mtime(&path, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
}

#[test]
fn a_stale_dep_info_rewritten_after_the_restore_refuses_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &[STALE, SCRIPT_STALE]);
    rewrite(&repo, STALE);
    refused(&repo);
}

#[test]
fn a_stale_build_script_dep_info_rewritten_after_the_restore_refuses_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &[STALE, SCRIPT_STALE]);
    rewrite(&repo, SCRIPT_STALE);
    refused(&repo);
}

#[test]
fn a_stale_dep_info_the_manifest_does_not_list_refuses_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &[SCRIPT_STALE]);
    refused(&repo);
}

#[test]
fn a_stale_build_script_dep_info_the_manifest_does_not_list_refuses_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &[STALE]);
    refused(&repo);
}

#[test]
fn a_manifest_of_an_older_format_does_not_excuse_a_stale_dep_info() {
    let repo = with_stale_unit();
    let header = format!("velnor-restored-units 4 {}", head(&repo));
    manifest_of(&repo, &header, &[STALE, SCRIPT_STALE]);
    refused(&repo);
}

#[test]
fn a_manifest_written_at_another_commit_does_not_excuse_a_stale_dep_info() {
    let repo = with_stale_unit();
    let header = format!("velnor-restored-units 5 {OTHER_COMMIT}");
    manifest_of(&repo, &header, &[STALE, SCRIPT_STALE]);
    refused(&repo);
}

#[test]
fn a_manifest_bound_to_no_commit_does_not_excuse_a_stale_dep_info() {
    let repo = with_stale_unit();
    manifest_of(&repo, "velnor-restored-units 5", &[STALE, SCRIPT_STALE]);
    refused(&repo);
}

#[test]
fn an_audit_before_the_first_commit_does_not_trust_a_manifest_bound_to_no_commit() {
    let repo = stale_units();
    manifest_of(&repo, "velnor-restored-units 5", &[STALE, SCRIPT_STALE]);
    refused(&repo);
}

fn with_restored_run() -> Repo {
    let repo = with_stale_unit();
    let script = dep_info_of(SCRIPT_STALE);
    let output = format!("{TARGET}/build/app-{RUN}/output");
    unit(&repo, &output, "cargo:rerun-if-changed=build.rs\n");
    link_script(&repo, &script, &output);
    unit(
        &repo,
        &format!("{TARGET}/.fingerprint/app-{RUN}/run-build-script-build-script-build"),
        "",
    );
    repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_500_000_000));
    manifest(&repo, &[STALE, SCRIPT_STALE]);
    repo
}

fn uncompiled(repo: &Repo) {
    let output = audit(repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("build script run without a current dep-info of its compile"),
        "{output:?}"
    );
}

#[test]
fn a_build_script_run_of_a_restored_stale_script_is_credited_not_a_gap() {
    let repo = with_restored_run();
    let output = audit(&repo, &["--write"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
}

#[test]
fn a_restored_run_is_not_credited_when_its_script_dep_info_lost_build_rs() {
    let repo = with_restored_run();
    unit(&repo, &dep_info_of(SCRIPT_STALE), "out.d:\n");
    uncompiled(&repo);
}

#[test]
fn a_restored_run_is_not_credited_when_its_script_dep_info_is_deleted() {
    let repo = with_restored_run();
    std::fs::remove_file(repo.root().join(dep_info_of(SCRIPT_STALE)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    uncompiled(&repo);
}

#[test]
fn a_restored_run_is_not_credited_when_its_script_dep_info_names_no_repository_path() {
    let repo = with_restored_run();
    unit(
        &repo,
        &dep_info_of(SCRIPT_STALE),
        "out.d: /nowhere/build.rs\n",
    );
    uncompiled(&repo);
}

fn restore(repo: &Repo) {
    for args in [["inputs", "save"], ["inputs", "restore"]] {
        let output = velnor_ci()
            .current_dir(repo.root().join("rust"))
            .args(args)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(output.status.success(), "{args:?}: {output:?}");
    }
    for (path, seconds) in [
        (dep_info_of(STALE), 3_000_000_001),
        (dep_info_of(SCRIPT_STALE), 3_000_000_001),
        ("shared/greeting.txt".to_owned(), 4_000_000_000),
    ] {
        repo.set_mtime(&path, UNIX_EPOCH + Duration::from_secs(seconds));
    }
}

#[test]
fn a_restore_then_audit_credits_a_restored_run_and_refuses_it_once_its_script_dep_info_changes() {
    let repo = with_restored_run();
    std::fs::remove_file(repo.root().join(manifest_path())).unwrap_or_else(|e| unreachable!("{e}"));
    restore(&repo);
    assert!(repo.root().join(manifest_path()).is_file());

    let credited = audit(&repo, &["--write"]);
    assert_eq!(credited.status.code(), Some(0), "{credited:?}");

    unit(&repo, &dep_info_of(SCRIPT_STALE), "out.d:\n");
    uncompiled(&repo);
}
