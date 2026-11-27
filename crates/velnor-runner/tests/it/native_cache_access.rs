// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, OpenOptions};
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, SystemTime};

use anyhow::Result;

use crate::native_cache_support::{Store, run, saved};

const THREE_DAYS: Duration = Duration::from_hours(72);
const FRESH: Duration = Duration::from_secs(300);

fn age(store: &Store, key: &str) -> Result<()> {
    let stamp = store.entry(key)?.dir.join("accessed");
    fs::set_permissions(&stamp, fs::Permissions::from_mode(0o644))?;
    OpenOptions::new()
        .write(true)
        .open(stamp)?
        .set_modified(SystemTime::now() - THREE_DAYS)?;
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
