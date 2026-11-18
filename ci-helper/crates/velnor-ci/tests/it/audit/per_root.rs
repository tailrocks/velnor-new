// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::Repo;

use super::{audit, own_units, unit, workspace_below_the_root};

fn two_roots(tools_read: &str) -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    repo.write_all(&[
        (
            "tools/Cargo.toml",
            "[package]\nname = \"tools\"\nedition = \"2024\"\n",
        ),
        ("tools/Cargo.lock", "version = 4\n"),
        ("tools/src/main.rs", "fn main() {}\n"),
        ("shared/tools-read.txt", ""),
    ]);
    for (file, fingerprint) in [
        ("tools-f000000000000101", "bin-tools"),
        ("tools-f000000000000102", "test-bin-tools"),
    ] {
        unit(
            &repo,
            &format!("tools/target/debug/deps/{file}.d"),
            &format!("out.d: src/main.rs {tools_read}\n"),
        );
        unit(
            &repo,
            &format!("tools/target/debug/.fingerprint/{file}/{fingerprint}"),
            "",
        );
    }
    repo.git(&["add", "--all"]);
    repo
}

const UNDECLARED: &str = "src/../../shared/tools-read.txt";

#[test]
fn a_per_root_audit_finds_an_undeclared_input_of_that_root() {
    let repo = two_roots(UNDECLARED);
    let audited = audit(&repo, &["--root", "tools"]);
    assert_eq!(audited.status.code(), Some(1), "{audited:?}");
    assert!(
        String::from_utf8_lossy(&audited.stderr).contains("shared/tools-read.txt%09tools"),
        "{audited:?}"
    );
}

#[test]
fn a_per_root_audit_ignores_the_inputs_of_other_roots() {
    let repo = two_roots(UNDECLARED);
    let audited = audit(&repo, &["--root", "rust"]);
    assert_eq!(audited.status.code(), Some(0), "{audited:?}");
}

#[test]
fn a_per_root_audit_does_not_need_other_roots_built() {
    let repo = two_roots("");
    std::fs::remove_dir_all(repo.root().join("tools/target"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let audited = audit(&repo, &["--root", "rust"]);
    assert_eq!(audited.status.code(), Some(0), "{audited:?}");
}

#[test]
fn a_per_root_audit_names_the_roots_when_the_directory_is_not_one() {
    let repo = two_roots("");
    let audited = audit(&repo, &["--root", "nowhere"]);
    assert_eq!(audited.status.code(), Some(1), "{audited:?}");
    assert!(
        String::from_utf8_lossy(&audited.stderr).contains("no Cargo root in nowhere"),
        "{audited:?}"
    );
}

#[test]
fn a_per_root_audit_cannot_write_the_lock() {
    let repo = two_roots("");
    let audited = audit(&repo, &["--root", "rust", "--write"]);
    assert_eq!(audited.status.code(), Some(2), "{audited:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
