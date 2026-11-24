// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

fn arena(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-root-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

fn scanned(root: &Path) -> io::Result<Entries> {
    scan_with(
        root,
        &BTreeSet::new(),
        &mut Budget::new(usize::MAX),
        &Probe::default(),
    )
}

#[test]
fn a_walk_root_that_is_a_regular_file_is_rejected() {
    let base = arena("regular-file");
    let root = base.join("file");
    fs::write(&root, b"").unwrap_or_else(|e| unreachable!("{e}"));
    let result = scanned(&root);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(result.is_err(), "a file root was scanned: {result:?}");
}

#[test]
fn a_walk_root_that_is_a_looping_symlink_is_rejected() {
    let base = arena("looping-link");
    let root = base.join("loop");
    std::os::unix::fs::symlink("loop", &root).unwrap_or_else(|e| unreachable!("{e}"));
    let result = scanned(&root);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        result.is_err(),
        "a looping link root was scanned: {result:?}"
    );
}

#[test]
fn a_walk_root_that_is_a_symlink_to_a_file_is_rejected() {
    let base = arena("link-to-file");
    let target = base.join("file");
    let root = base.join("link");
    fs::write(&target, b"").unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&target, &root).unwrap_or_else(|e| unreachable!("{e}"));
    let result = scanned(&root);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        result.is_err(),
        "a link-to-file root was scanned: {result:?}"
    );
}
