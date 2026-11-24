// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use super::groups::grouped_tools;
use super::separate_build::unit_count;
use super::{placed, unit, velnor_ci};
use crate::io::Repo;

const TARGET: &str = "target-velnor/tools";
const BUILD: &str = "target-velnor/tools-build";

fn build_units(repo: &Repo, package: &str, normal: &str, harness: &str) {
    for (hash, fingerprint) in [
        (normal, format!("lib-{package}")),
        (harness, format!("test-lib-{package}")),
    ] {
        let base = format!("{BUILD}/debug/build/{package}/{hash}");
        unit(
            repo,
            &format!("{base}/out/{package}-{hash}.d"),
            "out.d: src/lib.rs data.txt\n",
        );
        unit(repo, &format!("{base}/fingerprint/{fingerprint}"), "");
    }
}

fn shared_build_directory() -> Repo {
    let repo = grouped_tools();
    placed(&repo, &format!("{BUILD}/debug"));
    build_units(&repo, "a", "f000000000000001", "f000000000000002");
    build_units(&repo, "b", "f000000000000003", "f000000000000004");
    repo
}

fn audit_into(repo: &Repo, build: &str) -> Output {
    velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", repo.root().join(TARGET))
        .env("CARGO_BUILD_BUILD_DIR", repo.root().join(build))
        .args(["inputs", "audit", "--write"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn a_build_directory_shared_by_a_group_counts_each_unit_once() {
    let repo = shared_build_directory();
    let written = audit_into(&repo, BUILD);
    assert!(written.status.success(), "{written:?}");
    assert_eq!(unit_count(&written), 4, "{written:?}");
}
