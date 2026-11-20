// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;

use super::command;
use super::migration::{APP_SELECTION, app_and_lib_project, build, package_output, wrapped_build};
use crate::io::Repo;

#[test]
fn the_selection_refusal_names_the_untrusted_packages_and_their_kind() {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let refused = wrapped_build(&repo, &APP_SELECTION);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(
            "explicit selection: lib (build-script output). This usually means an earlier `velnor-ci inputs build` step in this job failed (failed commands record no receipts); fix that step first."
        ),
        "{refused:?}"
    );
}

#[test]
fn an_earlier_failed_build_that_selected_other_packages_is_not_blamed() {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    let mut failing = APP_SELECTION.to_vec();
    failing.push("--no-such-flag");
    let failed = wrapped_build(&repo, &failing);
    assert!(!failed.status.success(), "{failed:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = wrapped_build(&repo, &APP_SELECTION);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    let stderr = String::from_utf8_lossy(&refused.stderr);
    assert!(
        stderr.contains("explicit selection: lib (build-script output). This usually means"),
        "{refused:?}"
    );
    assert!(
        !stderr.contains("An earlier inputs build failed"),
        "{refused:?}"
    );
}

#[test]
fn an_earlier_failed_inputs_build_covering_the_refused_package_is_named() {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    let failing = [
        "cargo",
        "build",
        "-p",
        "lib",
        "--all-targets",
        "--offline",
        "--no-such-flag",
    ];
    let failed = wrapped_build(&repo, &failing);
    assert!(!failed.status.success(), "{failed:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = wrapped_build(&repo, &APP_SELECTION);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains(
            "An earlier inputs build failed: `cargo build` (failed commands record no receipts); fix that step first"
        ),
        "{refused:?}"
    );
}

#[test]
fn a_failed_build_leaves_no_command_arguments_on_disk() {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    let mut failing = APP_SELECTION.to_vec();
    failing.push("--token=hunter2-credential");
    let failed = wrapped_build(&repo, &failing);
    assert!(!failed.status.success(), "{failed:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = wrapped_build(&repo, &APP_SELECTION);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    assert!(
        !String::from_utf8_lossy(&refused.stderr).contains("hunter2-credential"),
        "{refused:?}"
    );
    assert!(
        !files_below(repo.root()).any(|file| fs::read(file).is_ok_and(|bytes| bytes
            .windows(b"hunter2-credential".len())
            .any(|window| window == b"hunter2-credential"))),
        "a command argument reached the target directory"
    );
}

fn files_below(root: &std::path::Path) -> impl Iterator<Item = std::path::PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.into_iter()
}

#[test]
fn a_failed_build_marks_only_the_active_roots_target_directory() {
    let repo = app_and_lib_project();
    repo.write(
        "other/Cargo.toml",
        "[package]\nname = \"other\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    repo.write(
        "other/Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"other\"\nversion = \"0.1.0\"\n",
    );
    repo.write("other/src/lib.rs", "");
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    let mut failing = APP_SELECTION.to_vec();
    failing.push("--no-such-flag");
    let failed = wrapped_build(&repo, &failing);
    assert!(!failed.status.success(), "{failed:?}");
    let marker = ".velnor-input-inventories/failed-inputs-build.json";
    assert!(repo.root().join("rust/target").join(marker).exists());
    assert!(
        !repo.root().join("other/target").join(marker).exists(),
        "a failure in one root marked another root's target directory"
    );
}

#[test]
fn a_build_that_cannot_start_leaves_a_failure_marker() {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let spawned = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", "velnor-no-such-program"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!spawned.status.success(), "{spawned:?}");
    assert!(
        repo.root()
            .join("rust/target/.velnor-input-inventories/failed-inputs-build.json")
            .exists(),
        "{spawned:?}"
    );
}

type Run = Option<(&'static str, &'static str)>;

fn wrapped_in(repo: &Repo, cargo: &[&str], run: Run) -> Output {
    let mut wrapped = command(env!("CARGO_BIN_EXE_velnor-ci"));
    wrapped
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--"])
        .args(cargo)
        .env_remove("GITHUB_RUN_ID")
        .env_remove("GITHUB_RUN_ATTEMPT");
    if let Some((id, attempt)) = run {
        wrapped
            .env("GITHUB_RUN_ID", id)
            .env("GITHUB_RUN_ATTEMPT", attempt);
    }
    wrapped.output().unwrap_or_else(|e| unreachable!("{e}"))
}

fn prepared_project() -> Repo {
    let repo = app_and_lib_project();
    let plain = build(&repo, false);
    assert!(plain.status.success(), "{plain:?}");
    let prepared = build(&repo, true);
    assert!(prepared.status.success(), "{prepared:?}");
    repo
}

