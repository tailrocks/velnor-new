// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::migration::{app_and_lib_project, build, package_output, wrapped_build};
use crate::io::Repo;

fn project_with_lib_targets() -> Repo {
    let repo = app_and_lib_project();
    repo.write(
        "rust/lib/src/lib.rs",
        "/// ```\n/// assert_eq!(lib::one(), 1);\n/// ```\npub fn one() -> u32 {\n    1\n}\n",
    );
    repo.write("rust/lib/src/bin/tool.rs", "fn main() {}\n");
    repo.write(
        "rust/lib/tests/one.rs",
        "#[test]\nfn one() {\n    assert_eq!(lib::one(), 1);\n}\n",
    );
    repo
}

fn lib_build_script_runs(repo: &Repo) -> String {
    fs::read_to_string(repo.root().join("rust/target/lib-runs.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn rebuilds_the_untrusted_lib(cargo: &[&str]) {
    rebuilds_the_untrusted_lib_in(&project_with_lib_targets(), cargo);
}

fn rebuilds_the_untrusted_lib_in(repo: &Repo, cargo: &[&str]) {
    let plain = build(repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    let lib = package_output(repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let before = lib_build_script_runs(repo);
    let wrapped = wrapped_build(repo, cargo);
    assert!(wrapped.status.success(), "{cargo:?}: {wrapped:?}");
    assert_ne!(
        lib_build_script_runs(repo),
        before,
        "{cargo:?}: {wrapped:?}"
    );
}

#[test]
fn a_generated_binary_build_selects_its_package() {
    rebuilds_the_untrusted_lib(&["cargo", "build", "-p", "lib", "--bin", "tool", "--offline"]);
}

#[test]
fn a_generated_test_target_run_selects_its_package() {
    rebuilds_the_untrusted_lib(&["cargo", "test", "-p", "lib", "--test", "one", "--offline"]);
}

#[test]
fn a_generated_doctest_run_selects_the_workspace() {
    rebuilds_the_untrusted_lib(&["cargo", "test", "--doc", "--workspace", "--offline"]);
}

#[test]
fn a_run_of_a_binary_selects_its_package() {
    rebuilds_the_untrusted_lib(&["cargo", "run", "-p", "lib", "--bin", "tool", "--offline"]);
}

#[test]
fn a_bench_build_selects_its_package() {
    rebuilds_the_untrusted_lib(&[
        "cargo",
        "bench",
        "-p",
        "lib",
        "--profile",
        "dev",
        "--offline",
    ]);
}

#[test]
fn a_rustc_compile_selects_its_package() {
    rebuilds_the_untrusted_lib(&["cargo", "rustc", "-p", "lib", "--lib", "--offline"]);
}

#[test]
fn a_rustdoc_build_selects_its_package() {
    rebuilds_the_untrusted_lib(&["cargo", "rustdoc", "-p", "lib", "--lib", "--offline"]);
}

#[test]
fn a_fix_pass_is_refused() {
    let repo = project_with_lib_targets();
    let refused = wrapped_build(
        &repo,
        &[
            "cargo",
            "fix",
            "-p",
            "lib",
            "--lib",
            "--allow-no-vcs",
            "--offline",
        ],
    );
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("does not run cargo fix"), "{stderr}");
}

#[test]
fn a_run_that_names_no_target_is_refused() {
    let repo = project_with_lib_targets();
    let refused = wrapped_build(&repo, &["cargo", "run", "-p", "lib", "--offline"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("name it with --bin or --example"),
        "{stderr}"
    );
}

#[test]
fn a_cargo_alias_is_judged_by_what_it_expands_to() {
    let repo = project_with_lib_targets();
    repo.write(
        "rust/.cargo/config.toml",
        "[alias]\nxt = \"run -p lib --bin tool\"\n",
    );
    rebuilds_the_untrusted_lib_in(&repo, &["cargo", "xt", "--offline"]);
}

#[test]
fn a_cargo_alias_defined_on_the_command_line_is_refused() {
    let repo = project_with_lib_targets();
    let refused = wrapped_build(
        &repo,
        &[
            "cargo",
            "--config",
            "alias.xt='run -p lib --bin tool'",
            "xt",
            "--offline",
        ],
    );
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("cannot resolve the cargo alias xt"),
        "{stderr}"
    );
}

#[test]
fn a_cargo_alias_that_runs_a_shell_command_is_refused() {
    let repo = project_with_lib_targets();
    repo.write("rust/.cargo/config.toml", "[alias]\nxt = \"!echo hi\"\n");
    let refused = wrapped_build(&repo, &["cargo", "xt"]);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("cargo alias xt runs a shell command"),
        "{stderr}"
    );
}
