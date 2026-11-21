// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::MetadataExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::super::latest::newest;
use super::decision::{arena, at_epoch};
use super::*;
use crate::io::restore_record::Record;

fn tree(directory: &Path) {
    fs::create_dir_all(directory.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(directory.join("source"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    for path in [
        directory.join("source"),
        directory.join("target"),
        directory.to_path_buf(),
    ] {
        at_epoch(&path);
    }
}

fn changed(path: &Path) -> SystemTime {
    let metadata = fs::symlink_metadata(path).unwrap_or_else(|e| unreachable!("{e}"));
    UNIX_EPOCH
        + Duration::new(
            u64::try_from(metadata.ctime()).unwrap_or_else(|e| unreachable!("{e}")),
            u32::try_from(metadata.ctime_nsec()).unwrap_or_else(|e| unreachable!("{e}")),
        )
}

fn probed(base: &Path) -> SystemTime {
    let probe = base.join("probe");
    let deadline = SystemTime::now() + Duration::from_secs(30);
    let created = {
        fs::write(&probe, b"").unwrap_or_else(|e| unreachable!("{e}"));
        changed(&probe)
    };
    loop {
        fs::write(&probe, b"x").unwrap_or_else(|e| unreachable!("{e}"));
        if changed(&probe) > created {
            return changed(&probe);
        }
        assert!(SystemTime::now() < deadline, "the clock never advanced");
    }
}

fn newest_of(root: &Path) -> io::Result<Option<SystemTime>> {
    let targets = BTreeSet::from([root.join("target")]);
    let scan = walk(
        root,
        &targets,
        &mut Budget::new(usize::MAX),
        &Probe::default(),
    )?;
    let entries = view(&scan.entries, root, &targets)?;
    newest(&scan, &entries, &Record::default())
}

#[test]
fn a_directory_holding_an_excluded_entry_that_was_replaced_by_an_older_one_counts_its_change_time()
{
    let base = arena("excluded-replaced");
    let (staged, root) = (base.join("staged"), base.join("root"));
    tree(&staged);
    tree(&root);
    let after = probed(&base);
    fs::rename(&root, base.join("old")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::rename(&staged, &root).unwrap_or_else(|e| unreachable!("{e}"));
    let newest = newest_of(&root);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(Some(time)) if time >= after),
        "a replaced directory was accepted as untouched: {newest:?}"
    );
}

#[test]
fn an_excluded_entry_created_after_the_build_does_not_count_as_a_change() {
    let base = arena("excluded-created");
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("source"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    at_epoch(&root.join("source"));
    let after = probed(&base);
    fs::create_dir(root.join("target")).unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(root.join("target/output"), b"").unwrap_or_else(|e| unreachable!("{e}"));
    let newest = newest_of(&root);
    fs::remove_dir_all(&base).unwrap_or_else(|e| unreachable!("{e}"));
    assert!(
        matches!(newest, Ok(Some(time)) if time < after),
        "an excluded entry's creation was read as a change: {newest:?}"
    );
}
