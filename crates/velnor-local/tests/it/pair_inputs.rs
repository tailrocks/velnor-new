// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;

use velnor_local::images::{Lock, plan};
use velnor_local::io::pair::{ensure_clean, write_private};

use crate::support::temp_dir;

const REAL_LOCK: &str = include_str!("../../../../.velnor/runner.lock");

#[test]
fn a_lock_with_a_duplicate_assignment_or_a_malformed_line_is_refused() {
    for (lock, reason) in [
        ("a = \"1\"\na = \"2\"\n", "a second time"),
        ("a = \"1\"\nnot an assignment\n", "line 2"),
        ("[table]\n", "line 1"),
        ("a = \"1\"\nb = nope\n", "line 2"),
    ] {
        let refused = Lock::parse(lock).err().unwrap_or_default();
        assert!(refused.contains(reason), "{lock:?}: {refused}");
    }
    assert!(Lock::parse(REAL_LOCK).is_ok());
}

#[test]
fn each_checkout_builds_in_its_own_cargo_target_volume() {
    let lock = Lock::parse(REAL_LOCK).unwrap_or_else(|e| unreachable!("{e}"));
    let volume = |root: &str| -> String {
        let steps = plan(&lock, Path::new(root), Path::new("/stages"), "abc")
            .unwrap_or_else(|e| unreachable!("{e}"));
        steps[0]
            .iter()
            .find(|arg| arg.ends_with(":/target"))
            .cloned()
            .unwrap_or_default()
    };
    assert_ne!(volume("/a"), volume("/b"));
    assert_eq!(volume("/a"), volume("/a"));
    assert!(volume("/a").starts_with("velnor-arm64-target-"));
}

#[tokio::test]
async fn a_failed_jit_write_leaves_no_staged_credentials_behind() {
    let dir = temp_dir("pair-failed-write");
    let path = dir.join("jit");
    std::fs::create_dir(&path).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(write_private(&path, "secret").await.is_err());
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .flatten()
        .map(|entry| entry.file_name())
        .collect();
    assert_eq!(names, ["jit"]);
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["-c", "user.name=t", "-c", "user.email=t@t"])
        .args(args)
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(status.success(), "git {args:?}");
}

#[tokio::test]
async fn only_a_checkout_with_modified_tracked_files_is_refused() {
    let dir = temp_dir("pair-dirty");
    git(&dir, &["init", "-q"]);
    std::fs::write(dir.join("tracked"), "one").unwrap_or_else(|e| unreachable!("{e}"));
    git(&dir, &["add", "tracked"]);
    git(&dir, &["commit", "-q", "-m", "one"]);
    std::fs::write(dir.join("untracked"), "x").unwrap_or_else(|e| unreachable!("{e}"));
    assert!(ensure_clean(&dir).await.is_ok());
    std::fs::write(dir.join("tracked"), "two").unwrap_or_else(|e| unreachable!("{e}"));
    let refused = ensure_clean(&dir).await.err().map(|e| e.to_string());
    assert!(
        refused.is_some_and(|text| text.contains("uncommitted changes")),
        "a modified tracked file must be refused"
    );
}
