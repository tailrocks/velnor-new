// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "git {args:?}: {output:?}");
}

fn helper(root: &Path, operation: &str, target: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", operation, "--target-dir"])
        .arg(target)
        .current_dir(root)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn plain_owner() -> Repo {
    let owner = Repo::new("foreign-target");
    fs::remove_dir_all(owner.root().join(".git")).unwrap_or_else(|e| unreachable!("{e}"));
    owner
}

fn foreign_target(owner: &Repo, shape: &str) -> (PathBuf, PathBuf, PathBuf) {
    let foreign = owner.root().join("container/checkout");
    fs::create_dir_all(foreign.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    if shape == "bare" {
        git(&foreign, &["init", "--quiet", "--bare"]);
    } else {
        git(&foreign, &["init", "--quiet"]);
        fs::write(foreign.join("src/lib.rs"), "foreign checkout\n")
            .unwrap_or_else(|e| unreachable!("{e}"));
        git(&foreign, &["add", "--all"]);
    }
    let target = match shape {
        "container" => owner.root().join("container"),
        "source" => foreign.join("src"),
        "linked" => {
            git(
                &foreign,
                &[
                    "-c",
                    "user.name=lab",
                    "-c",
                    "user.email=lab@example.com",
                    "-c",
                    "commit.gpgsign=false",
                    "commit",
                    "--quiet",
                    "--message",
                    "fixture",
                ],
            );
            let linked = owner.root().join("linked");
            git(
                &foreign,
                &[
                    "worktree",
                    "add",
                    "--quiet",
                    "--detach",
                    linked.to_str().unwrap_or_else(|| unreachable!()),
                ],
            );
            linked
        }
        "symlink" => {
            let link = owner.root().join("link");
            std::os::unix::fs::symlink(&foreign, &link).unwrap_or_else(|e| unreachable!("{e}"));
            link
        }
        _ => foreign.clone(),
    };
    let marker = if shape == "bare" {
        foreign.join("HEAD")
    } else if shape == "linked" {
        target.join(".git")
    } else {
        foreign.join(".git/HEAD")
    };
    (target, foreign, marker)
}

#[test]
fn foreign_checkout_targets_are_preserved_by_save_and_restore() {
    let mut failures = Vec::new();
    for operation in ["save", "restore"] {
        for shape in ["checkout", "source", "linked", "bare", "symlink"] {
            let root = Repo::new("foreign-caller");
            root.write("src/lib.rs", "caller checkout\n");
            let owner = plain_owner();
            let (target, foreign, marker) = foreign_target(&owner, shape);
            fs::create_dir_all(target.join(".velnor")).unwrap_or_else(|e| unreachable!("{e}"));
            let snapshot = target.join(".velnor/inputs.tsv");
            fs::write(&snapshot, "invalid\n").unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(target.join("stale-cache"), "cache\n")
                .unwrap_or_else(|e| unreachable!("{e}"));
            let original_marker = fs::read(&marker).unwrap_or_else(|e| unreachable!("{e}"));
            let output = helper(root.root(), operation, &target);
            let intact = !output.status.success()
                && fs::read(&snapshot).is_ok_and(|bytes| bytes == b"invalid\n")
                && fs::read(target.join("stale-cache")).is_ok_and(|bytes| bytes == b"cache\n")
                && fs::read(&marker).is_ok_and(|bytes| bytes == original_marker)
                && (shape == "bare"
                    || fs::read(foreign.join("src/lib.rs"))
                        .is_ok_and(|bytes| bytes == b"foreign checkout\n"))
                && root.root().join("src/lib.rs").is_file();
            if !intact {
                failures.push(format!("{operation} {shape}: {output:?}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn warm_inputs_preserve_a_nested_checkout_and_cold_restore_refuses_to_remove_it() {
    let root = Repo::new("nested-warm-caller");
    root.write("src/lib.rs", "caller checkout\n");
    let saved_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    root.set_mtime("src/lib.rs", saved_time);
    let owner = plain_owner();
    let (target, foreign, marker) = foreign_target(&owner, "container");
    let original_marker = fs::read(&marker).unwrap_or_else(|e| unreachable!("{e}"));
    let saved = helper(root.root(), "save", &target);
    assert!(saved.status.success(), "{saved:?}");
    let snapshot = target.join(".velnor/inputs.tsv");
    let saved_snapshot = fs::read(&snapshot).unwrap_or_else(|e| unreachable!("{e}"));
    assert_ne!(saved_snapshot, [] as [u8; 0]);
    root.set_mtime("src/lib.rs", saved_time + Duration::from_secs(100));
    let restored = helper(root.root(), "restore", &target);
    assert!(restored.status.success(), "{restored:?}");
    assert_eq!(root.mtime("src/lib.rs"), saved_time);
    let tied = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("restored")
        .current_dir(root.root())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(tied.status.success(), "{tied:?}");
    assert_eq!(fs::read(&snapshot).unwrap_or_default(), saved_snapshot);
    fs::write(&snapshot, "invalid\n").unwrap_or_else(|e| unreachable!("{e}"));
    let cold = helper(root.root(), "restore", &target);
    assert!(!cold.status.success(), "{cold:?}");
    assert_eq!(fs::read(&snapshot).unwrap_or_default(), b"invalid\n");
    assert_eq!(fs::read(&marker).unwrap_or_default(), original_marker);
    assert_eq!(
        fs::read(foreign.join("src/lib.rs")).unwrap_or_default(),
        b"foreign checkout\n"
    );
}

#[test]
fn an_external_cache_with_a_foreign_checkout_symlink_is_disposable() {
    let root = Repo::new("foreign-link-caller");
    root.write("src/lib.rs", "caller checkout\n");
    let owner = plain_owner();
    let foreign = owner.root().join("foreign");
    fs::create_dir_all(&foreign).unwrap_or_else(|e| unreachable!("{e}"));
    git(&foreign, &["init", "--quiet"]);
    fs::write(foreign.join("source.txt"), "foreign checkout\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    git(&foreign, &["add", "--all"]);
    let head = fs::read(foreign.join(".git/HEAD")).unwrap_or_else(|e| unreachable!("{e}"));
    let cache = owner.root().join("cache");
    fs::create_dir_all(&cache).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&foreign, cache.join("checkout"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let saved = helper(root.root(), "save", &cache);
    assert!(saved.status.success(), "{saved:?}");
    let restored = helper(root.root(), "restore", &cache);
    assert!(restored.status.success(), "{restored:?}");
    fs::write(cache.join(".velnor/inputs.tsv"), "invalid\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    let cold = helper(root.root(), "restore", &cache);
    assert!(cold.status.success(), "{cold:?}");
    assert!(String::from_utf8_lossy(&cold.stdout).contains("building cold"));
    assert!(!cache.exists());
    assert_eq!(
        fs::read(foreign.join("source.txt")).unwrap_or_default(),
        b"foreign checkout\n"
    );
    assert_eq!(
        fs::read(foreign.join(".git/HEAD")).unwrap_or_default(),
        head
    );
}