fn refusal_after_a_failed_lib_build(failed_in: Run, refused_in: Run) -> String {
    let repo = prepared_project();
    let failing = [
        "cargo",
        "build",
        "-p",
        "lib",
        "--all-targets",
        "--offline",
        "--no-such-flag",
    ];
    let failed = wrapped_in(&repo, &failing, failed_in);
    assert!(!failed.status.success(), "{failed:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = wrapped_in(&repo, &APP_SELECTION, refused_in);
    assert_eq!(refused.status.code(), Some(1), "{refused:?}");
    String::from_utf8_lossy(&refused.stderr).into_owned()
}

const NAMED: &str = "An earlier inputs build failed: `cargo build`";

#[test]
fn a_failed_non_cargo_command_records_no_package_selection() {
    let repo = prepared_project();
    let secrets = [
        "hunter2-credential",
        "hunter3-credential",
        "hunter4-credential",
    ];
    let failed = command(env!("CARGO_BIN_EXE_velnor-ci"))
        .current_dir(repo.root().join("rust"))
        .args(["inputs", "build", "--", "sh", "-c", "exit 1", "-p"])
        .args([
            secrets[0],
            "-phunter3-credential",
            "--package=hunter4-credential",
        ])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!failed.status.success(), "{failed:?}");
    let marker = repo
        .root()
        .join("rust/target/.velnor-input-inventories/failed-inputs-build.json");
    assert!(marker.exists(), "{failed:?}");
    for file in files_below(repo.root()) {
        let bytes = fs::read(file).unwrap_or_default();
        for secret in secrets {
            assert!(
                !bytes
                    .windows(secret.len())
                    .any(|window| window == secret.as_bytes()),
                "{secret} reached the target directory"
            );
        }
    }
}

#[test]
fn a_selection_refusal_does_not_overwrite_the_failed_build_it_reports() {
    let repo = prepared_project();
    let failing = ["cargo", "build", "-p", "lib", "--no-such-flag", "--offline"];
    let failed = wrapped_in(&repo, &failing, None);
    assert!(!failed.status.success(), "{failed:?}");
    let lib = package_output(&repo, "lib");
    fs::remove_file(lib.with_extension("velnor-inputs.json"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    for _ in 0..2 {
        let refused = wrapped_in(&repo, &APP_SELECTION, None);
        assert_eq!(refused.status.code(), Some(1), "{refused:?}");
        assert!(
            String::from_utf8_lossy(&refused.stderr).contains(NAMED),
            "{refused:?}"
        );
    }
}

#[test]
fn a_marker_that_cannot_be_cleared_does_not_fail_a_successful_build() {
    let repo = prepared_project();
    let marker = repo
        .root()
        .join("rust/target/.velnor-input-inventories/failed-inputs-build.json");
    fs::create_dir_all(&marker).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(marker.join("keep"), "x").unwrap_or_else(|e| unreachable!("{e}"));
    let rebuilt = wrapped_in(&repo, &APP_SELECTION, None);
    assert!(rebuilt.status.success(), "{rebuilt:?}");
    assert!(
        String::from_utf8_lossy(&rebuilt.stderr).contains("cannot clear"),
        "{rebuilt:?}"
    );
}

#[test]
fn a_marker_from_the_same_run_is_named() {
    let run = Some(("1001", "1"));
    assert!(refusal_after_a_failed_lib_build(run, run).contains(NAMED));
}

#[test]
fn a_marker_from_another_run_or_attempt_is_ignored() {
    for refused_in in [Some(("1002", "1")), Some(("1001", "2"))] {
        let stderr = refusal_after_a_failed_lib_build(Some(("1001", "1")), refused_in);
        assert!(!stderr.contains(NAMED), "{stderr}");
        assert!(stderr.contains("explicit selection: lib"), "{stderr}");
    }
}

#[test]
fn a_marker_without_a_run_identity_is_ignored_when_the_run_has_one() {
    let stderr = refusal_after_a_failed_lib_build(None, Some(("1001", "1")));
    assert!(!stderr.contains(NAMED), "{stderr}");
}

#[test]
fn a_failed_build_records_only_package_names_that_are_workspace_members() {
    let repo = prepared_project();
    let failing = [
        "cargo",
        "build",
        "-p",
        "lib",
        "-p",
        "git+https://example.invalid/app#app@0.1.0",
        "-p",
        "no-such-member",
        "--all-targets",
        "--offline",
        "--no-such-flag",
    ];
    let failed = wrapped_in(&repo, &failing, None);
    assert!(!failed.status.success(), "{failed:?}");
    let marker = repo
        .root()
        .join("rust/target/.velnor-input-inventories/failed-inputs-build.json");
    let recorded = fs::read_to_string(&marker).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(recorded.contains(r#""packages":["lib"]"#), "{recorded}");
}
