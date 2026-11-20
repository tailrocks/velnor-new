// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use super::Invocation;

fn words(line: &str) -> Vec<String> {
    line.split(' ').map(str::to_owned).collect()
}

fn selects(line: &str, package: &str) -> bool {
    let command = words(line);
    Invocation::new(&command)
        .selection(&BTreeSet::from(["app", "lib"]))
        .unwrap_or_else(|| unreachable!("a cargo build has a selection"))
        .selects(package)
}

#[test]
fn every_spelling_of_a_member_package_flag_selects_that_member() {
    for line in [
        "cargo build -p lib --offline",
        "cargo build --package lib --offline",
        "cargo build --package=lib --offline",
        "cargo build -plib --offline",
    ] {
        assert!(selects(line, "lib"), "{line}");
        assert!(!selects(line, "app"), "{line}");
    }
}

#[test]
fn every_spelling_of_a_non_member_package_flag_selects_nothing() {
    for line in [
        "cargo build -p no-such --offline",
        "cargo build --package=no-such --offline",
        "cargo build -pno-such --offline",
        "cargo build --package=git+https://example.invalid/app#app@0.1.0 --offline",
    ] {
        assert!(!selects(line, "app"), "{line}");
        assert!(!selects(line, "lib"), "{line}");
    }
}
