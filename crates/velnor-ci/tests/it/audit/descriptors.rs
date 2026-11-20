// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
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

fn audit_under(repo: &Repo, limits: &str) -> Output {
    Command::new("sh")
        .arg("-c")
        .arg(format!("{limits} && exec \"$0\" \"$@\""))
        .arg(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["inputs", "audit", "--write"])
        .current_dir(repo.root())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("CARGO_BUILD_BUILD_DIR")
        .env_remove("VELNOR_AUDIT_MAX_ENTRIES")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_deep_tree_with_pending_siblings_audits_once_the_open_file_limit_is_raised() {
    let hard = hard_open_file_limit();
    if hard < RAISE_NEEDS_HARD_LIMIT {
        println!(
            "skipped: the hard open-file limit {hard} is below {RAISE_NEEDS_HARD_LIMIT}, so the soft limit cannot be raised past {SOFT_LIMIT_WITHOUT_RAISE}"
        );
        return;
    }
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
