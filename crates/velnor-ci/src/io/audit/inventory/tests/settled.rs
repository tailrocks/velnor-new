// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;
use std::time::SystemTime;

use super::super::latest::{Hooks, newest_with};

use super::*;

fn tree(name: &str, build: &dyn Fn(&Path)) -> PathBuf {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-settled-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    build(&root);
    root
}

fn raced_scan_is_not_fresh(root: &Path, disturb: &dyn Fn(&Path)) -> io::Result<Option<SystemTime>> {
    let scan = walk(
        root,
        &BTreeSet::new(),
        &mut Budget::new(usize::MAX),
        &Probe {
            before_open: &|directory| {
                disturb(directory);
                Ok(())
            },
            ..Probe::default()
        },
    );
    let newest = scan.map(|scan| newest_with(&scan, &scan.entries, &Hooks::default()));
    fs::remove_dir_all(root).unwrap_or_else(|e| unreachable!("{e}"));
    newest.unwrap_or(Ok(None))
}

fn assert_refused(newest: &io::Result<Option<SystemTime>>) {
    assert!(matches!(newest, Ok(None)), "{newest:?}");
}

#[test]
fn a_nested_ancestor_replaced_after_its_children_were_queued_is_not_accepted_as_fresh() {
    let root = tree("ancestor", &|root| {
        fs::create_dir_all(root.join("a/b")).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let newest = raced_scan_is_not_fresh(&root, &|directory| {
        if directory.ends_with("a/b") {
            let a = directory.parent().unwrap_or_else(|| unreachable!());
            fs::remove_dir_all(a).unwrap_or_else(|e| unreachable!("{e}"));
            fs::write(a, b"").unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    assert_refused(&newest);
}

#[test]
fn an_alias_to_a_directory_removed_before_it_is_opened_is_not_accepted_as_fresh() {
    let root = tree("alias", &|root| {
        fs::create_dir(root.join("a")).unwrap_or_else(|e| unreachable!("{e}"));
        symlink("a", root.join("z")).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let newest = raced_scan_is_not_fresh(&root, &|directory| {
        if directory.ends_with("a") {
            fs::remove_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    assert_refused(&newest);
}

#[test]
fn a_directory_renamed_and_replaced_by_a_link_to_it_is_not_accepted_as_fresh() {
    let root = tree("renamed", &|root| {
        fs::create_dir(root.join("d")).unwrap_or_else(|e| unreachable!("{e}"));
    });
    let newest = raced_scan_is_not_fresh(&root, &|directory| {
        if directory.ends_with("d") {
            let moved = directory.with_file_name("moved");
            fs::rename(directory, &moved).unwrap_or_else(|e| unreachable!("{e}"));
            symlink(&moved, directory).unwrap_or_else(|e| unreachable!("{e}"));
        }
    });
    assert_refused(&newest);
}
