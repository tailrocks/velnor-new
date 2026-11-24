// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use super::separate_build::with_build_directory;
use super::workspace_below_the_root;
use crate::io::Repo;

fn refused(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_holds_tracked_input(repo: &Repo, directory: &str, arguments: &[&str]) {
    let stderr = refused(&with_build_directory(repo, directory, arguments));
    assert!(
        stderr.contains("holds the tracked input"),
        "{directory} {arguments:?}: {stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_build_directory_that_holds_tracked_inputs_is_refused_by_the_audit_and_the_build() {
    for directory in [".", "rust", "rust/app"] {
        let repo = workspace_below_the_root();
        assert_holds_tracked_input(&repo, directory, &["inputs", "audit", "--write"]);
        assert_holds_tracked_input(&repo, directory, &["inputs", "build", "--", "true"]);
    }
}

#[test]
fn a_build_directory_named_like_the_start_of_a_tracked_directory_is_not_refused_for_overlap() {
    let repo = workspace_below_the_root();
    let output = with_build_directory(&repo, "rust/app-build", &["inputs", "build", "--", "true"]);
    assert!(output.status.success(), "{output:?}");
}
