// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;
use std::time::Duration;

use anyhow::{Result, ensure};

use crate::native_cache_support::{Store, overwrite_entry, run, run_bounded, saved, unlock_entry};

const MISS: &str = "hit=[] matched=[] primary=[k-1]";

#[test]
fn a_fifo_data_tar_fails_the_restore_and_does_not_hang() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let data = unlock_entry(&store)?.join("data.tar");
    fs::remove_file(&data)?;
    ensure!(Command::new("mkfifo").arg(&data).status()?.success());
    let restored = run_bounded("cache-restore-exact", &store, &[], Duration::from_secs(30))?;
    assert_eq!(restored.ran.code, Some(1), "{}", restored.ran.stdout);
    assert!(
        restored.ran.stdout.contains("not a regular file"),
        "{}",
        restored.ran.stdout
    );
    assert!(!restored.workspace_file("data").exists());
    Ok(())
}

#[test]
fn a_symlinked_marker_key_or_version_hides_the_entry() -> Result<()> {
    for name in ["complete", "key", "version"] {
        let store = Store::new()?;
        saved(&store, "k-1", "hello")?;
        let entry = unlock_entry(&store)?;
        let target = store.root().with_file_name(format!("target-{name}"));
        fs::copy(entry.join(name), &target)?;
        fs::remove_file(entry.join(name))?;
        symlink(&target, entry.join(name))?;
        let restored = run("cache-restore-exact", &store, &[])?;
        assert!(restored.succeeded(), "{name}: {}", restored.ran.stdout);
        assert!(
            restored.ran.has_line(MISS),
            "{name}: {}",
            restored.ran.stdout
        );
        assert!(!restored.workspace_file("data").exists());
    }
    Ok(())
}

#[test]
fn a_symlinked_data_tar_is_never_read() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let data = unlock_entry(&store)?.join("data.tar");
    let copy = store.root().with_file_name("copy-of-data.tar");
    fs::copy(&data, &copy)?;
    fs::remove_file(&data)?;
    symlink(&copy, &data)?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(
        restored.ran.code == Some(1) || restored.ran.has_line(MISS),
        "{}",
        restored.ran.stdout
    );
    assert!(!restored.workspace_file("data").exists());
    Ok(())
}

#[test]
fn an_entry_whose_archive_holds_no_members_is_a_miss_with_a_warning() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    overwrite_entry(&store, &[])?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(
        restored
            .ran
            .stdout
            .contains("::warning::Failed to restore: the cache entry for key k-1 holds no members"),
        "{}",
        restored.ran.stdout
    );
    assert!(restored.ran.has_line(MISS), "{}", restored.ran.stdout);
    assert!(!restored.workspace_file("data").exists());
    Ok(())
}
