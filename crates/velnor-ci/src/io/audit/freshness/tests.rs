// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::time::{Duration, UNIX_EPOCH};

use super::*;

fn snapshot(name: &str) -> (PathBuf, inventory::Scan) {
    let root = std::env::temp_dir().join(format!("velnor-freshness-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    for file in ["kept.txt", "gone.txt"] {
        let path = root.join(file);
        fs::write(&path, b"").unwrap_or_else(|e| unreachable!("{e}"));
        fs::File::open(&path)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let mut budget = Budget::new(usize::MAX);
    let scan = inventory::scan(&root, &BTreeSet::new(), &mut budget)
        .unwrap_or_else(|e| unreachable!("{e}"));
    (root, scan)
}

#[test]
fn a_snapshot_whose_entries_all_remain_has_a_newest_file() {
    let (root, scan) = snapshot("control");
    let newest = inventory::newest(&scan, &scan.entries, &Record::default());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(newest, Ok(Some(_))), "{newest:?}");
}

#[test]
fn an_entry_removed_after_the_scan_is_reported_as_vanished_not_as_an_error() {
    let (root, scan) = snapshot("vanished");
    fs::remove_file(root.join("gone.txt")).unwrap_or_else(|e| unreachable!("{e}"));
    let newest = inventory::newest(&scan, &scan.entries, &Record::default());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(matches!(newest, Ok(None)), "{newest:?}");
}

#[test]
fn a_write_after_the_walk_is_seen_by_the_decision() {
    let (root, scan) = snapshot("late-write");
    let future = UNIX_EPOCH + std::time::Duration::from_secs(4_000_000_000);
    fs::File::open(root.join("kept.txt"))
        .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(future)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let newest = inventory::newest(&scan, &scan.entries, &Record::default());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(Some(time)) if time == future),
        "a write after the walk was missed: {newest:?}"
    );
}

fn directory_snapshot(name: &str) -> (PathBuf, inventory::Scan) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-freshness-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let directory = root.join("d");
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(directory.join("f"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    for path in [directory.join("f"), directory.clone(), root.clone()] {
        fs::File::open(&path)
            .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let mut budget = Budget::new(usize::MAX);
    let scan = inventory::scan(&root, &BTreeSet::new(), &mut budget)
        .unwrap_or_else(|e| unreachable!("{e}"));
    (root, scan)
}

#[test]
fn a_file_created_after_the_walk_makes_the_snapshot_stale() {
    let (root, scan) = directory_snapshot("created");
    fs::write(root.join("d").join("new"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let newest = inventory::newest(&scan, &scan.entries, &Record::default());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(None)),
        "a file created after the walk was missed: {newest:?}"
    );
}

#[test]
fn a_directory_rewritten_with_the_same_names_after_the_walk_makes_the_snapshot_stale() {
    let (root, scan) = directory_snapshot("rewritten");
    let directory = root.join("d");
    fs::remove_file(directory.join("f")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(directory.join("f"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(directory.join("f"))
        .and_then(|file| file.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let newest = inventory::newest(&scan, &scan.entries, &Record::default());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(None)),
        "a directory rewritten after the walk was accepted as the walked one: {newest:?}"
    );
}

#[test]
fn a_same_size_rewrite_with_its_modified_time_restored_makes_the_snapshot_stale() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-freshness-restored-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let directory = root.join("d");
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    let file = directory.join("f");
    fs::write(&file, b"a").unwrap_or_else(|e| unreachable!("{e}"));
    for path in [&file, &directory, &root] {
        fs::File::open(path)
            .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
    let mut budget = Budget::new(usize::MAX);
    let scan = inventory::scan(&root, &BTreeSet::new(), &mut budget)
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(&file, b"b").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&file)
        .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let newest = inventory::newest(&scan, &scan.entries, &Record::default());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let built = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    assert!(
        matches!(newest, Ok(Some(time)) if time > built),
        "a same-size rewrite with its mtime restored was not seen as newer than the build: {newest:?}"
    );
}

#[test]
fn a_single_file_rewritten_at_the_same_size_with_its_modified_time_restored_is_stale() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-freshness-file-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let file = root.join("f");
    fs::write(&file, b"a").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&file)
        .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let built = UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);
    fs::write(&file, b"b").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&file)
        .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let mut freshness =
        Freshness::new(&[], &[], usize::MAX, Rc::default()).unwrap_or_else(|e| unreachable!("{e}"));
    let changed = freshness.changed(&file, built, &file, &BTreeSet::new());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(changed, Ok(true)),
        "a same-size rewrite with its mtime restored was not stale: {changed:?}"
    );
}

fn change_time(path: &Path) -> SystemTime {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|e| unreachable!("{e}"));
    let seconds = u64::try_from(metadata.ctime()).unwrap_or_else(|e| unreachable!("{e}"));
    let nanos = u32::try_from(metadata.ctime_nsec()).unwrap_or_else(|e| unreachable!("{e}"));
    SystemTime::UNIX_EPOCH + Duration::new(seconds, nanos)
}

