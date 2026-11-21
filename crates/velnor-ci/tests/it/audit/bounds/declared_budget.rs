// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;
use std::time::{Duration, UNIX_EPOCH};

use super::super::{inventory, unit, velnor_ci};
use super::entries::{VARIABLE, declared_tree};
use crate::io::Repo;

const OUTPUTS: usize = 6;

fn outputs_declaring_one_directory(entries: usize) -> Repo {
    let repo = declared_tree(entries);
    for index in 1..OUTPUTS {
        let output = format!("rust/target/debug/build/lib-f00000000000000{index:x}/output");
        unit(
            &repo,
            &output,
            "cargo:rerun-if-changed=../../shared/first\n",
        );
        repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
        inventory::build(&repo, &output, None);
    }
    repo
}

fn build_within(repo: &Repo, limit: &str) -> Output {
    velnor_ci()
        .env(VARIABLE, limit)
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn declaration_trust_scans_over_the_entry_limit_fail_and_name_the_variable() {
    let repo = outputs_declaring_one_directory(200);
    let refused = build_within(&repo, "900");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(error.contains(VARIABLE), "{refused:?}");
}

#[test]
fn declaration_trust_scans_within_the_entry_limit_pass() {
    let repo = outputs_declaring_one_directory(200);
    let within = build_within(&repo, "5000");
    assert!(within.status.success(), "{within:?}");
}
