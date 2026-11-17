// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use velnor_ci::io::{break_time_ties_with, cold_on_failure, every_root_target};

use crate::io::Repo;

fn helper(repo: &Repo, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn stale_target(repo: &Repo, relative: &str) {
    repo.write_unstaged(
        &format!("{relative}/debug/.fingerprint/stale/lib-stale"),
        "x",
    );
}

#[test]
fn a_malformed_saved_file_makes_restore_a_cold_build_with_no_stale_target() {
    let repo = Repo::new("cold-malformed");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    stale_target(&repo, "target");
    repo.write_unstaged("target/.velnor/inputs.tsv", "not a valid line\n");
    let output = helper(&repo, &["inputs", "restore"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("building cold"), "{stdout}");
    assert!(stdout.contains("inputs.tsv"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    assert!(repo.root().join("src/lib.rs").is_file());
}

#[test]
fn a_restore_with_bad_arguments_still_fails() {
    let repo = Repo::new("cold-args");
    let output = helper(&repo, &["inputs", "restore", "--bogus"]);
    assert!(!output.status.success());
}

#[test]
fn rounding_detected_after_tie_breaking_makes_a_cold_build_in_every_root() {
    let repo = Repo::new("cold-rounding");
    for path in ["build.rs", "src/lib.rs"] {
        repo.write(path, "x\n");
    }
    repo.write("app/Cargo.toml", "[package]\n");
    let tied = UNIX_EPOCH + Duration::new(1_700_000_000, 193_657_627);
    for path in [".gitignore", "build.rs", "src/lib.rs"] {
        repo.set_mtime(path, tied);
    }
    stale_target(&repo, "target");
    stale_target(&repo, "app/target");
    let report = cold_on_failure(
        &every_root_target(repo.root()).unwrap_or_else(|e| unreachable!("{e}")),
        "restored",
        || break_time_ties_with(repo.root(), &|_, _| Ok(())),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        report.contains("still share a modification time"),
        "{report}"
    );
    assert!(report.contains("building cold"), "{report}");
    assert!(!repo.root().join("target").exists());
    assert!(!repo.root().join("app/target").exists());
}

#[test]
fn a_non_utf8_tracked_path_outside_target_allows_verified_cold_deletion() {
    let repo = Repo::new("cold-non-utf8");
    repo.write("src/lib.rs", "pub fn one() {}\n");
    let blob = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["hash-object", "-w", "--stdin"])
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let blob = String::from_utf8_lossy(&blob.stdout).trim().to_owned();
    let entry = format!("100644,{blob},");
    let mut path = entry.into_bytes();
    path.extend_from_slice(b"src/caf\xe9.rs");
    let status = Command::new("git")
        .arg("-C")
        .arg(repo.root())
        .args(["update-index", "--add", "--cacheinfo"])
        .arg(OsStr::from_bytes(&path))
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success());
    stale_target(&repo, "target");
    let output = helper(&repo, &["restored"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout.contains("not valid UTF-8"), "{stdout}");
    assert!(stdout.contains("building cold"), "{stdout}");
    assert!(!repo.root().join("target").exists());
    assert_eq!(
        std::fs::read(repo.root().join("src/lib.rs")).unwrap_or_default(),
        b"pub fn one() {}\n"
    );
    repo.git(&["ls-files", "--error-unmatch", "src/lib.rs"]);
}
