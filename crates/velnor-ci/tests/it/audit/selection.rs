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
            "An earlier inputs build failed: `cargo build -p app --all-targets --offline --no-such-flag` (failed commands record no receipts); fix that step first"
        ),
        "{refused:?}"
    );
}
