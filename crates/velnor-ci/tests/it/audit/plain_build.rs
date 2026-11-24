// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::new_layout::own_units_1100;
use super::separate_build::two_standalone_roots;
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

#[test]
fn a_plain_target_named_through_a_parent_component_that_the_command_creates_still_refuses() {
    let repo = workspace_below_the_root();
    let refused = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_BUILD_BUILD_DIR", "scratch/../rust/target")
        .args([
            "inputs",
            "build",
            "--",
            "sh",
            "-c",
            "mkdir -p rust/target/debug && : > rust/target/debug/libapp.rlib",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("unknown cargo layout?"), "{stderr}");
}

#[test]
fn a_relative_plain_target_is_audited_from_the_step_directory_as_cargo_writes_it() {
    let repo = workspace_below_the_root();
    unit(&repo, "rust/target/debug/libapp.rlib", "");
    let refused = velnor_ci()
        .current_dir(repo.root().join("rust/app"))
        .env("CARGO_TARGET_DIR", "../target")
        .args(["inputs", "build", "--", "true"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(stderr.contains("unknown cargo layout?"), "{stderr}");
}

#[test]
fn a_relative_target_dir_is_resolved_after_the_command_changes_directory() {
    let repo = workspace_below_the_root();
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", "target")
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "rust",
            "build",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
}

#[test]
fn a_templated_build_directory_is_refused_by_inputs_build_and_audit() {
    let repo = workspace_below_the_root();
    for arguments in [
        &["inputs", "build", "--", "true"][..],
        &["inputs", "audit"][..],
    ] {
        let refused = velnor_ci()
            .current_dir(repo.root())
            .env("CARGO_BUILD_BUILD_DIR", "{workspace-root}/target")
            .args(arguments)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(refused.status.code(), Some(1), "{refused:?}");
        let stderr = String::from_utf8_lossy(&refused.stderr);
        assert!(
            stderr.contains(
                "CARGO_BUILD_BUILD_DIR={workspace-root}/target uses a cargo path template; set a plain path instead"
            ),
            "{stderr}"
        );
    }
}

#[test]
fn a_cargo_configuration_under_the_changed_directory_decides_the_target_the_build_audits() {
    let repo = workspace_below_the_root();
    unit(
        &repo,
        "rust/app/.cargo/config.toml",
        "[build]\ntarget-dir = \"../elsewhere\"\n",
    );
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "rust/app",
            "build",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert!(repo.root().join("rust/elsewhere/debug").is_dir());
}

#[test]
fn a_build_refused_for_an_unknown_layout_in_a_later_target_saves_nothing_for_an_earlier_one() {
    let repo = two_standalone_roots();
    let refused = velnor_ci()
        .current_dir(repo.root())
        .args([
            "inputs",
            "build",
            "--",
            "sh",
            "-c",
            "mkdir -p web/target/debug && : > web/target/debug/libx.rlib",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("unknown cargo layout?"),
        "{refused:?}"
    );
    let saved: Vec<_> = fs::read_dir(repo.root().join("tools/b/target/.velnor-input-inventories"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            Path::new(&name).extension().is_some_and(|e| e == "json")
                && !name.starts_with("failed-")
        })
        .collect();
    assert!(
        saved.is_empty(),
        "an inventory was saved before the later directory was refused: {saved:?}"
    );
}

#[test]
fn rustdoc_is_audited_where_cargo_dash_c_writes_it() {
    let repo = workspace_below_the_root();
    let built = velnor_ci()
        .current_dir(repo.root())
        .env("CARGO_TARGET_DIR", "target")
        .env("RUSTC_BOOTSTRAP", "1")
        .args([
            "inputs",
            "build",
            "--",
            "cargo",
            "-Z",
            "unstable-options",
            "-C",
            "rust",
            "rustdoc",
            "-p",
            "lib",
            "--offline",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(built.status.success(), "{built:?}");
    assert!(
        repo.root()
            .join("rust/target/.velnor-input-inventories")
            .is_dir(),
        "the build did not audit the target cargo wrote"
    );
}
