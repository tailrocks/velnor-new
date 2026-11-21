// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output};

use nix::sys::resource::{Resource, getrlimit};

use super::inventory::build;
use super::{own_units, unit, workspace_below_the_root};
use crate::io::Repo;

const DEPTH: usize = 300;
const SIBLINGS: usize = 20;
const OUTPUT: &str = "rust/target/debug/build/lib-f000000000000008/output";
const SOFT_LIMIT_WITHOUT_RAISE: u32 = 256;
const CLEAR_LIMIT: u32 = 128;
const RAISE_NEEDS_HARD_LIMIT: u64 = 1024;

fn hard_open_file_limit() -> u64 {
    getrlimit(Resource::RLIMIT_NOFILE).map_or(0, |(_, hard)| hard)
}

fn deep_repository() -> Repo {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    let mut directory = repo.root().join("deep");
    for _ in 0..DEPTH {
        for sibling in 0..SIBLINGS {
            fs::create_dir_all(directory.join(format!("a{sibling}")))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        directory = directory.join("z");
    }
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    build(&repo, OUTPUT, None);
    repo
}

fn velnor_ci_under(repo: &Repo, limits: &str, args: &[&str], path: Option<&str>) -> Output {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("{limits} && exec \"$0\" \"$@\""))
        .arg(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(args)
        .current_dir(repo.root())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("VELNOR_AUDIT_MAX_ENTRIES");
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn audit_under(repo: &Repo, limits: &str) -> Output {
    velnor_ci_under(repo, limits, &["inputs", "audit", "--write"], None)
}

fn fake_cargo() -> (Repo, String) {
    let bin = Repo::empty("cargo-bin");
    let cargo = bin.root().join("cargo");
    let real = std::env::var("PATH")
        .unwrap_or_default()
        .split(':')
        .map(|dir| PathBuf::from(dir).join("cargo"))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| unreachable!("no cargo on PATH"));
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\nif [ \"$1\" = metadata ]; then exec \"{}\" \"$@\"; fi\nulimit -Sn\ntouch -m -t 206601010000 {OUTPUT}\n",
            real.display()
        ),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        bin.root().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (bin, path)
}

fn receipt(repo: &Repo) -> PathBuf {
    repo.root().join(format!("{OUTPUT}.velnor-inputs.json"))
}

#[test]
fn a_deep_tree_with_pending_siblings_audits_once_the_open_file_limit_is_raised() {
    let hard = hard_open_file_limit();
    assert!(
        hard >= RAISE_NEEDS_HARD_LIMIT,
        "the hard open-file limit {hard} is below {RAISE_NEEDS_HARD_LIMIT}, so the soft limit cannot be raised past {SOFT_LIMIT_WITHOUT_RAISE} and the raise path would not run"
    );
    let repo = deep_repository();
    let audited = audit_under(&repo, &format!("ulimit -Sn {SOFT_LIMIT_WITHOUT_RAISE}"));
    assert!(audited.status.success(), "{audited:?}");
    assert!(repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn exhausting_the_open_file_limit_names_the_depth_and_the_limit() {
    let repo = deep_repository();
    let audited = audit_under(
        &repo,
        &format!("ulimit -Sn {CLEAR_LIMIT} && ulimit -Hn {CLEAR_LIMIT}"),
    );
    assert_eq!(audited.status.code(), Some(1), "{audited:?}");
    let stderr = String::from_utf8_lossy(&audited.stderr);
    assert!(
        stderr.contains("directory tree deeper than ")
            && stderr.contains(&format!(
                " levels with pending siblings exceeds the open-file limit ({CLEAR_LIMIT}); raise the open-file limit"
            )),
        "{stderr}"
    );
}

#[test]
fn a_build_that_exhausts_the_open_file_limit_runs_cargo_warns_and_treats_the_tree_as_untrusted() {
    let repo = deep_repository();
    assert!(
        receipt(&repo).exists(),
        "a complete build records its inputs"
    );
    let (_bin, path) = fake_cargo();
    let built = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {CLEAR_LIMIT} && ulimit -Hn {CLEAR_LIMIT}"),
        &["inputs", "build", "--", "cargo", "build"],
        Some(&path),
    );
    assert!(built.status.success(), "{built:?}");
    let stderr = String::from_utf8_lossy(&built.stderr);
    assert!(
        stderr.contains("velnor-ci: WARNING: directory tree deeper than ")
            && stderr.contains(&format!(
                " levels with pending siblings exceeds the open-file limit ({CLEAR_LIMIT}); raise the open-file limit"
            )),
        "{stderr}"
    );
    assert!(
        String::from_utf8_lossy(&built.stdout)
            .lines()
            .any(|line| line == CLEAR_LIMIT.to_string()),
        "cargo did not run under the original limit: {built:?}"
    );
    assert!(
        !receipt(&repo).exists(),
        "an incomplete capture kept the previous inputs record"
    );
}

#[test]
fn the_build_command_sees_the_open_file_limit_the_process_started_with() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(&repo, OUTPUT, "cargo:rerun-if-changed=../..\n");
    let built = velnor_ci_under(
        &repo,
        &format!("ulimit -Sn {SOFT_LIMIT_WITHOUT_RAISE}"),
        &["inputs", "build", "--", "sh", "-c", "ulimit -Sn"],
        None,
    );
    assert!(built.status.success(), "{built:?}");
    assert!(
        String::from_utf8_lossy(&built.stdout)
            .lines()
            .any(|line| line == SOFT_LIMIT_WITHOUT_RAISE.to_string()),
        "the build command inherited the raised limit: {built:?}"
    );
}
