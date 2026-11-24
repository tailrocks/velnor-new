// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Output;

use super::separate_build::two_standalone_roots;
use super::{unit, velnor_ci, workspace_below_the_root};

fn audit_from_rust(root: &Path, value: &str) -> Output {
    velnor_ci()
        .current_dir(root.join("rust"))
        .env("CARGO_BUILD_BUILD_DIR", value)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_relative_build_directory_is_audited_where_cargo_dash_c_writes_it() {
    let repo = workspace_below_the_root();
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", "bd")
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "rust",
            "build",
            "--workspace",
            "--all-targets",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    let written = repo.root().join("rust/bd");
    assert!(written.join("debug").is_dir(), "{}", written.display());
    assert!(!repo.root().join("bd").exists());
    let relative = audit_from_rust(repo.root(), "bd");
    assert!(relative.status.success(), "{relative:?}");
    let absolute = audit_from_rust(repo.root(), &written.display().to_string());
    assert!(absolute.status.success(), "{absolute:?}");
    assert_eq!(relative.stdout, absolute.stdout);
    assert!(
        !String::from_utf8_lossy(&relative.stdout).contains("from 0 unit files"),
        "{relative:?}"
    );
}

#[test]
fn a_standalone_root_is_probed_from_its_own_directory_not_the_step_that_has_a_target_config() {
    let repo = two_standalone_roots();
    unit(
        &repo,
        "web/.cargo/config.toml",
        "[build]\ntarget-dir = \"../shared\"\n",
    );
    repo.git(&["add", "--all"]);
    let built = velnor_ci()
        .current_dir(repo.root().join("web"))
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(built.status.success(), "{stderr}");
    assert!(!stderr.contains("cannot be told apart"), "{stderr}");
}

#[test]
fn a_cargo_command_validates_a_standalone_root_from_its_own_directory_not_the_step() {
    let repo = two_standalone_roots();
    unit(
        &repo,
        "web/.cargo/config.toml",
        "[build]\ntarget-dir = \"../shared\"\n",
    );
    repo.git(&["add", "--all"]);
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "web",
            "build",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(built.status.success(), "{stderr}");
    assert!(repo.root().join("shared/debug").is_dir());
}

#[test]
fn a_relative_config_file_of_the_command_is_not_applied_from_an_unrelated_roots_directory() {
    let repo = two_standalone_roots();
    unit(&repo, "web/settings.toml", "[build]\njobs = 1\n");
    repo.git(&["add", "--all"]);
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "web",
            "--config",
            "settings.toml",
            "build",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(built.status.success(), "{stderr}");
}

#[test]
fn a_target_variable_of_the_step_does_not_apply_to_a_root_the_step_is_not_in() {
    let repo = two_standalone_roots();
    let shared = repo.root().join("shared");
    for variable in ["CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"] {
        let built = velnor_ci()
            .current_dir(repo.root().join("web"))
            .env(variable, &shared)
            .args(["inputs", "build", "--", "true"])
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        let stderr = String::from_utf8_lossy(&built.stderr);
        assert!(built.status.success(), "{variable}: {stderr}");
        assert!(
            !stderr.contains("cannot be told apart"),
            "{variable}: {stderr}"
        );
    }
}
