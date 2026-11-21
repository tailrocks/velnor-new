// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{MetadataExt, PermissionsExt};

use super::*;

fn checkout(name: &str) -> PathBuf {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-record-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src")).unwrap_or_else(|e| unreachable!("{e}"));
    let init = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["init", "-q"])
        .status()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(init.success());
    root
}

fn change(path: &Path) -> Change {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|e| unreachable!("{e}"));
    (metadata.ctime(), metadata.ctime_nsec())
}

#[test]
fn only_paths_recorded_at_their_current_change_time_are_vouched() {
    let root = checkout("vouch");
    let recorded = root.join("src/recorded.rs");
    let other = root.join("src/other.rs");
    for file in [&recorded, &other] {
        fs::write(file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    }
    record(
        &root,
        &Restored {
            top: Some(root.clone()),
            paths: vec![recorded.clone()],
        },
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let read = Record::read(&root);
    let vouched = (
        read.vouches(&recorded, change(&recorded)),
        read.vouches(&root.join("src"), change(&root.join("src"))),
        read.vouches(&root, change(&root)),
        read.vouches(&other, change(&other)),
    );
    let mut permissions = fs::metadata(&recorded)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&recorded, permissions).unwrap_or_else(|e| unreachable!("{e}"));
    let after = Record::read(&root).vouches(&recorded, change(&recorded));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(vouched, (true, true, true, false));
    assert!(!after, "a changed file stayed vouched");
}

#[test]
fn a_new_record_drops_the_entries_that_changed_since_the_last_one() {
    let root = checkout("merge");
    let first = root.join("src/first.rs");
    let second = root.join("src/second.rs");
    for file in [&first, &second] {
        fs::write(file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    }
    let restored = |paths: Vec<PathBuf>| Restored { top: None, paths };
    record(&root, &restored(vec![first.clone()])).unwrap_or_else(|e| unreachable!("{e}"));
    let mut permissions = fs::metadata(&first)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&first, permissions).unwrap_or_else(|e| unreachable!("{e}"));
    record(&root, &restored(vec![second.clone()])).unwrap_or_else(|e| unreachable!("{e}"));
    let read = Record::read(&root);
    let vouched = (
        read.vouches(&first, change(&first)),
        read.vouches(&second, change(&second)),
    );
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(vouched, (false, true));
}

#[test]
fn clearing_the_record_vouches_nothing() {
    let root = checkout("clear");
    let file = root.join("src/a.rs");
    fs::write(&file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    record(
        &root,
        &Restored {
            top: None,
            paths: vec![file.clone()],
        },
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    clear(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let vouched = Record::read(&root).vouches(&file, change(&file));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!vouched);
}
