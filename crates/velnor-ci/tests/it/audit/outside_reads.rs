// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! What the audit does with a dep-info path outside the repository.

use std::fs;

use super::{audit, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

const TARGET: &str = "rust/target/debug";
const APP_DEPS: &str = "rust/target/debug/deps/app-f000000000000001.d";
const LIB_DEPS: &str = "rust/target/debug/deps/lib-f000000000000005.d";
const LIB_OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008";
const APP_OUTPUT: &str = "rust/target/debug/build/app-f000000000000004/output";
const SOMEWHERE: &str = "/velnor-test-elsewhere";

fn units() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, TARGET, &[]);
    unit(&repo, APP_OUTPUT, "cargo:rerun-if-changed=build.rs\n");
    super::inventory::build(&repo, APP_OUTPUT, None);
    repo
}

fn lock(repo: &Repo) -> Option<String> {
    fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock")).ok()
}

fn written(repo: &Repo) -> String {
    let result = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_HOME", format!("{SOMEWHERE}/cargo-home"))
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.status.success(), "{result:?}");
    lock(repo).unwrap_or_default()
}

#[test]
fn a_dep_info_read_outside_the_repository_in_no_output_directory_refuses_the_audit() {
    let repo = units();
    unit(
        &repo,
        APP_DEPS,
        &format!("out.d: app/src/lib.rs {SOMEWHERE}/docs/x.md\n"),
    );
    let refused = audit(&repo, &["--write"]);
    let message = String::from_utf8_lossy(&refused.stderr);
    assert_eq!(refused.status.code(), Some(1), "{message}");
    assert!(message.contains("outside the repository"), "{message}");
    assert!(
        message.contains(&format!("{SOMEWHERE}/docs/x.md")),
        "{message}"
    );
    assert!(lock(&repo).is_none());
}

#[test]
fn a_dep_info_read_under_a_recorded_build_script_output_directory_is_audited() {
    let repo = units();
    unit(
        &repo,
        &format!("{LIB_OUTPUT}/root-output"),
        &format!("{SOMEWHERE}/out/lib\n"),
    );
    unit(
        &repo,
        LIB_DEPS,
        &format!("out.d: lib/src/lib.rs {SOMEWHERE}/out/lib/o.txt\n"),
    );
    assert_eq!(written(&repo), ".\tlib\n");
}

#[test]
fn a_dep_info_read_under_the_cargo_home_registry_is_audited() {
    let repo = units();
    unit(
        &repo,
        APP_DEPS,
        &format!(
            "out.d: app/src/lib.rs {SOMEWHERE}/cargo-home/registry/src/index/dep-1.0.0/src/lib.rs\n"
        ),
    );
    assert!(!written(&repo).contains("dep-1.0.0"));
}

#[test]
fn an_undeclared_build_script_without_a_root_output_credits_a_dot_line_to_its_package() {
    let repo = units();
    fs::remove_file(repo.root().join(format!("{LIB_OUTPUT}/root-output")))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(written(&repo), ".\tlib\n");
}

#[test]
fn the_output_directory_of_another_package_credits_no_dot_line() {
    let repo = units();
    unit(
        &repo,
        &format!("{LIB_OUTPUT}/root-output"),
        &format!("{SOMEWHERE}/out/lib\n"),
    );
    unit(
        &repo,
        APP_DEPS,
        &format!("out.d: app/src/lib.rs {SOMEWHERE}/out/lib/o.txt\n"),
    );
    let lock = written(&repo);
    assert!(!lock.contains("\tapp"), "{lock}");
}
