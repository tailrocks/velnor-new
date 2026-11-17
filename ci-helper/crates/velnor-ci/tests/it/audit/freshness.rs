// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, FileTimes};
use std::time::{Duration, UNIX_EPOCH};

use super::{audit, own_units, unit, workspace_below_the_root};

#[test]
fn a_deleted_tracked_dep_info_input_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    unit(
        &repo,
        &format!("{target}/deps/app-f000000000000001.d"),
        "out.d: app/src/lib.rs ../shared/greeting.txt\n",
    );
    fs::remove_file(repo.root().join("shared/greeting.txt"))
        .unwrap_or_else(|e| unreachable!("{e}"));

    let deleted = audit(&repo, &["--write"]);
    assert_eq!(deleted.status.code(), Some(1), "{deleted:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}

#[test]
fn a_changed_file_inside_a_declared_directory_prevents_writing_the_inputs_lock() {
    let repo = workspace_below_the_root();
    repo.write("shared/assets/nested/logo.txt", "original");
    let target = "rust/target/debug";
    own_units(&repo, target, &[]);
    let output = format!("{target}/build/lib-f000000000000008/output");
    unit(
        &repo,
        &output,
        "cargo:rerun-if-changed=../../shared/assets\n",
    );
    let old = UNIX_EPOCH + Duration::from_secs(2_000_000_000);
    repo.set_mtime(&output, UNIX_EPOCH + Duration::from_secs(3_000_000_001));
    for directory in ["shared/assets", "shared/assets/nested"] {
        File::open(repo.root().join(directory))
            .and_then(|file| file.set_times(FileTimes::new().set_modified(old)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    repo.write_unstaged("shared/assets/nested/logo.txt", "changed");
    repo.set_mtime(
        "shared/assets/nested/logo.txt",
        UNIX_EPOCH + Duration::from_secs(4_000_000_000),
    );
    assert_eq!(repo.mtime("shared/assets"), old);
    assert_eq!(repo.mtime("shared/assets/nested"), old);

    let changed = audit(&repo, &["--write"]);
    assert_eq!(changed.status.code(), Some(1), "{changed:?}");
    assert!(!repo.root().join(".velnor/rust-inputs.lock").exists());
}
