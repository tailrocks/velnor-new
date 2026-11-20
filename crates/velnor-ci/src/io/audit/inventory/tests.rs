// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::atomic::{AtomicBool, Ordering};
use std::{fs, thread};

use super::walk::scan_with;
use super::*;

fn scan_racing(name: &str, race: impl Fn(&Path) -> io::Result<fs::ReadDir>) -> io::Result<Entries> {
    let root = std::env::temp_dir().join(format!("velnor-scan-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("sub")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("kept.txt"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let mut budget = Budget::new(usize::MAX);
    let result = scan_with(&root, &BTreeSet::new(), &mut budget, |directory| {
        if directory.ends_with("sub") {
            race(directory)
        } else {
            fs::read_dir(directory)
        }
    });
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    result
}

fn root_children(entries: &Entries) -> &BTreeMap<PathBuf, Kind> {
    entries
        .values()
        .next()
        .unwrap_or_else(|| unreachable!("root is always recorded"))
}

#[test]
fn a_directory_removed_before_it_is_opened_is_skipped_not_an_error() {
    let entries = scan_racing("removed", |directory: &Path| -> io::Result<fs::ReadDir> {
        fs::remove_dir(directory)?;
        fs::read_dir(directory)
    });
    let Ok(entries) = entries else {
        panic!("scan failed on a directory removed before it was opened: {entries:?}");
    };
    let children = root_children(&entries);
    assert!(children.contains_key(Path::new("kept.txt")), "{children:?}");
    assert!(!children.contains_key(Path::new("sub")), "{children:?}");
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

#[test]
fn a_directory_replaced_by_a_regular_file_before_it_is_opened_is_recorded_as_that_file() {
    let entries = scan_racing(
        "replaced-file",
        |directory: &Path| -> io::Result<fs::ReadDir> {
            fs::remove_dir(directory)?;
            fs::write(directory, b"")?;
            fs::read_dir(directory)
        },
    );
    let Ok(entries) = entries else {
        panic!("scan failed on a directory replaced by a file: {entries:?}");
    };
    let children = root_children(&entries);
    assert_eq!(children.get(Path::new("sub")), Some(&Kind::File), "{children:?}");
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

#[test]
fn a_directory_replaced_by_a_symbolic_link_before_it_is_opened_is_recorded_as_that_link() {
    let entries = scan_racing(
        "replaced-link",
        |directory: &Path| -> io::Result<fs::ReadDir> {
            let target = directory.with_file_name("kept.txt");
            fs::remove_dir(directory)?;
            std::os::unix::fs::symlink(target, directory)?;
            fs::read_dir(directory)
        },
    );
    let Ok(entries) = entries else {
        panic!("scan failed on a directory replaced by a link: {entries:?}");
    };
    let children = root_children(&entries);
    assert!(
        matches!(children.get(Path::new("sub")), Some(Kind::Link { .. })),
        "{children:?}"
    );
    assert_eq!(entries.len(), 1, "{:?}", entries.keys());
}

#[test]
fn a_directory_that_is_a_directory_again_before_it_is_opened_still_fails_the_scan() {
    let entries = scan_racing(
        "directory-again",
        |directory: &Path| -> io::Result<fs::ReadDir> {
            fs::remove_dir(directory)?;
            fs::create_dir(directory)?;
            Err(io::Error::from(io::ErrorKind::NotFound))
        },
    );
    assert!(
        matches!(&entries, Err(error) if error.kind() == io::ErrorKind::NotFound),
        "{entries:?}"
    );
}

#[test]
fn a_directory_that_cannot_be_opened_still_fails_the_scan() {
    let entries = scan_racing("denied", |_: &Path| {
        Err(io::Error::from(io::ErrorKind::PermissionDenied))
    });
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
