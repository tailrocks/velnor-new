// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::new_layout::own_units_1100;
use super::{unit, velnor_ci, workspace_below_the_root};

#[test]
fn a_plain_target_that_holds_outputs_and_no_unit_makes_the_build_refuse_up_front() {
    let repo = workspace_below_the_root();
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    let refused = velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("found no units in") && stderr.contains("unknown cargo layout?"),
        "{stderr}"
    );
}

#[test]
fn a_plain_target_with_units_builds_without_refusal() {
    let repo = workspace_below_the_root();
    own_units_1100(&repo, "rust/target/debug", &[]);
    let built = velnor_ci()
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
}
