// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use super::walk::{Budget, Probe, scan_with};

fn linked_directory_root(label: &str) -> PathBuf {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!(
            "velnor-scan-revisit-{label}-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("d")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("d").join("old"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink("d", root.join("link")).unwrap_or_else(|e| unreachable!("{e}"));
    root
}

fn walk(root: &Path, replace: bool) -> io::Result<usize> {
    let replaced = AtomicBool::new(false);
    let mut budget = Budget::new(usize::MAX);
    let scan = scan_with(
        root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            after_open: &|directory| {
                if replace && directory.ends_with("d") && !replaced.swap(true, Ordering::Relaxed) {
                    fs::rename(directory, directory.with_file_name("d.aside"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::create_dir_all(directory).unwrap_or_else(|e| unreachable!("{e}"));
                    fs::write(directory.join("new"), b"").unwrap_or_else(|e| unreachable!("{e}"));
                }
            },
            ..Probe::default()
        },
    );
    scan.map(|scan| scan.entries.len())
}

#[test]
fn a_directory_replaced_after_it_was_scanned_then_linked_to_fails_the_scan() {
    let root = linked_directory_root("replaced");
    let failure = walk(&root, true);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let error = failure.expect_err("kept the old listing of a replaced directory");
    assert!(
        error
            .to_string()
            .contains("changed while the audit scanned"),
        "{error}"
    );
}

#[test]
fn a_directory_linked_to_after_it_was_scanned_unchanged_is_not_an_error() {
    let root = linked_directory_root("stable");
    let walked = walk(&root, false);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(walked.unwrap_or_else(|e| unreachable!("{e}")), 2);
}
