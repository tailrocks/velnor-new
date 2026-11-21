// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::{Path, PathBuf};

use nix::dir::Dir;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;

use super::find_within;
use super::prune::{Identity, descend, remove_tree};

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

fn outside(root: &Path) -> PathBuf {
    let outside = root.join("outside");
    tree(&outside, 3);
    outside
}

fn identity(path: &Path) -> Identity {
    Identity::of(&fs::symlink_metadata(path).unwrap_or_else(|e| unreachable!("{e}")))
}

#[test]
fn a_hooks_directory_swapped_for_a_symlink_refuses_the_save_and_spares_the_target() {
    let root = home("hooks-link");
    let database = database(&root);
    let outside = outside(&root);
    std::os::unix::fs::symlink(&outside, database.join("hooks"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let found = find_within(&root.join(".cargo"), 1_000);
    let left = fs::read_dir(&outside).map_or(0, Iterator::count);
    drop(fs::remove_dir_all(&root));
    assert!(found.is_some_and(|reason| reason.contains("symbolic link")));
    assert_eq!(left, 3);
}

#[test]
fn a_pruned_directory_replaced_by_a_symlink_before_it_is_opened_is_never_followed() {
    let root = home("open-link");
    let database = database(&root);
    let outside = outside(&root);
    let hooks = database.join("hooks");
    tree(&hooks, 1);
    let expected = identity(&hooks);
    fs::remove_dir_all(&hooks).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&outside, &hooks).unwrap_or_else(|e| unreachable!("{e}"));
    let found = remove_tree(&hooks, expected, 0, &mut 1_000);
    let left = fs::read_dir(&outside).map_or(0, Iterator::count);
    drop(fs::remove_dir_all(&root));
    assert!(found.is_some_and(|reason| reason.contains("cannot open")));
    assert_eq!(left, 3);
}

#[test]
fn a_pruned_directory_replaced_by_another_directory_is_left_alone() {
    let root = home("open-swap");
    let database = database(&root);
    let hooks = database.join("hooks");
    tree(&hooks, 1);
    let expected = identity(&hooks);
    fs::remove_dir_all(&hooks).unwrap_or_else(|e| unreachable!("{e}"));
    tree(&hooks, 4);
    let found = remove_tree(&hooks, expected, 0, &mut 1_000);
    let left = fs::read_dir(&hooks).map_or(0, Iterator::count);
    drop(fs::remove_dir_all(&root));
    assert!(found.is_some_and(|reason| reason.contains("replaced")));
    assert_eq!(left, 4);
}

#[test]
fn a_subdirectory_swapped_for_a_symlink_after_it_was_classified_is_never_followed() {
    let root = home("descend-link");
    let database = database(&root);
    let outside = outside(&root);
    let hooks = database.join("hooks");
    fs::create_dir_all(&hooks).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&outside, hooks.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    let parent = Dir::open(&hooks, OFlag::O_RDONLY | OFlag::O_DIRECTORY, Mode::empty())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let found = descend(&parent, c"sub", &hooks.join("sub"), 0, &mut 1_000);
    let left = fs::read_dir(&outside).map_or(0, Iterator::count);
    drop(fs::remove_dir_all(&root));
    assert!(found.is_some_and(|reason| reason.contains("cannot open")));
    assert_eq!(left, 3);
}

#[test]
fn a_symlink_inside_a_hooks_tree_is_removed_without_touching_its_target() {
    let root = home("inner-link");
    let database = database(&root);
    let outside = outside(&root);
    let hooks = database.join("hooks");
    fs::create_dir_all(hooks.join("a")).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&outside, hooks.join("a/link"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let found = find_within(&root.join(".cargo"), 1_000);
    let left = fs::read_dir(&outside).map_or(0, Iterator::count);
    let gone = !hooks.exists();
    drop(fs::remove_dir_all(&root));
    assert_eq!(found, None);
    assert_eq!(left, 3);
    assert!(gone);
}
