// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::symlink;

use super::handle::{self, Origin};
use super::walk::DESCRIPTOR_BUDGET;
use super::*;

fn deep_tree(root: &Path, depth: usize, siblings: usize) {
    let mut directory = root.to_path_buf();
    for _ in 0..depth {
        for sibling in 0..siblings {
            fs::create_dir_all(directory.join(format!("s{sibling}")))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        directory = directory.join("c");
        fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }
}

fn open_descriptors() -> usize {
    fs::read_dir("/dev/fd").map_or(0, Iterator::count)
}

fn scan_deep(name: &str, depth: usize, held: usize) -> (io::Result<Entries>, usize) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    deep_tree(&root, depth, 4);
    let peak = AtomicUsize::new(0);
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            after_open: &|_| {
                peak.fetch_max(open_descriptors(), Ordering::Relaxed);
            },
            held,
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    (result, peak.load(Ordering::Relaxed))
}

#[test]
fn a_deep_tree_with_pending_siblings_is_walked_within_a_small_descriptor_budget() {
    let baseline = open_descriptors();
    let (entries, peak) = scan_deep("deep-budget", 160, Probe::default().held);
    let entries = entries.unwrap_or_else(|e| panic!("deep tree scan failed: {e}"));
    assert_eq!(entries.len(), 1 + 160 * 5, "{}", entries.len());
    assert!(
        peak <= baseline + DESCRIPTOR_BUDGET,
        "walk held {peak} descriptors (baseline {baseline}) in a tree 160 levels deep"
    );
}

fn deep_links_tree(root: &Path, depth: usize, linked_from: usize, target_depth: usize) {
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
fn a_deep_tree_with_deep_link_targets_stays_within_one_descriptor_budget() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-links-budget-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    deep_links_tree(&root, 120, 56, 200);
    let baseline = open_descriptors();
    let peak = AtomicUsize::new(0);
    let sample = || {
        peak.fetch_max(open_descriptors(), Ordering::Relaxed);
    };
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            after_open: &|_| sample(),
            opened: &sample,
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    result.unwrap_or_else(|e| panic!("deep link scan failed: {e}"));
    let peak = peak.load(Ordering::Relaxed);
    assert!(
        peak <= baseline + DESCRIPTOR_BUDGET,
        "audit held {peak} descriptors (baseline {baseline}), more than {DESCRIPTOR_BUDGET}"
    );
}

#[test]
fn releasing_every_parent_handle_records_the_same_inventory() {
    let (kept, _) = scan_deep("kept-handles", 12, usize::MAX);
    let (released, _) = scan_deep("released-handles", 12, 0);
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

#[test]
fn a_replaced_parent_that_reuses_the_inode_is_refused() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-recycled-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let parent = root.join("d");
    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&parent).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(parent.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    let replacement = handle::open(&Origin::Path, &parent).unwrap_or_else(|e| unreachable!("{e}"));
    let mut recycled = handle::identity(&replacement).unwrap_or_else(|e| unreachable!("{e}"));
    recycled.born = Some(std::time::UNIX_EPOCH);
    let origin = Origin::Reopen {
        parent: parent.clone(),
        identity: recycled,
        name: "sub".into(),
    };
    let opened = handle::open(&origin, &parent.join("sub"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(&opened, Err(error) if error.to_string().contains("changed while the audit scanned it; retry")),
        "a replacement with the recorded inode but another birth must be refused: {:?}",
        opened.as_ref().err()
    );
}
