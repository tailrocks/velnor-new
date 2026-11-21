// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Output;

use super::inventory::build;
use super::{own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";

fn built_repository() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    build(&repo, OUTPUT, None);
    repo
}

fn receipt_bytes(repo: &Repo) -> Vec<u8> {
    fs::read(repo.root().join(format!("{OUTPUT}.velnor-inputs.json")))
        .unwrap_or_else(|e| unreachable!("{e}"))
}

fn refused(repo: &Repo, program: &str) -> Output {
    let refused = super::velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--", program])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    refused
}

fn assert_nothing_was_recorded(repo: &Repo, receipt: &[u8]) {
    assert_eq!(receipt_bytes(repo), receipt);
    assert!(repo.root().join(OUTPUT).exists());
}

#[test]
fn a_build_program_that_does_not_exist_is_a_spawn_error_and_records_nothing() {
    let repo = built_repository();
    let receipt = receipt_bytes(&repo);
    let stderr =
        String::from_utf8_lossy(&refused(&repo, "definitely-not-a-program").stderr).into_owned();
    assert!(
        stderr.contains("cannot run build command: No such file or directory"),
        "{stderr}"
    );
    assert_nothing_was_recorded(&repo, &receipt);
}

#[test]
fn a_build_program_that_is_not_executable_is_a_spawn_error_and_records_nothing() {
    let repo = built_repository();
    let receipt = receipt_bytes(&repo);
    unit(&repo, "tool", "#!/bin/sh\n");
    let stderr = String::from_utf8_lossy(&refused(&repo, "./tool").stderr).into_owned();
    assert!(
        stderr.contains("cannot run build command: Permission denied"),
        "{stderr}"
    );
    assert_nothing_was_recorded(&repo, &receipt);
}

#[test]
fn an_empty_build_program_is_a_spawn_error_and_records_nothing() {
    let repo = built_repository();
    let receipt = receipt_bytes(&repo);
    let stderr = String::from_utf8_lossy(&refused(&repo, "").stderr).into_owned();
    assert!(stderr.contains("cannot run build command: "), "{stderr}");
    assert_nothing_was_recorded(&repo, &receipt);
}

#[test]
fn a_build_program_whose_name_starts_with_a_dash_runs_as_a_program() {
    let repo = built_repository();
    unit(&repo, "-dash", "#!/bin/sh\nexit 0\n");
    fs::set_permissions(repo.root().join("-dash"), fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let ran = super::velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "./-dash"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(ran.status.success(), "{ran:?}");
}

#[test]
fn a_bare_build_program_whose_name_starts_with_a_dash_is_run_by_its_absolute_path() {
    let repo = built_repository();
    let bin = repo.root().join("bin");
    let ran = repo.root().join("ran.txt");
    unit(
        &repo,
        "bin/-prog",
        &format!("#!/bin/sh\nprintf '%s' \"$0\" > '{}'\n", ran.display()),
    );
    fs::set_permissions(bin.join("-prog"), fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let inherited = std::env::var_os("PATH").unwrap_or_default();
    let path =
        std::env::join_paths(std::iter::once(bin.clone()).chain(std::env::split_paths(&inherited)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    let output = super::velnor_ci()
        .current_dir(repo.root())
        .env("PATH", path)
        .args(["inputs", "build", "--", "-prog"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(&ran).unwrap_or_else(|e| unreachable!("{e}")),
        bin.join("-prog").display().to_string()
    );
}
