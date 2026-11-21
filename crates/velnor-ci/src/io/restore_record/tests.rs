// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::time::{Duration, SystemTime};

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

fn inode(path: &Path) -> Inode {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|e| unreachable!("{e}"));
    (metadata.dev(), metadata.ino())
}

fn vouches(record: &Record, path: &Path) -> bool {
    record.vouches(inode(path), change(path))
}

fn restored(scope: Option<&Path>, paths: Vec<PathBuf>) -> Restored {
    Restored {
        top: None,
        scopes: scope.map(Path::to_path_buf).into_iter().collect(),
        paths,
    }
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
            scopes: Vec::new(),
            paths: vec![recorded.clone()],
        },
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let read = Record::read(&root);
    let vouched = (
        vouches(&read, &recorded),
        vouches(&read, &root.join("src")),
        vouches(&read, &root),
        vouches(&read, &other),
    );
    let mut permissions = fs::metadata(&recorded)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&recorded, permissions).unwrap_or_else(|e| unreachable!("{e}"));
    let after = vouches(&Record::read(&root), &recorded);
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
    record(&root, &restored(None, vec![first.clone()])).unwrap_or_else(|e| unreachable!("{e}"));
    let mut permissions = fs::metadata(&first)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&first, permissions).unwrap_or_else(|e| unreachable!("{e}"));
    record(&root, &restored(None, vec![second.clone()])).unwrap_or_else(|e| unreachable!("{e}"));
    let read = Record::read(&root);
    let vouched = (vouches(&read, &first), vouches(&read, &second));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(vouched, (false, true));
}

#[test]
fn a_restore_of_one_scope_keeps_the_entries_of_another() {
    let root = checkout("scopes");
    let first = root.join("src/first.rs");
    let second = root.join("lib/second.rs");
    fs::create_dir_all(root.join("lib")).unwrap_or_else(|e| unreachable!("{e}"));
    for file in [&first, &second] {
        fs::write(file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    }
    record(
        &root,
        &restored(Some(&root.join("src")), vec![first.clone()]),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    record(
        &root,
        &restored(Some(&root.join("lib")), vec![second.clone()]),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    let read = Record::read(&root);
    let vouched = (vouches(&read, &first), vouches(&read, &second));
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(vouched, (true, true));
}

#[test]
fn a_restore_forgets_what_an_earlier_one_vouched_inside_its_own_scope() {
    let root = checkout("rescope");
    let file = root.join("src/a.rs");
    fs::write(&file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    record(
        &root,
        &restored(Some(&root.join("src")), vec![file.clone()]),
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    record(&root, &restored(Some(&root.join("src")), Vec::new()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let vouched = vouches(&Record::read(&root), &file);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(!vouched, "a file the new restore left alone stayed vouched");
}

#[test]
fn a_hard_link_alias_of_a_restored_file_is_vouched_at_the_inodes_change_time() {
    let root = checkout("alias");
    let file = root.join("src/a.rs");
    let alias = root.join("src/alias.rs");
    fs::write(&file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    fs::hard_link(&file, &alias).unwrap_or_else(|e| unreachable!("{e}"));
    record(&root, &restored(None, vec![file.clone()])).unwrap_or_else(|e| unreachable!("{e}"));
    let read = Record::read(&root);
    let before = vouches(&read, &alias);
    let mut permissions = fs::metadata(&alias)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .permissions();
    permissions.set_mode(0o600);
    fs::set_permissions(&alias, permissions).unwrap_or_else(|e| unreachable!("{e}"));
    let after = vouches(&read, &alias);
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(before, "an alias of a restored inode was not vouched");
    assert!(!after, "an alias changed after the restore stayed vouched");
}

fn seconds((seconds, nanos): Change) -> SystemTime {
    SystemTime::UNIX_EPOCH
        + Duration::new(
            u64::try_from(seconds).unwrap_or(0),
            u32::try_from(nanos).unwrap_or(0),
        )
}

#[test]
fn settling_returns_only_once_the_clock_has_passed_the_change_times_it_covers() {
    let root = checkout("settled");
    let file = root.join("src/a.rs");
    fs::write(&file, b"").unwrap_or_else(|e| unreachable!("{e}"));
    let entry = observe(&file).unwrap_or_else(|| unreachable!("no entry"));
    let stored = entry.change;
    settle(&[entry]);
    let now = SystemTime::now();
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let floor = if stored.1 == 0 {
        seconds((stored.0 + 1, 0))
    } else {
        seconds(stored) + TICK
    };
    assert!(now >= floor, "returned {now:?} before {floor:?}");
}

#[test]
fn a_filesystem_with_whole_second_change_times_is_awaited_to_the_next_second() {
    let now = seconds((100, 500_000_000));
    assert_eq!(pause((100, 0), true, now), Duration::from_millis(500));
    assert_eq!(pause((99, 0), true, now), Duration::ZERO);
}

#[test]
fn a_filesystem_with_fine_change_times_is_awaited_for_one_tick() {
    let now = seconds((100, 5_000_000));
    assert_eq!(
        pause((100, 1_000_000), false, now),
        TICK.saturating_sub(Duration::from_millis(4))
    );
    assert_eq!(pause((99, 1_000_000), false, now), Duration::ZERO);
}
