// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use super::super::{audit, own_units, unit, workspace_below_the_root};

const MAX_TARGET_DIRECTORIES: usize = 4096;

#[test]
fn target_directory_count_is_bounded_without_removing_the_existing_lock() {
    let repo = workspace_below_the_root();
    own_units(&repo, "rust/target/debug", &[]);
    unit(
        &repo,
        "rust/target/debug/deps/app-f000000000000001.d",
        "out.d: app/src/lib.rs app/src/../../../shared/greeting.txt\n",
    );
    let baseline = audit(&repo, &["--write"]);
    assert!(baseline.status.success(), "{baseline:?}");
    let lock = repo.root().join(".velnor/rust-inputs.lock");
    let before = fs::read(&lock).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(before, b"shared/greeting.txt\tapp\n");

    let target = repo.root().join("rust/target");
    let wide = target.join("breadth");
    fs::create_dir_all(&wide).unwrap_or_else(|e| unreachable!("{e}"));
    let existing = directory_count(&target);
    assert!(existing < MAX_TARGET_DIRECTORIES, "{existing}");
    for index in 0..(MAX_TARGET_DIRECTORIES - existing) {
        fs::create_dir(wide.join(index.to_string())).unwrap_or_else(|e| unreachable!("{e}"));
    }
    let within_limit = audit(&repo, &["--write"]);
    assert!(within_limit.status.success(), "{within_limit:?}");

    fs::create_dir(wide.join("over-limit")).unwrap_or_else(|e| unreachable!("{e}"));
    let beyond_limit = audit(&repo, &["--write"]);
    assert_eq!(beyond_limit.status.code(), Some(1), "{beyond_limit:?}");
    let error = String::from_utf8_lossy(&beyond_limit.stderr);
    assert!(error.contains("target directory traversal exceeds 4096 directories"), "{error}");
    assert_eq!(fs::read(lock).unwrap_or_else(|e| unreachable!("{e}")), before);
}

fn directory_count(root: &Path) -> usize {
    1 + fs::read_dir(root)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")))
        .filter(|entry| {
            entry
                .file_type()
                .unwrap_or_else(|e| unreachable!("{e}"))
                .is_dir()
        })
        .map(|entry| directory_count(&entry.path()))
        .sum::<usize>()
}
