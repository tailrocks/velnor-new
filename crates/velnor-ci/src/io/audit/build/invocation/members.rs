// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::Path;

use velnor_model::{PackageName, RepoPath, RustMember, RustRoot};

use super::invoked_members;

fn name(value: &str) -> PackageName {
    PackageName::new(value).unwrap_or_else(|e| unreachable!("{e}"))
}

fn root(manifest: &str, members: &[&str]) -> RustRoot {
    let directory = manifest.strip_suffix("Cargo.toml").unwrap_or("");
    RustRoot {
        manifest: RepoPath::new(manifest).unwrap_or_else(|e| unreachable!("{e}")),
        target_dir: RepoPath::new("target").unwrap_or_else(|e| unreachable!("{e}")),
        nextest_ci_profile: false,
        members: members
            .iter()
            .map(|member| RustMember {
                name: name(member),
                manifest: RepoPath::new(format!("{directory}{member}/Cargo.toml"))
                    .unwrap_or_else(|e| unreachable!("{e}")),
                directory: None,
                dependencies: Vec::new(),
                targets: Vec::new(),
                custom_harnesses: Vec::new(),
            })
            .collect(),
        depends_on: Vec::new(),
        external_dependencies: Vec::new(),
    }
}

fn names(roots: &[RustRoot], location: &str) -> BTreeSet<String> {
    invoked_members(roots, Path::new("/repo"), Path::new(location))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_command_selects_only_the_members_of_the_root_it_runs_in() {
    let roots = [
        root("rust/Cargo.toml", &["app", "lib"]),
        root("other/Cargo.toml", &["other"]),
    ];
    assert_eq!(names(&roots, "/repo/rust"), set(&["app", "lib"]));
    assert_eq!(names(&roots, "/repo/other"), set(&["other"]));
}

#[test]
fn a_command_run_below_a_root_uses_that_root() {
    let roots = [
        root("rust/Cargo.toml", &["app", "lib"]),
        root("other/Cargo.toml", &["other"]),
    ];
    assert_eq!(names(&roots, "/repo/rust/lib"), set(&["app", "lib"]));
}

#[test]
fn the_nearest_root_wins_over_an_enclosing_root() {
    let roots = [
        root("Cargo.toml", &["top"]),
        root("rust/Cargo.toml", &["app"]),
    ];
    assert_eq!(names(&roots, "/repo/rust"), set(&["app"]));
    assert_eq!(names(&roots, "/repo/docs"), set(&["top"]));
}

#[test]
fn a_location_outside_every_root_selects_nothing() {
    let roots = [root("rust/Cargo.toml", &["app"])];
    assert_eq!(names(&roots, "/elsewhere"), set(&[]));
}
