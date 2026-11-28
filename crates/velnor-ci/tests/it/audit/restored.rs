// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, own_units, unit, workspace_below_the_root};
use crate::io::Repo;
use velnor_ci::inputs::restored_units_file;

const TARGET: &str = "rust/target/debug";
const STALE: &str = "f00000000000000e";
const SCRIPT_STALE: &str = "f00000000000000f";
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
    repo.commit();
    repo
}

fn manifest_of(repo: &Repo, header: &str, restored: &[&str]) {
    let lines: Vec<String> = restored
        .iter()
        .map(|hash| format!("restored {hash}"))
        .collect();
    unit(
        repo,
        &manifest_path(),
        &format!("{header}\n{}\n", lines.join("\n")),
    );
}

fn manifest(repo: &Repo, restored: &[&str]) {
    let header = format!("velnor-restored-units 4 {}", head(repo));
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

#[test]
fn a_stale_dep_info_the_manifest_does_not_list_refuses_the_audit() {
    let repo = with_stale_unit();
    manifest(&repo, &["f00000000000000d", SCRIPT_STALE]);
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
    let header = format!("velnor-restored-units 3 {}", head(&repo));
    manifest_of(&repo, &header, &[STALE, SCRIPT_STALE]);
    refused(&repo);
}

#[test]
fn a_manifest_written_at_another_commit_does_not_excuse_a_stale_dep_info() {
    let repo = with_stale_unit();
    let header = format!("velnor-restored-units 4 {OTHER_COMMIT}");
    manifest_of(&repo, &header, &[STALE, SCRIPT_STALE]);
    refused(&repo);
}

#[test]
fn a_manifest_bound_to_no_commit_does_not_excuse_a_stale_dep_info() {
    let repo = with_stale_unit();
    manifest_of(&repo, "velnor-restored-units 4", &[STALE, SCRIPT_STALE]);
    refused(&repo);
}
