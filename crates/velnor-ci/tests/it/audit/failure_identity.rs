// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::migration::{APP_SELECTION, package_output};
use super::selection::{
    NAMED, Run, prepared_project, refusal_after_a_failed_lib_build, wrapped_in,
};

#[test]
fn a_marker_from_a_run_is_ignored_when_the_current_invocation_has_no_run_identity() {
    let stderr = refusal_after_a_failed_lib_build(Some(("1001", "1")), None);
    assert!(!stderr.contains(NAMED), "{stderr}");
    assert!(stderr.contains("explicit selection: lib"), "{stderr}");
}

#[test]
fn a_marker_without_a_run_identity_is_named_by_an_invocation_without_one() {
    let stderr = refusal_after_a_failed_lib_build(None, None);
    assert!(stderr.contains(NAMED), "{stderr}");
}

fn refusal_after(failing: &[&str], failed_in: Run) -> String {
    let repo = prepared_project();
    let failed = wrapped_in(&repo, failing, failed_in);
    assert!(!failed.status.success(), "{failed:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = wrapped_in(&repo, &APP_SELECTION, failed_in);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    String::from_utf8_lossy(&refused.stderr).into_owned()
}

#[test]
fn a_workspace_build_minus_another_package_is_named_for_a_package_it_compiled() {
    let failing = [
        "cargo",
        "build",
        "--workspace",
        "--exclude",
        "app",
        "--all-targets",
        "--offline",
        "--no-such-flag",
    ];
    assert!(refusal_after(&failing, None).contains(NAMED));
}

#[test]
fn a_workspace_build_that_excludes_the_refused_package_is_not_named() {
    let failing = [
        "cargo",
        "build",
        "--workspace",
        "--exclude",
        "lib",
        "--all-targets",
        "--offline",
        "--no-such-flag",
    ];
    let stderr = refusal_after(&failing, None);
    assert!(!stderr.contains(NAMED), "{stderr}");
    assert!(stderr.contains("explicit selection: lib"), "{stderr}");
}
