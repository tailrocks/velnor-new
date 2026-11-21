// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;

use super::super::{unit, workspace_below_the_root};
use super::{APART, built_in, from_step_directory, stamp, with_build_directory};
use crate::io::Repo;

#[test]
fn a_unit_without_dep_info_in_a_separate_build_directory_makes_the_audit_refuse() {
    let repo = built_in(APART, &["lib"]);
    let refused = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no current dep-info in")
            && stderr.contains("rust/build/debug")
            && stderr.contains("lib"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_build_directory_that_holds_no_units_while_the_target_holds_outputs_makes_the_audit_refuse() {
    let repo = workspace_below_the_root();
    unit(&repo, "rust/target/debug/app.d", "app: app/src/lib.rs\n");
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    fs::create_dir_all(repo.root().join("rust/build")).unwrap_or_else(|e| unreachable!("{e}"));
    let refused = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no units in") && stderr.contains("rust/build"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

fn refused_for_parent_component(output: &Output, value: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "CARGO_BUILD_BUILD_DIR={value} is relative and contains `..`"
        )),
        "{stderr}"
    );
}

#[test]
fn a_relative_build_directory_with_a_parent_component_is_refused_by_the_audit() {
    let repo = built_in(APART, &[]);
    stamp(&repo, APART);
    let refused = from_step_directory(&repo, "../build", &["inputs", "audit", "--write"]);
    refused_for_parent_component(&refused, "../build");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_relative_build_directory_of_dot_then_parent_is_refused_by_the_audit() {
    let repo = built_in(APART, &[]);
    let refused = from_step_directory(&repo, "./..", &["inputs", "audit", "--write"]);
    refused_for_parent_component(&refused, "./..");
}

#[test]
fn a_relative_build_directory_ending_in_a_parent_component_is_refused_by_the_audit() {
    let repo = built_in(APART, &[]);
    let refused = from_step_directory(&repo, "a/..", &["inputs", "audit", "--write"]);
    refused_for_parent_component(&refused, "a/..");
}

#[test]
fn a_build_directory_through_a_symlink_and_a_parent_component_is_refused_before_cargo_reads_it() {
    let repo = workspace_below_the_root();
    let outside = repo.root().join("rust/lnk-target/inner");
    fs::create_dir_all(&outside).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&outside, repo.root().join("rust/app/lnk"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let audit = from_step_directory(&repo, "lnk/../bd", &["inputs", "audit", "--write"]);
    refused_for_parent_component(&audit, "lnk/../bd");
    let build = from_step_directory(
        &repo,
        "lnk/../bd",
        &["inputs", "build", "--", "cargo", "build"],
    );
    refused_for_parent_component(&build, "lnk/../bd");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
    assert!(!outside.parent().unwrap_or(&outside).join("bd").exists());
}

fn two_standalone_roots() -> Repo {
    let repo = Repo::new("audit-shared-build");
    unit(&repo, ".gitignore", "target/\nbuild/\n");
    unit(&repo, "mise.toml", &crate::toolchain::mise_rust_pin());
    for (directory, name) in [("web", "w"), ("tools/b", "b")] {
        unit(
            &repo,
            &format!("{directory}/Cargo.toml"),
            &format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"),
        );
        unit(&repo, &format!("{directory}/Cargo.lock"), "version = 4\n");
        unit(&repo, &format!("{directory}/src/lib.rs"), "");
    }
    unit(&repo, "build/keep", "");
    repo.git(&["add", "--all"]);
    repo
}

#[test]
fn two_ungrouped_roots_sharing_a_build_directory_are_refused() {
    let repo = two_standalone_roots();
    let refused = with_build_directory(&repo, "build", &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("cannot be told apart"),
        "{refused:?}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_relative_build_directory_with_a_parent_component_is_refused_by_the_build() {
    let repo = workspace_below_the_root();
    let refused = from_step_directory(
        &repo,
        "../build",
        &["inputs", "build", "--", "cargo", "build"],
    );
    refused_for_parent_component(&refused, "../build");
    assert!(!repo.root().join("rust/build").exists());
}
