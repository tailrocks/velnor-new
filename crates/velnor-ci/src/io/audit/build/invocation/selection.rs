// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::Invocation;

fn selected(line: &str, package: &str) -> bool {
    let command: Vec<String> = line.split(' ').map(str::to_owned).collect();
    let members = BTreeSet::from(["app", "lib", "tool"]);
    Invocation::new(&command)
        .selection(&members)
        .unwrap_or_default()
        .selects(package)
}

#[test]
fn a_workspace_run_minus_an_exclusion_selects_every_other_member() {
    for line in [
        "cargo build --workspace --exclude app",
        "cargo build --workspace --exclude=app",
        "cargo build --all --exclude app",
    ] {
        assert!(selected(line, "lib"), "{line}");
        assert!(selected(line, "tool"), "{line}");
        assert!(!selected(line, "app"), "{line}");
    }
}

#[test]
fn several_exclusions_each_remove_a_member() {
    let line = "cargo test --workspace --exclude app --exclude=tool";
    assert!(selected(line, "lib"));
    assert!(!selected(line, "app"));
    assert!(!selected(line, "tool"));
}

#[test]
fn an_exclusion_that_names_no_exact_member_selects_nothing_it_cannot_prove() {
    for line in [
        "cargo build --workspace --exclude a*",
        "cargo build --workspace --exclude lib@0.1.0",
    ] {
        assert!(!selected(line, "lib"), "{line}");
        assert!(!selected(line, "app"), "{line}");
    }
}

#[test]
fn a_plain_workspace_run_selects_every_member() {
    assert!(selected("cargo build --workspace", "app"));
    assert!(selected("cargo build --workspace", "lib"));
}

#[test]
fn an_explicit_package_beside_an_exclusion_stays_selected() {
    let line = "cargo build --workspace --exclude app -p app";
    assert!(selected(line, "app"));
}
