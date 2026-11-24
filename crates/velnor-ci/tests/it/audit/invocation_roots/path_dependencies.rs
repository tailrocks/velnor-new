// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::root_package_with_path_dependencies;
use super::{command, unit};

const REFUSAL: &str = "outside this command's explicit selection";

fn stderr_of_build_with_untrusted_output_of(package: &str, selection: &[&str]) -> String {
    let repo = root_package_with_path_dependencies();
    unit(
        &repo,
        &format!("target/debug/build/{package}-f000000000000005/output"),
        "cargo:rerun-if-changed=src/lib.rs\n",
    );
    let output = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root())
        .args(["inputs", "build", "--", "cargo", "build"])
        .args(selection)
        .arg("--no-such-flag")
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_RUN_ATTEMPT")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_workspace_build_selects_the_path_dependencies_outside_the_workspace() {
    for selection in [
        &["--workspace"][..],
        &["--workspace", "--all-targets"],
        &["--all"],
    ] {
        for package in ["mid", "leaf", "side"] {
            let stderr = stderr_of_build_with_untrusted_output_of(package, selection);
            assert!(
                !stderr.contains(REFUSAL) && stderr.contains("build command failed"),
                "{selection:?} {package}: {stderr}"
            );
        }
    }
}

#[test]
fn a_package_build_does_not_select_a_path_dependency_it_does_not_name() {
    let stderr = stderr_of_build_with_untrusted_output_of("mid", &["-p", "other"]);
    assert!(stderr.contains(REFUSAL), "{stderr}");
}
