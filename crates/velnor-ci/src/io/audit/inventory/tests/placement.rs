// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use super::*;

#[test]
fn a_directory_whose_parent_grants_search_but_not_read_is_scanned() {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-search-only-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let parent = base.join("parent");
    let root = parent.join("root");
    fs::create_dir_all(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("kept.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o311))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let entries = scan(&root, &BTreeSet::new(), &mut budget);
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    let entries = entries.unwrap_or_else(|e| panic!("scan failed under a search-only parent: {e}"));
    assert!(
        entries.contains_key(&root.join("sub")),
        "{:?}",
        entries.keys()
    );
    let children = entries
        .get(&root)
        .unwrap_or_else(|| unreachable!("{:?}", entries.keys()));
    assert!(children.contains_key(Path::new("kept.txt")), "{children:?}");
}

#[test]
fn the_filesystem_root_is_scanned_without_a_containing_directory() {
    let top_level: BTreeSet<PathBuf> = fs::read_dir("/")
        .unwrap_or_else(|e| unreachable!("{e}"))
        .map(|entry| entry.unwrap_or_else(|e| unreachable!("{e}")).path())
        .collect();
    let mut budget = Budget::new(usize::MAX);
    let entries = scan(Path::new("/"), &top_level, &mut budget);
    let entries = entries.unwrap_or_else(|e| panic!("scan of / failed: {e}"));
    let children = entries
        .get(Path::new("/"))
        .unwrap_or_else(|| unreachable!("{:?}", entries.keys()));
    assert_eq!(children.len(), top_level.len(), "{children:?}");
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}
