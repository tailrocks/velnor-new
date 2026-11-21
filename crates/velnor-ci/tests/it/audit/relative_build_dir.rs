// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Output;

use super::{velnor_ci, workspace_below_the_root};
use crate::io::Repo;

fn build_from(repo: &Repo, step: &str, value: &str, cargo: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root().join(step))
        .env("CARGO_BUILD_BUILD_DIR", value)
        .env("RUSTC_BOOTSTRAP", "1")
        .args(["inputs", "build", "--", "cargo"])
        .args(cargo)
        .arg("--offline")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn audit_from(repo: &Repo, step: &str, value: &str) -> Output {
    velnor_ci()
        .current_dir(repo.root().join(step))
        .env("CARGO_BUILD_BUILD_DIR", value)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn audits_where_cargo_wrote(repo: &Repo, step: &str, value: &str, written: &Path) {
    assert!(written.join("debug").is_dir(), "{}", written.display());
    let relative = audit_from(repo, step, value);
    assert!(relative.status.success(), "{relative:?}");
    let absolute = audit_from(repo, step, &written.display().to_string());
    assert!(absolute.status.success(), "{absolute:?}");
    assert_eq!(relative.stdout, absolute.stdout);
    assert!(
        !String::from_utf8_lossy(&relative.stdout).contains("from 0 unit files"),
        "{relative:?}"
    );
}

#[test]
fn a_relative_build_directory_with_a_parent_component_is_audited_where_cargo_writes_it() {
    let repo = workspace_below_the_root();
    let built = build_from(
        &repo,
        "rust/app",
        "../build",
        &["build", "--workspace", "--all-targets"],
    );
    assert!(built.status.success(), "{built:?}");
    assert!(!repo.root().join("rust/app/build").exists());
    audits_where_cargo_wrote(
        &repo,
        "rust/app",
        "../build",
        &repo.root().join("rust/build"),
    );
}

#[test]
fn a_symlink_before_a_parent_component_is_audited_where_cargo_writes_it() {
    let repo = workspace_below_the_root();
    let inner = repo.root().join("rust/lnk-target/inner");
    fs::create_dir_all(&inner).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&inner, repo.root().join("rust/app/lnk"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let built = build_from(
        &repo,
        "rust/app",
        "lnk/../bd",
        &["build", "--workspace", "--all-targets"],
    );
    assert!(built.status.success(), "{built:?}");
    assert!(!repo.root().join("rust/app/bd").exists());
    audits_where_cargo_wrote(
        &repo,
        "rust/app",
        "lnk/../bd",
        &repo.root().join("rust/lnk-target/bd"),
    );
}

#[test]
fn a_relative_build_directory_is_audited_where_cargo_dash_c_writes_it() {
    let repo = workspace_below_the_root();
    let built = build_from(
        &repo,
        "",
        "bd",
        &[
            "-Z",
            "unstable-options",
            "-C",
            "rust",
            "build",
            "--workspace",
            "--all-targets",
        ],
    );
    assert!(built.status.success(), "{built:?}");
    assert!(!repo.root().join("bd").exists());
    audits_where_cargo_wrote(&repo, "rust", "bd", &repo.root().join("rust/bd"));
}
