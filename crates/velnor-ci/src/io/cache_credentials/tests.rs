// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use super::find_within;

fn home(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("velnor-credentials-{name}-{}", std::process::id()));
    drop(fs::remove_dir_all(&root));
    root
}

fn database(root: &Path) -> PathBuf {
    let database = root.join(".cargo/git/db/shared-1");
    fs::create_dir_all(&database).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(database.join("HEAD"), "ref: refs/heads/main\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(database.join("config"), "[core]\n\tbare = true\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    database
}

fn tree(directory: &Path, files: usize) {
    fs::create_dir_all(directory).unwrap_or_else(|e| unreachable!("{e}"));
    for each in 0..files {
        fs::write(directory.join(format!("hook-{each}")), "x")
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
}

#[test]
fn a_hooks_tree_larger_than_the_entry_budget_refuses_the_save_and_is_not_walked_in_full() {
    let root = home("entries");
    let hooks = database(&root).join("hooks");
    tree(&hooks.join("a"), 40);
    let found = find_within(&root.join(".cargo"), 30);
    let left = fs::read_dir(hooks.join("a")).map_or(0, Iterator::count);
    drop(fs::remove_dir_all(&root));
    assert!(found.is_some_and(|reason| reason.contains("too many entries")));
    assert!(left > 0);
}

#[test]
fn a_hooks_tree_within_the_entry_budget_is_removed() {
    let root = home("within");
    let database = database(&root);
    tree(&database.join("hooks/a/b"), 5);
    tree(&database.join("logs/refs"), 5);
    let found = find_within(&root.join(".cargo"), 1_000);
    let hooks = database.join("hooks").exists();
    let logs = database.join("logs").exists();
    drop(fs::remove_dir_all(&root));
    assert_eq!(found, None);
    assert!(!hooks && !logs);
}

#[test]
fn a_logs_tree_nested_beyond_the_depth_limit_refuses_the_save() {
    let root = home("depth");
    let mut deep = database(&root).join("logs");
    for _ in 0..40 {
        deep.push("d");
    }
    tree(&deep, 1);
    let found = find_within(&root.join(".cargo"), 1_000);
    drop(fs::remove_dir_all(&root));
    assert!(found.is_some_and(|reason| reason.contains("nested too deeply")));
}
