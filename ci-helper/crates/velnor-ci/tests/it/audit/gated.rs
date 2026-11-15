// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use super::{audit, own_units_in, unit, workspace_below_the_root};
use crate::io::Repo;

const DEBUG: &str = "rust/target/debug";
const TRIPLE: &str = "rust/target/x86_64-unknown-linux-gnu/debug";
const INACTIVE: &str = "cfg(target_os = \"no-such-os\")";
const ACTIVE: &str = "cfg(not(target_os = \"no-such-os\"))";

fn package(name: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\nedition = \"2024\"\n\n{extra}")
}

fn finish(repo: &Repo) {
    repo.git(&["add", "--all"]);
    own_units_in(repo, DEBUG, TRIPLE, &[]);
}

fn direct(platform: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package(
            "app",
            &format!("[target.'{platform}'.build-dependencies]\nlib = {{ path = \"../lib\" }}\n"),
        ),
    );
    finish(&repo);
    repo
}

fn transitive(platform: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package(
            "app",
            "[build-dependencies]\nmacros = { path = \"../macros\" }\n",
        ),
    );
    unit(
        &repo,
        "rust/macros/Cargo.toml",
        &package(
            "macros",
            &format!(
                "[lib]\nproc-macro = true\n\n[target.'{platform}'.dependencies]\nlib = {{ path = \"../lib\" }}\n"
            ),
        ),
    );
    finish(&repo);
    repo
}

fn unowned(platform: &str) -> Repo {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/Cargo.toml",
        &package("app", "[dependencies]\nmid = { path = \"../../mid\" }\n"),
    );
    unit(
        &repo,
        "mid/Cargo.toml",
        &package(
            "mid",
            &format!(
                "[target.'{platform}'.build-dependencies]\nlib = {{ path = \"../rust/lib\" }}\n"
            ),
        ),
    );
    unit(&repo, "mid/src/lib.rs", "");
    unit(&repo, ".velnor/rust-ci.toml", "exclude = [\"mid\"]\n");
    finish(&repo);
    repo
}

fn passes(repo: &Repo) {
    let output = audit(repo, &["--write"]);
    assert!(output.status.success(), "{output:?}");
}

fn needs_the_host_units_of_lib(repo: &Repo) {
    let output = audit(repo, &["--write"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("for lib (rust/lib/Cargo.toml)"),
        "{output:?}"
    );
}

#[test]
fn a_build_dependency_under_an_inactive_cfg_needs_no_host_units() {
    passes(&direct(INACTIVE));
}

#[test]
fn a_build_dependency_under_an_inactive_triple_needs_no_host_units() {
    passes(&direct("x86_64-no-such-vendor-none"));
}

#[test]
fn a_build_dependency_under_an_active_cfg_still_needs_its_host_units() {
    needs_the_host_units_of_lib(&direct(ACTIVE));
}

#[test]
fn a_build_dependency_under_a_target_feature_cfg_still_needs_its_host_units() {
    needs_the_host_units_of_lib(&direct("cfg(target_feature = \"no-such-feature\")"));
}

#[test]
fn a_dependency_of_a_host_package_under_an_inactive_cfg_needs_no_host_units() {
    passes(&transitive(INACTIVE));
}

#[test]
fn a_dependency_of_a_host_package_under_an_active_cfg_needs_its_host_units() {
    needs_the_host_units_of_lib(&transitive(ACTIVE));
}

#[test]
fn a_package_outside_the_workspaces_with_an_inactive_build_dependency_needs_no_host_units() {
    passes(&unowned(INACTIVE));
}

#[test]
fn a_package_outside_the_workspaces_with_an_active_build_dependency_needs_its_host_units() {
    needs_the_host_units_of_lib(&unowned(ACTIVE));
}

#[test]
fn a_rustc_that_prints_no_host_triple_fails_the_audit() {
    let repo = direct(INACTIVE);
    let shims = repo.root().join("shims");
    fs::create_dir_all(&shims).unwrap_or_else(|e| unreachable!("{e}"));
    let rustc = shims.join("rustc");
    fs::write(&rustc, "#!/bin/sh\necho 'release: 1.99.1'\n")
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&rustc, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        shims.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .env("PATH", path)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no host triple"),
        "{output:?}"
    );
}
