// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::handle::{self, Origin};
use super::*;

fn released_scan(name: &str, change: &dyn Fn(&Path)) -> (io::Result<Entries>, PathBuf) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!(
            "velnor-scan-released-{name}-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    let parent = root.join("d");
    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(parent.join("kept.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let changed = AtomicBool::new(false);
    let mut budget = Budget::new(usize::MAX);
    let entries = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with("d/sub") && !changed.swap(true, Ordering::Relaxed) {
                    change(directory);
                }
                Ok(())
            },
            held: 0,
            ..Probe::default()
        },
    );
    assert!(
        changed.load(Ordering::Relaxed),
        "the tree was never changed"
    );
    (entries, root)
}

#[test]
fn a_child_removed_before_it_is_opened_is_skipped_when_its_parent_was_released() {
    let (entries, root) = released_scan("vanished", &|directory| {
        fs::remove_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
    });
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let entries = entries.unwrap_or_else(|e| panic!("a vanished child failed the scan: {e}"));
    let children = &entries[&root.join("d")];
    assert!(children.contains_key(Path::new("kept.txt")), "{children:?}");
    assert!(!children.contains_key(Path::new("sub")), "{children:?}");
    assert_eq!(entries.len(), 2, "{:?}", entries.keys());
}

#[test]
fn a_sibling_added_to_a_released_parent_does_not_fail_the_scan() {
    let (entries, root) = released_scan("sibling", &|directory| {
        fs::write(directory.with_file_name("added.txt"), b"")
            .unwrap_or_else(|e| unreachable!("{e}"));
    });
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let entries = entries.unwrap_or_else(|e| panic!("a new sibling failed the scan: {e}"));
    let children = &entries[&root.join("d")];
    assert!(children.contains_key(Path::new("sub")), "{children:?}");
    assert!(
        !children.contains_key(Path::new("added.txt")),
        "{children:?}"
    );
}

#[test]
fn a_child_whose_stamp_differs_from_the_one_recorded_at_release_is_refused() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-child-stamp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let parent = root.join("d");
    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    let stamp_of = |path: &Path| {
        handle::stamp(handle::open(&Origin::Path, path).unwrap_or_else(|e| unreachable!("{e}")))
            .unwrap_or_else(|e| unreachable!("{e}"))
    };
    let origin = Origin::Reopen {
        parent: parent.clone(),
        stamp: stamp_of(&parent),
        child: Some(stamp_of(&parent.join("sub"))),
        name: "sub".into(),
    };
    let unchanged = handle::open(&origin, &parent.join("sub"));
    fs::write(parent.join("added.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&parent)
        .and_then(|directory| directory.set_modified(std::time::UNIX_EPOCH))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let busier_parent = handle::open(&origin, &parent.join("sub"));
    fs::File::open(parent.join("sub"))
        .and_then(|directory| directory.set_modified(std::time::UNIX_EPOCH))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let refused = handle::open(&origin, &parent.join("sub"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(unchanged.is_ok(), "{:?}", unchanged.err());
    assert!(busier_parent.is_ok(), "{:?}", busier_parent.err());
    assert!(
        matches!(&refused, Err(error) if error.to_string().contains("changed while the audit scanned it; retry")),
        "a child with another stamp than recorded must be refused: {:?}",
        refused.err()
    );
}

#[test]
fn a_released_parent_replaced_by_an_empty_directory_fails_the_scan() {
    let (entries, root) = released_scan("replaced-empty", &|directory| {
        let parent = directory
            .parent()
            .unwrap_or_else(|| unreachable!("a child has a parent"));
        fs::rename(parent, parent.with_file_name("d.aside"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::create_dir(parent).unwrap_or_else(|e| unreachable!("{e}"));
    });
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains("changed while the audit scanned it; retry")),
        "{entries:?}"
    );
}
