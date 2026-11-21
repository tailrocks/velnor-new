// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::MetadataExt;
use std::time::UNIX_EPOCH;

use super::super::latest::{Hooks, newest_with};
use super::*;

fn arena(name: &str) -> PathBuf {
    let base = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-decision-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    base
}

fn at_epoch(path: &Path) {
    fs::File::open(path)
        .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn scanned(root: &Path) -> Scan {
    walk(
        root,
        &BTreeSet::new(),
        &mut Budget::new(usize::MAX),
        &Probe::default(),
    )
    .unwrap_or_else(|e| unreachable!("{e}"))
}

fn flat_directory(name: &str) -> (PathBuf, Scan) {
    let root = arena(name);
    for file in ["a", "b"] {
        fs::write(root.join(file), b"").unwrap_or_else(|e| unreachable!("{e}"));
        at_epoch(&root.join(file));
    }
    at_epoch(&root);
    let scan = scanned(&root);
    (root, scan)
}

#[test]
fn a_link_target_replaced_by_a_directory_after_the_scan_makes_the_snapshot_stale() {
    let base = arena("link-to-directory");
    let root = base.join("root");
    let outside = base.join("outside");
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&outside, b"").unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap_or_else(|e| unreachable!("{e}"));
    let scan = scanned(&root);
    fs::remove_file(&outside).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir(&outside).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(outside.join("new"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let newest = newest_with(&scan, &scan.entries, None, &Hooks::default());
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(None)),
        "a link target that became a directory was accepted: {newest:?}"
    );
}

#[test]
fn a_directory_that_grew_after_the_scan_is_not_read_to_its_end() {
    let (root, scan) = flat_directory("grown");
    for index in 0..3_000 {
        fs::write(root.join(format!("late-{index}")), b"").unwrap_or_else(|e| unreachable!("{e}"));
    }
    let read = AtomicUsize::new(0);
    let newest = newest_with(
        &scan,
        &scan.entries,
        None,
        &Hooks {
            listed: &|_| {
                read.fetch_add(1, Ordering::Relaxed);
            },
            ..Hooks::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(newest, Ok(None)), "{newest:?}");
    let read = read.load(Ordering::Relaxed);
    assert!(read <= 3, "read {read} names of a directory that held 2");
}

#[test]
fn a_directory_changed_while_its_children_are_observed_makes_the_snapshot_stale() {
    let (root, scan) = flat_directory("observed");
    let changed = AtomicBool::new(false);
    let newest = newest_with(
        &scan,
        &scan.entries,
        None,
        &Hooks {
            observed: &|_| {
                if !changed.swap(true, Ordering::Relaxed) {
                    fs::write(root.join("late"), b"").unwrap_or_else(|e| unreachable!("{e}"));
                }
            },
            ..Hooks::default()
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(None)),
        "a change made while the children were observed was accepted: {newest:?}"
    );
}

fn set_modified(path: &Path, seconds: u64) {
    fs::File::open(path)
        .and_then(|file| {
            file.set_times(
                fs::FileTimes::new()
                    .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
            )
        })
        .unwrap_or_else(|e| unreachable!("{e}"));
}

fn change_time(path: &Path) -> std::time::SystemTime {
    let metadata = fs::metadata(path).unwrap_or_else(|e| unreachable!("{e}"));
    let seconds = u64::try_from(metadata.ctime()).unwrap_or_else(|e| unreachable!("{e}"));
    let nanos = u32::try_from(metadata.ctime_nsec()).unwrap_or_else(|e| unreachable!("{e}"));
    UNIX_EPOCH + std::time::Duration::new(seconds, nanos)
}

#[test]
fn a_link_target_ancestor_swapped_after_it_is_resolved_never_reports_the_decoy_time() {
    let base = arena("ancestor-swap");
    let root = base.join("root");
    let real = base.join("ext").join("target.txt");
    let decoy = base.join("evil").join("target.txt");
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    for file in [&real, &decoy] {
        fs::create_dir_all(file.parent().unwrap_or_else(|| unreachable!("parent")))
            .unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    }
    std::os::unix::fs::symlink("../ext/target.txt", root.join("link"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    set_modified(&decoy, 4_000_000_000);
    let scan = scanned(&root);
    set_modified(&real, 1_700_000_000);
    let real_time = change_time(&real);
    let swapped = AtomicBool::new(false);
    let newest = newest_with(
        &scan,
        &scan.entries,
        None,
        &Hooks {
            resolved: &|_| {
                if !swapped.swap(true, Ordering::Relaxed) {
                    fs::rename(base.join("ext"), base.join("ext.gone"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::rename(base.join("evil"), base.join("ext"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                }
            },
            ..Hooks::default()
        },
    );
    let swapped = swapped.load(Ordering::Relaxed);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(swapped, "the ancestor was never swapped");
    let kept = match &newest {
        Ok(None) => true,
        Ok(Some(time)) => *time == real_time,
        Err(_) => false,
    };
    assert!(
        kept,
        "an ancestor swapped after the target was resolved leaked the decoy: {newest:?}"
    );
}
