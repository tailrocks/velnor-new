// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

use super::new_layout::own_units_1100;
use super::{unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/build/debug/build/lib/f000000000000008/run/stdout";

fn with_build_directory(repo: &Repo, arguments: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", repo.root().join("rust/build"))
        .args(arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn built_apart(left_out: &[&str]) -> Repo {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, "rust/build/debug", left_out);
    unit(&repo, "rust/target/debug/app.d", "app: app/src/lib.rs\n");
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    repo
}

fn stamp(repo: &Repo) {
    let built = with_build_directory(
        repo,
        &[
            "inputs",
            "build",
            "--",
            "touch",
            "-m",
            "-t",
            "206501010000",
            OUTPUT,
        ],
    );
    assert!(built.status.success(), "{built:?}");
}

fn lock(repo: &Repo) -> String {
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn units_in_a_build_directory_apart_from_the_target_directory_are_audited() {
    let repo = built_apart(&[]);
    stamp(&repo);
    let written = with_build_directory(&repo, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn a_file_written_into_the_build_directory_after_the_build_does_not_make_the_inputs_stale() {
    let repo = built_apart(&[]);
    stamp(&repo);
    unit(&repo, "rust/build/debug/late.o", "");
    repo.set_mtime(
        "rust/build/debug/late.o",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let written = with_build_directory(&repo, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn a_unit_without_dep_info_in_a_separate_build_directory_makes_the_audit_refuse() {
    let repo = built_apart(&["lib"]);
    let refused = with_build_directory(&repo, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no current dep-info in") && stderr.contains("lib"),
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
    let refused = with_build_directory(&repo, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no units in") && stderr.contains("rust/build"),
        "{stderr}"
    );
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
