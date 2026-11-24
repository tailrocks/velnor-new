// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::PathBuf;

use super::protect_tree_with;
use crate::error::CiError;

fn fixture(name: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("fixture step");
    let root = exe
        .parent()
        .expect("fixture step")
        .join(format!("velnor-protect-tree-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("gone")).expect("fixture step");
    root
}

#[test]
fn checkout_under_target_is_refused() {
    let root = fixture("checkout");
    fs::create_dir_all(root.join("repo").join(".git")).expect("fixture step");
    let result = protect_tree_with(&root, &mut |_| {});
    fs::remove_dir_all(&root).expect("fixture step");
    assert!(result.is_err());
}

#[test]
fn directory_removed_after_listing_passes() {
    let root = fixture("vanished");
    let result = protect_tree_with(&root, &mut |dir| {
        if dir.ends_with("gone") {
            fs::remove_dir_all(dir).expect("fixture step");
        }
    });
    fs::remove_dir_all(&root).expect("fixture step");
    assert!(result.is_ok());
}

#[test]
fn checkout_renamed_into_unvisited_place_is_refused() {
    let root = fixture("renamed");
    let mut moved = false;
    let result = protect_tree_with(&root, &mut |dir| {
        if dir.ends_with("gone") && !moved {
            moved = true;
            // The listed directory is renamed, and a checkout now sits where the walk has not been.
            fs::rename(dir, root.join("moved")).expect("fixture step");
            fs::create_dir_all(root.join("moved").join(".git")).expect("fixture step");
        }
    });
    fs::remove_dir_all(&root).expect("fixture step");
    let error: CiError = result.expect_err("walk should fail");
    assert!(error.to_string().contains("Git checkout"), "{error}");
}
