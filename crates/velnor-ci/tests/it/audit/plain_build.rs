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

#[test]
fn a_failed_command_that_leaves_output_and_no_unit_reports_its_own_failure() {
    let repo = workspace_below_the_root();
    let failed = velnor_ci()
        .current_dir(repo.root())
        .args([
            "inputs",
            "build",
            "--",
            "sh",
            "-c",
            "mkdir -p rust/target/debug && : > rust/target/debug/libapp.rlib && exit 3",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    let stderr = String::from_utf8_lossy(&failed.stderr);
    assert!(
        stderr.contains("build command failed") && !stderr.contains("unknown cargo layout?"),
        "{stderr}"
    );
}

#[test]
fn a_plain_target_named_through_a_parent_component_still_refuses_an_unknown_layout() {
    let repo = workspace_below_the_root();
    std::fs::create_dir_all(repo.root().join("scratch")).unwrap_or_else(|e| unreachable!("{e}"));
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    let refused = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", "scratch/../rust/target")
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
