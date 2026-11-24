// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;

use super::super::latest::{Hooks, newest_with};
use super::descriptors::{chain_child, deep_tree};
use super::walk::HELD_HANDLES;
use super::*;

const DEPTH: usize = 200;

fn comb(name: &str, depth: usize) -> PathBuf {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!(
            "velnor-scan-checkpoints-{name}-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    deep_tree(&root, depth, 4);
    root
}

fn scan_comb(root: &Path, probe: &Probe<'_>) -> io::Result<Entries> {
    scan_with(root, &BTreeSet::new(), &mut Budget::new(usize::MAX), probe)
}

#[test]
fn an_evicted_parent_is_reopened_from_a_retained_ancestor_in_a_bounded_number_of_steps() {
    let root = comb("bounded", DEPTH);
    let steps = RefCell::new(Vec::new());
    let entries = scan_comb(
        &root,
        &Probe {
            reopened: &|_, taken| steps.borrow_mut().push(taken),
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let entries = entries.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(entries.len(), 1 + DEPTH * 5, "{}", entries.len());
    let steps = steps.into_inner();
    let longest = steps.iter().copied().max().unwrap_or(0);
    let bound = 2 * DEPTH.div_ceil(HELD_HANDLES);
    assert!(steps.len() > DEPTH, "only {} reopens", steps.len());
    assert!(
        longest <= bound,
        "a reopen took {longest} steps, more than {bound}, in a tree {DEPTH} levels deep"
    );
}

const CHANGED: &str = "changed while the audit scanned it; retry";

fn disturbed(
    name: &str,
    disturb: &dyn Fn(&Path, usize, &BTreeSet<PathBuf>) -> bool,
) -> (io::Result<Entries>, Option<PathBuf>) {
    let root = comb(name, 40);
    let opened = RefCell::new(BTreeSet::new());
    let hit = RefCell::new(None);
    let entries = scan_comb(
        &root,
        &Probe {
            held: 8,
            after_open: &|directory| {
                opened.borrow_mut().insert(directory.to_path_buf());
            },
            reopened: &|parent, steps| {
                let anchored = steps < parent.components().count();
                if anchored && hit.borrow().is_none() && disturb(parent, steps, &opened.borrow()) {
                    *hit.borrow_mut() = Some(parent.to_path_buf());
                }
            },
            ..Probe::default()
        },
    );
    let hit = hit.into_inner();
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    (entries, hit)
}

fn aside(directory: &Path) {
    fs::rename(directory, directory.with_file_name("aside"))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn refused(name: &str, disturb: &dyn Fn(&Path, usize, &BTreeSet<PathBuf>) -> bool) {
    let (entries, hit) = disturbed(name, disturb);
    assert!(
        hit.is_some(),
        "no parent was reopened from a retained ancestor"
    );
    assert!(
        matches!(&entries, Err(error) if error.to_string().contains(CHANGED)),
        "{entries:?}"
    );
}

#[test]
fn a_parent_swapped_for_a_new_directory_is_refused_when_reopened_from_a_retained_ancestor() {
    refused("swapped", &|parent, _, _| {
        aside(parent);
        fs::create_dir(parent).unwrap_or_else(|e| unreachable!("{e}"));
        true
    });
}

#[test]
fn a_parent_removed_is_refused_when_reopened_from_a_retained_ancestor() {
    refused("removed", &|parent, _, _| {
        aside(parent);
        true
    });
}

#[test]
fn an_ancestor_between_the_checkpoint_and_the_parent_swapped_is_refused() {
    refused("intermediate", &|parent, steps, _| {
        let Some(middle) = parent.parent().filter(|_| steps >= 2) else {
            return false;
        };
        aside(middle);
        fs::create_dir(middle).unwrap_or_else(|e| unreachable!("{e}"));
        true
    });
}

#[test]
fn a_child_restamped_after_its_parent_was_evicted_is_refused_from_a_retained_ancestor() {
    refused("restamped", &|parent, _, _| {
        for entry in fs::read_dir(parent).unwrap_or_else(|e| unreachable!("{e}")) {
            let entry = entry.unwrap_or_else(|e| unreachable!("{e}"));
            fs::File::open(entry.path())
                .and_then(|directory| directory.set_modified(std::time::UNIX_EPOCH))
                .unwrap_or_else(|e| unreachable!("{e}"));
        }
        true
    });
}

#[test]
fn a_child_removed_before_it_is_reopened_from_a_retained_ancestor_is_skipped() {
    let vanished = RefCell::new(Vec::new());
    let (entries, hit) = disturbed("vanished", &|parent, _, opened| {
        let chain = chain_child(parent);
        let mut gone = Vec::new();
        for entry in fs::read_dir(parent).unwrap_or_else(|e| unreachable!("{e}")) {
            let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
            if path.file_name() == chain.as_deref() || opened.contains(&path) {
                continue;
            }
            fs::remove_dir(&path).unwrap_or_else(|e| unreachable!("{e}"));
            gone.push(path);
        }
        let any = !gone.is_empty();
        vanished.borrow_mut().extend(gone);
        any
    });
    let entries = entries.unwrap_or_else(|e| panic!("a vanished child failed the scan: {e}"));
    let parent = hit.unwrap_or_else(|| panic!("no parent was reopened from a retained ancestor"));
    let vanished = vanished.into_inner();
    let children = &entries[&parent];
    for path in vanished {
        assert!(
            !children.contains_key(path.file_name().map_or(Path::new(""), Path::new)),
            "{path:?}"
        );
        assert!(!entries.contains_key(&path), "{path:?}");
    }
}

#[test]
fn a_child_swapped_for_a_link_after_its_parent_was_evicted_is_recorded_as_a_link() {
    let root = comb("linked", 40);
    let target = root.with_file_name(format!(
        "velnor-scan-checkpoints-target-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&target);
    fs::create_dir_all(target.join("inside")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(target.join("planted"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let opened = RefCell::new(BTreeSet::new());
    let swapped = RefCell::new(None);
    let entries = scan_comb(
        &root,
        &Probe {
            held: 8,
            after_open: &|directory| {
                opened.borrow_mut().insert(directory.to_path_buf());
            },
            reopened: &|parent, steps| {
                if steps >= parent.components().count() || swapped.borrow().is_some() {
                    return;
                }
                let chain = chain_child(parent);
                for entry in fs::read_dir(parent).unwrap_or_else(|e| unreachable!("{e}")) {
                    let path = entry.unwrap_or_else(|e| unreachable!("{e}")).path();
                    if path.file_name() == chain.as_deref() || opened.borrow().contains(&path) {
                        continue;
                    }
                    fs::remove_dir(&path).unwrap_or_else(|e| unreachable!("{e}"));
                    std::os::unix::fs::symlink(&target, &path)
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    *swapped.borrow_mut() = Some(path);
                    return;
                }
            },
            ..Probe::default()
        },
    );
    let _ = fs::remove_dir_all(&target);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let link = swapped
        .into_inner()
        .unwrap_or_else(|| panic!("no sibling was swapped after an anchored reopen"));
    let entries = entries.unwrap_or_else(|e| panic!("{e}"));
    let parent = link.parent().unwrap_or_else(|| unreachable!());
    let name = link.file_name().map_or(Path::new(""), Path::new);
    assert!(
        matches!(&entries[parent][name], Kind::Link { resolved: Some(resolved), .. } if *resolved == target),
        "{:?}",
        entries[parent][name]
    );
    assert!(
        !entries.keys().any(|key| key.starts_with(&link)),
        "{link:?} was followed"
    );
    assert!(
        entries.contains_key(&target),
        "the link target was not recorded"
    );
}

#[test]
fn an_inventory_scanned_through_retained_ancestors_equals_the_unbounded_one() {
    let root = comb("equal", 40);
    let bounded = scan_comb(
        &root,
        &Probe {
            held: 8,
            ..Probe::default()
        },
    );
    let unbounded = scan_comb(
        &root,
        &Probe {
            held: usize::MAX,
            ..Probe::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        bounded.unwrap_or_else(|e| panic!("{e}")),
        unbounded.unwrap_or_else(|e| panic!("{e}"))
    );
}

#[test]
fn a_checkpoint_moved_during_the_scan_is_rejected_by_the_freshness_pass() {
    let root = comb("moved", 40);
    let moved = RefCell::new(None);
    let scan = walk(
        &root,
        &BTreeSet::new(),
        &mut Budget::new(usize::MAX),
        &Probe {
            held: 8,
            reopened: &|parent, steps| {
                let anchored = steps < parent.components().count();
                let anchor = parent.ancestors().nth(steps);
                let pending = moved.borrow().is_none();
                if let (true, true, Some(anchor)) = (anchored, pending, anchor) {
                    aside(anchor);
                    *moved.borrow_mut() = Some(anchor.to_path_buf());
                }
            },
            ..Probe::default()
        },
    );
    let newest = scan
        .as_ref()
        .map(|scan| newest_with(scan, &scan.entries, &Hooks::default()));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(moved.into_inner().is_some(), "no checkpoint was moved");
    assert!(
        matches!(newest, Ok(Ok(None))),
        "a scan through a moved checkpoint was accepted as fresh: {newest:?}"
    );
}
