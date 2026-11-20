// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_ci::release_listed::{Listing, listing};

const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn sums(names: &[&str]) -> Vec<u8> {
    names
        .iter()
        .flat_map(|name| format!("{DIGEST}  {name}\n").into_bytes())
        .collect()
}

#[test]
fn an_optional_file_the_sums_list_must_be_on_disk() {
    let error = listing(&sums(&["app", "app-macos"]), "app-macos", false).err();
    let message = error.map(|e| e.to_string()).unwrap_or_default();
    assert!(message.contains("app-macos"), "{message}");
    assert!(message.contains("SHA256SUMS"), "{message}");
}

#[test]
fn an_optional_file_the_sums_list_and_disk_holds_is_verified() {
    assert_eq!(
        listing(&sums(&["app", "app-macos"]), "app-macos", true).ok(),
        Some(Listing::Listed)
    );
}

#[test]
fn an_optional_file_the_sums_do_not_list_is_skipped_whether_or_not_it_is_on_disk() {
    for present in [false, true] {
        assert_eq!(
            listing(&sums(&["app"]), "app-macos", present).ok(),
            Some(Listing::Unlisted),
            "{present}"
        );
    }
}

#[test]
fn sums_that_cannot_be_read_are_an_error() {
    assert!(listing(b"not a sums file", "app-macos", true).is_err());
}

fn verify_listed(dir: &std::path::Path, file: &str, command: &str) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["release", "verify-listed", "--dir"])
        .arg(dir)
        .args(["--file", file, "--", command])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn directory(name: &str, listed: &[&str], files: &[&str]) -> std::path::PathBuf {
    let dir = crate::support::fresh_dir("velnor-ci-verify-listed", name);
    std::fs::write(dir.join("SHA256SUMS"), sums(listed)).unwrap_or_else(|e| unreachable!("{e}"));
    for file in files {
        std::fs::write(dir.join(file), "bytes").unwrap_or_else(|e| unreachable!("{e}"));
    }
    dir
}

#[test]
fn the_command_runs_for_a_listed_file_and_its_failure_fails_the_step() {
    let dir = directory("runs", &["app", "app-macos"], &["app-macos"]);
    assert_eq!(
        verify_listed(&dir, "app-macos", "true").status.code(),
        Some(0)
    );
    assert_eq!(
        verify_listed(&dir, "app-macos", "false").status.code(),
        Some(1)
    );
    let _ignored = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_listed_file_that_is_not_on_disk_fails_without_running_the_command() {
    let dir = directory("missing", &["app", "app-macos"], &[]);
    assert_eq!(
        verify_listed(&dir, "app-macos", "true").status.code(),
        Some(1)
    );
    let _ignored = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unlisted_file_skips_the_command_even_when_it_would_fail() {
    let dir = directory("unlisted", &["app"], &[]);
    assert_eq!(
        verify_listed(&dir, "app-macos", "false").status.code(),
        Some(0)
    );
    let _ignored = std::fs::remove_dir_all(&dir);
}
