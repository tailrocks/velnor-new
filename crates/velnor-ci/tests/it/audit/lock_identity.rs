// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;
use std::fs;

use crate::io::Repo;

use super::{audit, path_dependency_units, root_package_with_path_dependencies, unit};

const HELPER_LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");

const NAME_ONLY: &str = "LICENSE\tother\nNOTICE\tother\nREADME.md\ttop\ndata/ext.txt\tother\ndata/ext.txt\ttop\ndata/qa.txt\ttop\next.rs\tother\next.rs\ttop\nqa.rs\ttop\nschema/future.json\tother\nshared/link.txt\ttop\n";

fn written_under(pin: &str) -> String {
    let repo = root_package_with_path_dependencies();
    unit(
        &repo,
        ".velnor/rust-ci.lock",
        &HELPER_LOCK.replace("version = \"0.1.81\"", &format!("version = \"{pin}\"")),
    );
    repo.git(&["add", "--all"]);
    path_dependency_units(&repo);
    let written = audit(&repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    read_lock(&repo)
}

fn read_lock(repo: &Repo) -> String {
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_pin_before_0_1_100_keeps_the_name_only_lock_format_it_can_read() {
    assert_eq!(written_under("0.1.99"), NAME_ONLY);
}

#[test]
fn a_pin_of_0_1_100_gets_the_manifest_of_each_package_on_every_line() {
    let lock = written_under("0.1.100");
    let mut expected = String::new();
    for line in NAME_ONLY.lines() {
        let manifest = if line.ends_with("\ttop") {
            "Cargo.toml"
        } else {
            "other/Cargo.toml"
        };
        writeln!(expected, "{line}\t{manifest}").unwrap_or_else(|e| unreachable!("{e}"));
    }
    assert_eq!(lock, expected);
}

#[test]
fn a_name_only_lock_still_passes_the_audit_under_a_pin_that_reads_manifests() {
    let repo = root_package_with_path_dependencies();
    unit(
        &repo,
        ".velnor/rust-ci.lock",
        &HELPER_LOCK.replace("version = \"0.1.81\"", "version = \"0.1.100\""),
    );
    unit(&repo, ".velnor/rust-inputs.lock", NAME_ONLY);
    repo.git(&["add", "--all"]);
    path_dependency_units(&repo);
    let current = audit(&repo, &[]);
    assert_eq!(current.status.code(), Some(0), "{current:?}");
}
