// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use nix::fcntl::{OFlag, openat};
use nix::sys::stat::{Mode, mkdirat};
use nix::unistd::symlinkat;

use super::walk::{Budget, Probe, scan_with};

fn looped_root(label: &str) -> PathBuf {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!(
            "velnor-scan-revisit-{label}-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("d/sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("d/old"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    symlink("..", root.join("d/sub/up")).unwrap_or_else(|e| unreachable!("{e}"));
    root
}

#[derive(Clone, Copy)]
enum Swap {
    None,
    Directory,
    Link,
}

fn walk(root: &Path, swap: Swap) -> io::Result<usize> {
    let replaced = AtomicBool::new(false);
    let mut budget = Budget::new(usize::MAX);
    scan_with(
        root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            listed: &|name| {
                if matches!(swap, Swap::None)
                    || name != "up"
                    || replaced.swap(true, Ordering::Relaxed)
                {
                    return;
                }
                let directory = root.join("d");
                let aside = root.join("d.aside");
                fs::rename(&directory, &aside).unwrap_or_else(|e| unreachable!("{e}"));
                if matches!(swap, Swap::Link) {
                    symlink(&aside, &directory).unwrap_or_else(|e| unreachable!("{e}"));
                } else {
                    fs::create_dir_all(directory.join("sub"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::write(directory.join("new"), b"").unwrap_or_else(|e| unreachable!("{e}"));
                }
            },
            ..Probe::default()
        },
    )
    .map(|scan| scan.entries.len())
}

#[test]
fn a_directory_replaced_after_it_was_scanned_then_linked_to_fails_the_scan() {
    let root = looped_root("replaced");
    let walked = walk(&root, Swap::Directory);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let error = walked.expect_err("kept the old listing of a replaced directory");
    assert!(
        error
            .to_string()
            .contains("changed while the audit scanned"),
        "{error}"
    );
}

#[test]
fn a_directory_linked_to_after_it_was_scanned_unchanged_is_not_an_error() {
    let root = looped_root("stable");
    let walked = walk(&root, Swap::None);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(walked.unwrap_or_else(|e| unreachable!("{e}")), 3);
}

#[test]
fn a_directory_replaced_by_a_link_to_the_identical_directory_fails_the_scan() {
    let root = looped_root("linked");
    let walked = walk(&root, Swap::Link);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let error = walked.expect_err("accepted a link to the old directory as the old directory");
    assert!(
        error
            .to_string()
            .contains("changed while the audit scanned"),
        "{error}"
    );
}

fn descend(parent: &OwnedFd, name: &str) -> OwnedFd {
    mkdirat(parent, name, Mode::S_IRWXU).unwrap_or_else(|e| unreachable!("{e}"));
    openat(
        parent,
        name,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn beyond_path_max(label: &str) -> (PathBuf, usize) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-long-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let name = "x".repeat(200);
    let depth = libc::PATH_MAX as usize / (name.len() + 1) + 2;
    let mut held = OwnedFd::from(File::open(&root).unwrap_or_else(|e| unreachable!("{e}")));
    for _ in 0..depth {
        held = descend(&held, &name);
    }
    symlinkat("..", &held, "up").unwrap_or_else(|e| unreachable!("{e}"));
    (root, depth + 1)
}

#[test]
fn a_directory_deeper_than_path_max_linked_back_to_is_revisited_without_a_whole_path_lookup() {
    let (root, directories) = beyond_path_max("revisit");
    let walked = walk(&root, Swap::None);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        walked.unwrap_or_else(|e| panic!("an unchanged deep tree failed the scan: {e}")),
        directories
    );
}
