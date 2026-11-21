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
    let refused = build_within(&repo, "150");
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let error = String::from_utf8_lossy(&refused.stderr);
    assert!(error.contains(VARIABLE), "{refused:?}");
}

#[test]
fn outputs_sharing_one_declared_directory_pass_within_the_entry_limit() {
    let repo = outputs_declaring_one_directory(200);
    let within = build_within(&repo, "900");
    assert!(within.status.success(), "{within:?}");
}

#[test]
fn one_output_declaring_one_directory_under_two_spellings_passes_within_the_entry_limit() {
    let repo = declared_tree(200);
    let output = "rust/target/debug/build/lib-f00000000000000f/output";
    unit(
        &repo,
        output,
        "cargo:rerun-if-changed=../../shared/first\ncargo:rerun-if-changed=../../shared/../shared/first\n",
    );
    repo.set_mtime(output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    inventory::build(&repo, output, None);
    let within = build_within(&repo, "600");
    assert!(within.status.success(), "{within:?}");
}
