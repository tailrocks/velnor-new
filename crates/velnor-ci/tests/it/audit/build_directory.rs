// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use super::{audit, own_units, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

fn refused(output: &Output) -> String {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn build(repo: &Repo, arguments: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", "cargo"])
        .args(arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_build_directory_set_in_a_member_directory_makes_the_audit_refuse_and_names_the_file() {
    for (file, config) in [
        (
            "rust/app/.cargo/config.toml",
            "[build]\nbuild-dir = \"bld\"\n",
        ),
        ("rust/lib/.cargo/config", "build.build-dir = \"bld\"\n"),
        (
            "rust/macros/.cargo/config.toml",
            "build = { build-dir = \"bld\" }\n",
        ),
    ] {
        let repo = workspace_below_the_root();
        unit(&repo, file, config);
        let stderr = refused(&audit(&repo, &["--write"]));
        assert!(stderr.contains(file), "{file}: {stderr}");
        assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
    }
}

fn audited(repo: &Repo) {
    own_units(repo, "rust/target/debug", &[]);
    let written = audit(repo, &["--write"]);
    assert!(written.status.success(), "{written:?}");
    assert!(repo.root().join(".velnor/rust-inputs.lock").is_file());
}

#[test]
fn a_member_cargo_configuration_without_a_build_directory_does_not_stop_the_audit() {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/.cargo/config.toml",
        "[build]\njobs = 2\n\n[term]\nquiet = false\n",
    );
    audited(&repo);
}

#[test]
fn a_member_configuration_that_includes_a_file_makes_the_audit_refuse_and_names_the_file() {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/.cargo/config.toml",
        "include = \"extra.toml\"\n",
    );
    unit(
        &repo,
        "rust/app/.cargo/extra.toml",
        "[build]\nbuild-dir = \"bld\"\n",
    );
    let stderr = refused(&audit(&repo, &["--write"]));
    assert!(stderr.contains("rust/app/.cargo/config.toml"), "{stderr}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_member_config_file_without_an_extension_shadows_the_toml_one() {
    let repo = workspace_below_the_root();
    unit(&repo, "rust/app/.cargo/config", "[build]\njobs = 2\n");
    unit(
        &repo,
        "rust/app/.cargo/config.toml",
        "[build]\nbuild-dir = \"bld\"\n",
    );
    unit(&repo, "rust/lib/.cargo/config", "[build]\njobs = 2\n");
    unit(&repo, "rust/lib/.cargo/config.toml", "not = [valid\n");
    audited(&repo);
}

#[test]
fn a_build_command_naming_a_build_directory_is_refused() {
    let repo = workspace_below_the_root();
    for arguments in [
        &["build", "--build-dir", "x"][..],
        &["build", "--build-dir=x"],
        &["build", "-Zbuild-dir"],
        &["build", "-Z", "build-dir"],
        &["build", "--config", "build.build-dir=\"x\""],
        &["build", "--config=build.build-dir=\"x\""],
        &["--config", "build.build-dir=\"x\"", "build"],
    ] {
        let stderr = refused(&build(&repo, arguments));
        assert!(
            stderr.contains("build directory"),
            "{arguments:?}: {stderr}"
        );
    }
}
