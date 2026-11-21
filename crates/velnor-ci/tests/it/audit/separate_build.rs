// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

mod refusals;

use velnor_test_support::real_cargo;

use super::migration::app_and_lib_project;
use super::new_layout::own_units_1100;
use super::{command, unit, velnor_ci, workspace_below_the_root};
use crate::io::Repo;

pub(super) const APART: &str = "rust/build";
const NESTED: &str = "rust/target/bd";

fn output(directory: &str) -> String {
    format!("{directory}/debug/build/lib/f000000000000008/run/stdout")
}

pub(super) fn with_build_directory(repo: &Repo, directory: &str, arguments: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", repo.root().join(directory))
        .args(arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

pub(super) fn built_in(directory: &str, left_out: &[&str]) -> Repo {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, &format!("{directory}/debug"), left_out);
    unit(&repo, "rust/target/debug/app.d", "app: app/src/lib.rs\n");
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    unit(&repo, &output(directory), "cargo:rerun-if-changed=../..\n");
    repo
}

pub(super) fn stamp(repo: &Repo, directory: &str) {
    let built = with_build_directory(
        repo,
        directory,
        &[
            "inputs",
            "build",
            "--",
            "touch",
            "-m",
            "-t",
            "206501010000",
            &output(directory),
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
    let repo = built_in(APART, &[]);
    stamp(&repo, APART);
    let written = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn a_file_written_into_the_build_directory_after_the_build_does_not_make_the_inputs_stale() {
    let repo = built_in(APART, &[]);
    stamp(&repo, APART);
    unit(&repo, "rust/build/debug/late.o", "");
    repo.set_mtime(
        "rust/build/debug/late.o",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    let written = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

fn unit_count(output: &Output) -> usize {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (_, tail) = stdout
        .split_once(" reads from ")
        .unwrap_or_else(|| unreachable!("{stdout}"));
    tail.split_whitespace()
        .next()
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| unreachable!("{stdout}"))
}

#[test]
fn a_build_directory_inside_the_target_directory_counts_each_unit_once() {
    let apart = built_in(APART, &[]);
    stamp(&apart, APART);
    let separate = with_build_directory(&apart, APART, &["inputs", "audit", "--write"]);
    assert!(separate.status.success(), "{separate:?}");

    let nested = built_in(NESTED, &[]);
    stamp(&nested, NESTED);
    let inside = with_build_directory(&nested, NESTED, &["inputs", "audit", "--write"]);
    assert!(inside.status.success(), "{inside:?}");
    assert_eq!(unit_count(&inside), unit_count(&separate));
    assert_eq!(lock(&nested), ".\tlib\n");
}

fn cargo_into_nested(repo: &Repo, wrapped: bool) -> Output {
    let mut cargo = if wrapped {
        let mut cargo = command(env!("CARGO_BIN_EXE_velnor-ci"));
        cargo.args(["inputs", "build", "--", "cargo"]);
        cargo
    } else {
        command(real_cargo())
    };
    cargo
        .current_dir(repo.root().join("rust"))
        .env("CARGO_BUILD_BUILD_DIR", repo.root().join(NESTED))
        .args(["build", "--workspace", "--all-targets", "--offline"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn an_untrusted_build_output_in_a_build_directory_inside_the_target_is_invalidated_once() {
    let repo = app_and_lib_project();
    let plain = cargo_into_nested(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let rebuilt = cargo_into_nested(&repo, true);
    assert!(rebuilt.status.success(), "{rebuilt:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join("rust/target/lib-runs.txt"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "2"
    );
}

#[test]
fn a_relative_build_directory_names_the_same_directory_for_the_workspace_and_the_command() {
    let repo = workspace_below_the_root();
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", "build")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "build",
            "--manifest-path",
            "rust/Cargo.toml",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert!(repo.root().join("build/debug").is_dir(), "{built:?}");
}

pub(super) fn from_step_directory(repo: &Repo, value: &str, arguments: &[&str]) -> Output {
    velnor_ci()
        .current_dir(repo.root().join("rust/app"))
        .env("CARGO_BUILD_BUILD_DIR", value)
        .args(arguments)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_relative_build_directory_through_a_symlink_is_audited_where_cargo_writes_it() {
    let repo = workspace_below_the_root();
    let target = repo.root().join("rust/lnk-target");
    fs::create_dir_all(&target).unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&target, repo.root().join("rust/app/lnk"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let built = from_step_directory(
        &repo,
        "lnk/bd",
        &[
            "inputs",
            "build",
            "--",
            "cargo",
            "build",
            "--workspace",
            "--all-targets",
        ],
    );
    assert!(built.status.success(), "{built:?}");
    assert!(target.join("bd/debug").is_dir(), "{built:?}");
    let through_link = from_step_directory(&repo, "lnk/bd", &["inputs", "audit", "--write"]);
    assert!(through_link.status.success(), "{through_link:?}");
    let direct = target.join("bd").display().to_string();
    let direct = from_step_directory(&repo, &direct, &["inputs", "audit", "--write"]);
    assert!(direct.status.success(), "{direct:?}");
    assert_eq!(through_link.stdout, direct.stdout);
    assert!(
        !String::from_utf8_lossy(&through_link.stdout).contains("from 0 unit files"),
        "{through_link:?}"
    );
}

fn link_to_build(repo: &Repo) {
    std::os::unix::fs::symlink("../build", repo.root().join("rust/app/link-to-build"))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_relative_build_directory_through_a_symlink_without_parent_components_is_audited_to_success() {
    let repo = built_in(APART, &[]);
    link_to_build(&repo);
    stamp(&repo, APART);
    let written = from_step_directory(&repo, "link-to-build", &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn an_absolute_build_directory_through_a_symlink_is_audited_to_success() {
    let repo = built_in(APART, &[]);
    link_to_build(&repo);
    stamp(&repo, APART);
    let through_link = repo.root().join("rust/app/link-to-build");
    let written = from_step_directory(
        &repo,
        &through_link.to_string_lossy(),
        &["inputs", "audit", "--write"],
    );
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn obsolete_units_in_the_target_directory_do_not_count_once_a_build_directory_is_set() {
    let repo = built_in(APART, &[]);
    stamp(&repo, APART);
    unit(
        &repo,
        "rust/target/debug/.fingerprint/gone-f0000000000000ff/lib-gone",
        "",
    );
    unit(
        &repo,
        "rust/target/debug/deps/gone-f0000000000000ff.d",
        "out.d: app/src/lib.rs\n",
    );
    repo.set_mtime(
        "rust/target/debug/deps/gone-f0000000000000ff.d",
        UNIX_EPOCH + Duration::from_secs(1_000_000_000),
    );
    let written = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(lock(&repo), ".\tlib\n");
}

#[test]
fn units_in_the_target_directory_do_not_hide_a_build_directory_in_an_unknown_layout() {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, "rust/target/debug", &[]);
    unit(&repo, "rust/build/debug/units/lib/x/liblib.rlib", "");
    let refused = with_build_directory(&repo, APART, &["inputs", "audit", "--write"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("found no units in"),
        "{refused:?}"
    );
}

#[test]
fn a_cargo_that_prints_no_build_directory_is_audited_in_the_target_directory() {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, "rust/target/debug", &[]);
    let shims = repo.root().join("../cargo-shim");
    fs::create_dir_all(&shims).unwrap_or_else(|e| unreachable!("{e}"));
    let shim = shims.join("cargo");
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nif [ \"$1\" = metadata ]; then\n  {real} \"$@\" | sed -e 's/,\"build_directory\":\"[^\"]*\"//' -e 's/\"build_directory\":\"[^\"]*\",//'\nelse\n  exec {real} \"$@\"\nfi\n",
            real = real_cargo().display()
        ),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&shim, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        shims.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let written = velnor_ci()
        .current_dir(repo.root())
        .env("PATH", path)
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let _ignored = fs::remove_dir_all(&shims);
    assert!(written.status.success(), "{written:?}");
    assert!(unit_count(&written) > 0, "{written:?}");
}
