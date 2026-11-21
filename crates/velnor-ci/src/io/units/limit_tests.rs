// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::{
    MAX_TARGET_DIRECTORIES_VARIABLE, TARGET_DIRECTORIES_CEILING, directory_limit_exceeded,
};

#[test]
fn a_traversal_at_the_ceiling_says_the_tree_must_shrink_not_raise_the_bound() {
    let error =
        directory_limit_exceeded(Path::new("target/x"), TARGET_DIRECTORIES_CEILING).to_string();
    assert!(
        error.contains(&format!("exceeds {TARGET_DIRECTORIES_CEILING} directories")),
        "{error}"
    );
    assert!(error.contains("reduce or split the target tree"), "{error}");
    assert!(!error.contains("raise"), "{error}");
}

#[test]
fn a_traversal_below_the_ceiling_says_how_to_raise_the_bound() {
    let error = directory_limit_exceeded(Path::new("target/x"), 3).to_string();
    assert!(error.contains(MAX_TARGET_DIRECTORIES_VARIABLE), "{error}");
    assert!(error.contains("raise the bound with"), "{error}");
}