#[test]
fn a_file_whose_change_time_equals_the_snapshot_time_is_stale() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-freshness-ctime-tie-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let file = root.join("f");
    fs::write(&file, b"a").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&file)
        .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let built = change_time(&file);
    let mut freshness =
        Freshness::new(&[], &[], usize::MAX, Rc::default()).unwrap_or_else(|e| unreachable!("{e}"));
    let changed = freshness.changed(&file, built, &file, &BTreeSet::new());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(changed, Ok(true)),
        "a change at the snapshot time was treated as fresh: {changed:?}"
    );
}

fn walked_receipt(directory: &Path, output: &Path) {
    let mut budget = Budget::new(usize::MAX);
    let scan = inventory::scan(directory, &BTreeSet::new(), &mut budget)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let evidence = inventory::Evidence {
        entries: scan.entries,
        declared: BTreeMap::new(),
        targets: BTreeSet::new(),
    };
    let bytes = serde_json::to_vec(&evidence).unwrap_or_else(|e| unreachable!("{e}"));
    let saved = directory.with_extension("inventory.json");
    fs::write(&saved, &bytes).unwrap_or_else(|e| unreachable!("{e}"));
    let receipt = inventory::receipt(output, saved, velnor_rust::sha256_hex(&bytes))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = serde_json::to_vec(&receipt).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(inventory::receipt_path(output), text).unwrap_or_else(|e| unreachable!("{e}"));
}

fn walked_directory(name: &str) -> (PathBuf, PathBuf, PathBuf, SystemTime) {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-freshness-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let directory = root.join("d");
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    let file = directory.join("f");
    fs::write(&file, b"a").unwrap_or_else(|e| unreachable!("{e}"));
    let output = root.join("out");
    fs::write(&output, b"o").unwrap_or_else(|e| unreachable!("{e}"));
    fs::File::open(&file)
        .and_then(|handle| handle.set_times(fs::FileTimes::new().set_modified(UNIX_EPOCH)))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let changed = change_time(&file);
    walked_receipt(&directory, &output);
    (root, directory, output, changed)
}

#[test]
fn a_directory_entry_whose_change_time_equals_the_snapshot_time_is_stale() {
    let (root, directory, output, built) = walked_directory("dir-tie");
    let mut freshness =
        Freshness::new(&[], &[], usize::MAX, Rc::default()).unwrap_or_else(|e| unreachable!("{e}"));
    let changed = freshness.changed(&directory, built, &output, &BTreeSet::new());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(changed, Ok(true)),
        "a directory entry changed at the snapshot time was treated as fresh: {changed:?}"
    );
}

#[test]
fn an_untouched_directory_older_than_the_snapshot_is_fresh() {
    let (root, directory, output, changed_at) = walked_directory("dir-older");
    let built = changed_at + Duration::from_secs(1);
    let mut freshness =
        Freshness::new(&[], &[], usize::MAX, Rc::default()).unwrap_or_else(|e| unreachable!("{e}"));
    let changed = freshness.changed(&directory, built, &output, &BTreeSet::new());
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(changed, Ok(false)),
        "an untouched directory older than the snapshot was treated as stale: {changed:?}"
    );
}

fn settled_after(root: &Path, entries: &[&Path]) -> SystemTime {
    let newest = entries.iter().map(|entry| change_time(entry)).max();
    let probe = root.join("probe");
    loop {
        fs::write(&probe, b"").unwrap_or_else(|e| unreachable!("{e}"));
        let time = change_time(&probe);
        if Some(time) > newest {
            return time;
        }
    }
}

#[test]
fn a_link_retargeted_between_its_two_lookups_is_stale() {
    let root = fs::canonicalize(std::env::temp_dir())
        .unwrap_or_else(|e| unreachable!("{e}"))
        .join(format!("velnor-freshness-link-race-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    let link = root.join("link");
    for name in ["old.txt", "other.txt"] {
        fs::write(root.join(name), b"").unwrap_or_else(|e| unreachable!("{e}"));
    }
    std::os::unix::fs::symlink("old.txt", &link).unwrap_or_else(|e| unreachable!("{e}"));
    let built = settled_after(
        &root,
        &[&root.join("old.txt"), &root.join("other.txt"), &link],
    );
    let mut freshness =
        Freshness::new(&[], &[], usize::MAX, Rc::default()).unwrap_or_else(|e| unreachable!("{e}"));
    let changed = freshness.changed_with(&link, built, &link, &BTreeSet::new(), &|| {
        fs::remove_file(&link).unwrap_or_else(|e| unreachable!("{e}"));
        std::os::unix::fs::symlink("other.txt", &link).unwrap_or_else(|e| unreachable!("{e}"));
    });
    fs::remove_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(changed, Ok(true)),
        "a link retargeted between its lookups was treated as fresh: {changed:?}"
    );
}
