// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::*;

fn base(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-anc-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    base
}

fn make(path: &Path, file: &str) {
    fs::create_dir_all(path).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(path.join(file), b"").unwrap_or_else(|e| unreachable!("{e}"));
}

fn swap_for_link(ancestor: &Path, evil: &Path) {
    let gone = ancestor.with_extension("gone");
    fs::rename(ancestor, gone).unwrap_or_else(|e| unreachable!("{e}"));
    symlink(evil, ancestor).unwrap_or_else(|e| unreachable!("{e}"));
}

fn scan_swapping(root: &Path, opening: &Path, ancestor: &Path, evil: &Path) -> io::Result<Entries> {
    let mut budget = Budget::new(usize::MAX);
    scan_with(
        root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory == opening {
                    swap_for_link(ancestor, evil);
                }
                Ok(())
            },
            ..Probe::default()
        },
    )
}

fn assert_secret_unread(result: &io::Result<Entries>) {
    assert!(
        result.is_err(),
        "an ancestor swapped for a link was followed: {:?}",
        result
            .as_ref()
            .map(|entries| entries.keys().collect::<Vec<_>>())
    );
}

#[test]
fn a_root_whose_ancestor_is_swapped_for_a_link_before_it_is_opened_fails_closed() {
    let base = base("root");
    let root = base.join("outer").join("root");
    make(&root, "kept.txt");
    make(&base.join("evil").join("root"), "secret.txt");
    let result = scan_swapping(&root, &root, &base.join("outer"), &base.join("evil"));
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert_secret_unread(&result);
}

#[test]
fn a_link_target_whose_ancestor_is_swapped_for_a_link_before_it_is_opened_fails_closed() {
    let base = base("target");
    let root = base.join("root");
    let target = base.join("ext").join("target");
    make(&root, "kept.txt");
    make(&target, "inner.txt");
    make(&base.join("evil").join("target"), "secret.txt");
    symlink("../ext/target", root.join("link")).unwrap_or_else(|e| unreachable!("{e}"));
    let result = scan_swapping(&root, &target, &base.join("ext"), &base.join("evil"));
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert_secret_unread(&result);
}

#[test]
fn a_link_target_reached_through_unswapped_ancestors_is_walked_as_before() {
    let base = base("stable");
    let root = base.join("root");
    let target = base.join("ext").join("target");
    make(&root, "kept.txt");
    make(&target, "inner.txt");
    symlink("../ext/target", root.join("link")).unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let entries = scan_with(&root, &BTreeSet::new(), &mut budget, &Probe::default());
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    let entries = entries.unwrap_or_else(|e| panic!("{e}"));
    assert!(entries.contains_key(&target), "{:?}", entries.keys());
}
