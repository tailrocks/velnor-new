// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::native_cache_support::{Store, run, saved};

const THREE_DAYS: Duration = Duration::from_hours(72);
const SEVEN_DAYS: Duration = Duration::from_hours(168);
const FRESH: Duration = Duration::from_secs(300);

fn age(store: &Store, key: &str) -> Result<()> {
    idle_for(store, key, THREE_DAYS)
}

fn idle_for(store: &Store, key: &str, idle: Duration) -> Result<()> {
    let stamp = store.entry(key)?.dir.join("accessed");
    fs::set_permissions(&stamp, fs::Permissions::from_mode(0o644))?;
    OpenOptions::new()
        .write(true)
        .open(stamp)?
        .set_modified(SystemTime::now() - idle)?;
    Ok(())
}

fn idle(store: &Store, key: &str) -> Result<Duration> {
    let stamp = store.entry(key)?.dir.join("accessed");
    Ok(SystemTime::now().duration_since(std::fs::metadata(stamp)?.modified()?)?)
}

#[test]
fn a_restore_hit_renews_the_access_stamp_of_the_entry() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    age(&store, "k-1")?;
    let restored = run("cache-restore-exact", &store, &[])?;
    assert!(restored.succeeded(), "{}", restored.ran.stdout);
    assert!(idle(&store, "k-1")? < FRESH);
    Ok(())
}

#[test]
fn a_lookup_only_hit_renews_the_access_stamp_of_the_entry() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    age(&store, "k-1")?;
    let looked = run("cache-lookup-only", &store, &[])?;
    assert!(looked.succeeded(), "{}", looked.ran.stdout);
    assert!(idle(&store, "k-1")? < FRESH);
    Ok(())
}

#[test]
fn another_uid_can_walk_through_the_namespaces_of_the_store_but_not_list_them() -> Result<()> {
    const LIST: u32 = 0o044;
    const WALK: u32 = 0o011;
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    let scope = store.entry("k-1")?.dir;
    let mut namespaces = Vec::new();
    let mut directory = scope.parent().map(std::path::Path::to_owned);
    while let Some(path) = directory.filter(|path| *path != store.root()) {
        namespaces.push(path.clone());
        directory = path.parent().map(std::path::Path::to_owned);
    }
    assert_eq!(namespaces.len(), 4);
    for (position, path) in namespaces.iter().enumerate() {
        let mode = fs::metadata(path)?.mode();
        assert_eq!(mode & WALK, WALK, "{}", path.display());
        assert_eq!(
            mode & LIST != 0,
            position == 0,
            "{}: only the scope directory is listable",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn an_entry_unused_for_more_than_seven_days_is_a_miss_even_before_any_save_evicts_it() -> Result<()>
{
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    idle_for(&store, "k-1", SEVEN_DAYS + Duration::from_secs(3600))?;
    let missed = run("cache-restore-exact", &store, &[])?;
    assert!(missed.succeeded(), "{}", missed.ran.stdout);
    assert!(
        missed.ran.stdout.contains("Cache not found"),
        "{}",
        missed.ran.stdout
    );
    assert!(!missed.workspace_file("data/a.txt").exists());
    Ok(())
}

#[test]
fn an_entry_unused_for_just_under_seven_days_is_still_a_hit() -> Result<()> {
    let store = Store::new()?;
    saved(&store, "k-1", "hello")?;
    idle_for(&store, "k-1", SEVEN_DAYS - Duration::from_secs(3600))?;
    let hit = run("cache-restore-exact", &store, &[])?;
    assert!(hit.succeeded(), "{}", hit.ran.stdout);
    assert!(hit.workspace_file("data/a.txt").exists());
    Ok(())
}
