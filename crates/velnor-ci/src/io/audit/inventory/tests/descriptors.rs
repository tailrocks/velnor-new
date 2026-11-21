// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::*;

fn walked_last(directory: &Path, name: &str) -> bool {
    let mut listing = handle::open(&handle::Origin::Path, directory, &handle::Climb::none())
        .unwrap_or_else(|e| unreachable!("{e}"));
    let names = handle::names(&mut listing, |_| Ok(())).unwrap_or_else(|e| unreachable!("{e}"));
    names.last().is_some_and(|last| last == name)
}

pub(super) fn deep_tree(root: &Path, depth: usize, siblings: usize) {
    let mut directory = root.to_path_buf();
    for _ in 0..depth {
        let chain = directory.join("c");
        fs::create_dir_all(&chain).unwrap_or_else(|e| unreachable!("{e}"));
        let mut kept = 0;
        let mut candidate = 0;
        while kept < siblings {
            let prefix = if candidate % 2 == 0 { "a" } else { "s" };
            let sibling = directory.join(format!("{prefix}{}", candidate / 2));
            candidate += 1;
            fs::create_dir(&sibling).unwrap_or_else(|e| unreachable!("{e}"));
            if walked_last(&directory, "c") {
                kept += 1;
            } else {
                fs::remove_dir(&sibling).unwrap_or_else(|e| unreachable!("{e}"));
            }
        }
        assert!(walked_last(&directory, "c"), "{directory:?}");
        directory = chain;
    }
}

fn scan_deep(name: &str, depth: usize, held: usize) -> io::Result<Entries> {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    deep_tree(&root, depth, 4);
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            held,
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    result
}

pub(super) fn deep_links_tree(root: &Path, depth: usize, linked_from: usize, target_depth: usize) {
    deep_tree(root, depth, 4);
    let mut target = root.join("t");
    for _ in 0..target_depth {
        target.push("a");
    }
    fs::create_dir_all(&target).unwrap_or_else(|e| unreachable!("{e}"));
    let relative = "a/".repeat(target_depth);
    let mut directory = root.to_path_buf();
    for level in 0..depth {
        directory.push("c");
        if level >= linked_from {
            let up = "../".repeat(level + 1);
            symlink(format!("{up}t/{relative}"), directory.join("rel"))
                .unwrap_or_else(|e| unreachable!("{e}"));
            symlink(&target, directory.join("abs")).unwrap_or_else(|e| unreachable!("{e}"));
        }
    }
}

#[test]
fn releasing_every_parent_handle_records_the_same_inventory() {
    let kept = scan_deep("kept-handles", 12, usize::MAX);
    let released = scan_deep("released-handles", 12, 0);
    let kept = kept.unwrap_or_else(|e| panic!("{e}"));
    let released = released.unwrap_or_else(|e| panic!("{e}"));
    let relative = |entries: &Entries| -> Vec<_> {
        entries
            .iter()
            .map(|(path, children)| {
                let depth = path.components().count();
                (depth, children.keys().cloned().collect::<Vec<_>>())
            })
            .collect()
    };
    assert_eq!(kept.len(), released.len());
    let mut kept = relative(&kept);
    let mut released = relative(&released);
    kept.sort();
    released.sort();
    assert_eq!(kept, released);
}

#[test]
fn a_parent_swapped_after_its_handle_was_released_fails_the_scan() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-released-swap-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let parent = root.join("d");
    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    let swapped = AtomicBool::new(false);
    let mut budget = Budget::new(usize::MAX);
    let entries = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with("d/sub") && !swapped.swap(true, Ordering::Relaxed) {
                    let parent = directory.with_file_name("");
                    fs::rename(&parent, root.join("d.aside"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
                }
                Ok(())
            },
            held: 0,
            ..Probe::default()
        },
    );
    let swapped = swapped.load(Ordering::Relaxed);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(swapped, "the parent was never swapped");
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains("changed while the audit scanned it; retry")),
        "a released parent that was swapped must fail the scan: {entries:?}"
    );
}
