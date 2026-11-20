// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs, thread};

use super::walk::{Probe, scan_with};
use super::*;

fn scan_with_open_failing(kind: io::ErrorKind) -> io::Result<Entries> {
    let root =
        std::env::temp_dir().join(format!("velnor-scan-open-{kind:?}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("kept.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with("sub") {
                    Err(io::Error::from(kind))
                } else {
                    Ok(())
                }
            },
            after_open: &|_| {},
        },
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    result
}

#[test]
fn a_directory_removed_before_it_is_opened_is_skipped_not_an_error() {
    let entries = scan_with_open_failing(io::ErrorKind::NotFound);
    let Ok(entries) = entries else {
        panic!("scan failed on a directory removed before it was opened: {entries:?}");
    };
    let root_children = entries
        .values()
        .next()
        .unwrap_or_else(|| unreachable!("root is always recorded"));
    assert!(
        root_children.contains_key(Path::new("kept.txt")),
        "{root_children:?}"
    );
    assert!(
        !root_children.contains_key(Path::new("sub")),
        "vanished directory still recorded: {root_children:?}"
    );
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

#[test]
fn a_directory_that_cannot_be_opened_still_fails_the_scan() {
    let entries = scan_with_open_failing(io::ErrorKind::PermissionDenied);
    assert!(
        matches!(&entries, Err(error) if error.kind() == io::ErrorKind::PermissionDenied),
        "{entries:?}"
    );
}

#[test]
fn an_entry_removed_during_the_walk_is_not_an_error() {
    let root = std::env::temp_dir().join(format!("velnor-scan-vanish-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let names: Vec<PathBuf> = (0..64)
        .map(|index| root.join(format!("entry-{index}")))
        .collect();
    let stop = AtomicBool::new(false);
    let failure = thread::scope(|scope| {
        scope.spawn(|| {
            while !stop.load(Ordering::Relaxed) {
                for name in &names {
                    let _ = fs::write(name, b"");
                    let _ = fs::remove_file(name);
                }
            }
        });
        let mut budget = Budget::new(usize::MAX);
        let failure = (0..2_000).find_map(|_| scan(&root, &BTreeSet::new(), &mut budget).err());
        stop.store(true, Ordering::Relaxed);
        failure
    });
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        failure.is_none(),
        "scan failed on a vanished entry: {failure:?}"
    );
}

fn swapped_directory_scan(label: &str) -> (PathBuf, Entries) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-swap-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let original = root.join("d");
    fs::create_dir_all(original.join("b")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(original.join("a"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(original.join("b").join("inner"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let swapped = AtomicBool::new(false);
    let mut budget = Budget::new(usize::MAX);
    let entries = scan_with(
        &root,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|_| Ok(()),
            after_open: &|directory| {
                if directory.ends_with("d") && !swapped.swap(true, Ordering::Relaxed) {
                    fs::rename(directory, directory.with_file_name("d.aside"))
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::create_dir_all(directory.join("a")).unwrap_or_else(|e| unreachable!("{e}"));
                    fs::write(directory.join("a").join("x"), b"")
                        .unwrap_or_else(|e| unreachable!("{e}"));
                    fs::write(directory.join("b"), b"").unwrap_or_else(|e| unreachable!("{e}"));
                }
            },
        },
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    (root, entries)
}

#[test]
fn a_directory_swapped_after_it_is_opened_is_recorded_from_the_opened_directory() {
    let (root, entries) = swapped_directory_scan("kinds");
    let children = entries
        .get(&root.join("d"))
        .unwrap_or_else(|| unreachable!("{:?}", entries.keys()));
    assert_eq!(
        children.get(Path::new("a")),
        Some(&Kind::File),
        "{children:?}"
    );
    assert_eq!(
        children.get(Path::new("b")),
        Some(&Kind::Directory),
        "{children:?}"
    );
}

#[test]
fn a_subdirectory_is_descended_through_its_opened_parent_not_its_path() {
    let (root, entries) = swapped_directory_scan("descent");
    assert!(
        !entries.contains_key(&root.join("d").join("a")),
        "walked the replacement directory: {:?}",
        entries.keys()
    );
    let inner = entries
        .get(&root.join("d").join("b"))
        .unwrap_or_else(|| unreachable!("{:?}", entries.keys()));
    assert!(inner.contains_key(Path::new("inner")), "{inner:?}");
}

#[test]
fn a_directory_replaced_by_a_symbolic_link_before_it_is_opened_is_not_followed() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-scan-link-swap-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let scanned = root.join("scanned");
    let elsewhere = root.join("elsewhere");
    fs::create_dir_all(scanned.join("d")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::create_dir_all(&elsewhere).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(elsewhere.join("secret"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let entries = scan_with(
        &scanned,
        &BTreeSet::new(),
        &mut budget,
        &Probe {
            before_open: &|directory| {
                if directory.ends_with("d") {
                    fs::remove_dir(directory).unwrap_or_else(|e| unreachable!("{e}"));
                    std::os::unix::fs::symlink(&elsewhere, directory)
                        .unwrap_or_else(|e| unreachable!("{e}"));
                }
                Ok(())
            },
            after_open: &|_| {},
        },
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        !entries.contains_key(&scanned.join("d")) && !entries.contains_key(&elsewhere),
        "followed the replacement link: {:?}",
        entries.keys()
    );
    let children = entries
        .get(&scanned)
        .unwrap_or_else(|| unreachable!("{:?}", entries.keys()));
    assert!(!children.contains_key(Path::new("d")), "{children:?}");
}
