// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use super::migration::{APP_SELECTION, app_and_lib_project, build, package_output, wrapped_build};

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
fn an_earlier_failed_inputs_build_is_named_in_the_selection_refusal() {
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
