// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::super::{audit, own_units, unit, workspace_below_the_root};

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
