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
    let repo = project_with_lib_targets();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let before = lib_build_script_runs(&repo);
    let wrapped = wrapped_build(&repo, cargo);
    assert!(wrapped.status.success(), "{cargo:?}: {wrapped:?}");
    assert_ne!(
        lib_build_script_runs(&repo),
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
