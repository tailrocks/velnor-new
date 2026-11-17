// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::{audit, own_units, unit, workspace_below_the_root};
use super::aggregate::write_dep_info_files;

#[test]
fn large_build_output_keeps_the_trailing_rerun_directive() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    let output_path = "rust/target/debug/build/app-f000000000000004/output";
    let mut output = "cargo:warning=".to_owned();
    output.push_str(&"x".repeat(1 << 20));
    output.push_str("\ncargo:rerun-if-changed=../../shared/greeting.txt\n");
    unit(&repo, output_path, &output);

    let result = audit(&repo, &["--write"]);

    assert!(result.status.success(), "{result:?}");
    assert_eq!(
        fs::read_to_string(repo.root().join(".velnor/rust-inputs.lock"))
            .unwrap_or_else(|e| unreachable!("{e}")),
        "shared/greeting.txt\tapp\n"
    );
}

#[test]
fn build_output_counts_toward_the_shared_target_data_budget() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(
        &repo,
        "rust/target/debug/deps/app-f000000000000001.d",
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    let baseline = audit(&repo, &["--write"]);
    assert!(baseline.status.success(), "{baseline:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let before = fs::read(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(before, b"shared/greeting.txt\tapp\n");

    let mut index = 20;
    write_dep_info_files(&repo, "rust/target/debug", "app/src/lib.rs", 7, &mut index);
    let output_path = "rust/target/debug/build/app-f000000000000004/output";
    let mut output = "cargo:warning=".to_owned();
    output.push_str(&"x".repeat(2 << 20));
    unit(&repo, output_path, &output);

    let over_limit = audit(&repo, &["--write"]);
    assert_eq!(over_limit.status.code(), Some(1), "{over_limit:?}");
    let error = String::from_utf8_lossy(&over_limit.stderr);
    assert!(
        error.contains("aggregate target data exceeds 8388608 bytes"),
        "{error}"
    );
    assert!(error.contains(output_path), "{error}");
    assert_eq!(
        fs::read(lock).unwrap_or_else(|e| unreachable!("{e}")),
        before
    );
}
