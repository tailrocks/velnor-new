// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};

use super::cache::{CacheOwner, MARKER, scope_dir};
use super::publish::SYNCED;
use super::tests::{Fixture, TAR, published};

const PRIVATE: u32 = 0o700;

fn scope_of(fixture: &Fixture) -> Result<PathBuf> {
    let scope = fixture
        .facts
        .cache_write_scope()
        .map_err(|denied| anyhow!("{denied}"))?;
    Ok(scope_dir(
        &fixture.root(),
        &CacheOwner::of(&fixture.facts),
        &scope,
    ))
}

fn mode_of(path: &Path) -> Result<u32> {
    Ok(fs::metadata(path)?.mode() & 0o7777)
}

#[tokio::test]
async fn a_data_file_is_frozen_before_it_is_synced() -> Result<()> {
    let fixture = Fixture::new()?;
    let response = fixture.upload("k-1", TAR).await;
    assert!(published(&response), "{response:?}");
    let data = fs::metadata(fixture.entry("k-1")?.join("data.tar"))?;
    let identity = (data.dev(), data.ino());
    let synced = SYNCED
        .lock()
        .map_err(|_| anyhow!("the sync record is poisoned"))?;
    let modes: Vec<_> = synced
        .iter()
        .filter(|seen| seen.what == "the cache data" && seen.identity == identity)
        .map(|seen| seen.mode)
        .collect();
    assert_eq!(modes, [0o444]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_entry_is_never_visible_in_the_scope_before_it_is_complete() -> Result<()> {
    const ENTRIES: usize = 60;
    let fixture = Fixture::new()?;
    let scope = scope_of(&fixture)?;
    let stop = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (stop, scope) = (stop.clone(), scope.clone());
        std::thread::spawn(move || {
            let mut unfinished = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                for item in fs::read_dir(&scope).into_iter().flatten().flatten() {
                    if fs::symlink_metadata(item.path().join(MARKER)).is_err() {
                        unfinished.push(item.path());
                    }
                }
            }
            unfinished
        })
    };
    for round in 0..ENTRIES {
        let response = fixture.upload(&format!("k-{round}"), TAR).await;
        assert!(published(&response), "{response:?}");
    }
    stop.store(true, Ordering::Relaxed);
    let unfinished = watcher
        .join()
        .map_err(|_| anyhow!("the watcher panicked"))?;
    assert_eq!(unfinished, Vec::<PathBuf>::new());
    Ok(())
}

#[tokio::test]
async fn a_store_directory_that_lost_its_shared_bits_is_repaired_by_the_next_save() -> Result<()> {
    let fixture = Fixture::new()?;
    let response = fixture.upload("k-1", TAR).await;
    assert!(published(&response), "{response:?}");
    let scope = scope_of(&fixture)?;
    let owner = scope
        .parent()
        .ok_or_else(|| anyhow!("a scope has no owner directory"))?;
    let broken = [fixture.root(), owner.to_owned(), scope.clone()];
    for directory in &broken {
        fs::set_permissions(directory, fs::Permissions::from_mode(PRIVATE))?;
    }
    let response = fixture.upload("k-2", TAR).await;
    assert!(published(&response), "{response:?}");
    let modes: Vec<_> = broken
        .iter()
        .map(|directory| mode_of(directory))
        .collect::<Result<_>>()?;
    assert_eq!(modes, [0o755; 3]);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn first_saves_that_race_to_create_the_store_directories_all_publish() -> Result<()> {
    const PUBLISHERS: usize = 16;
    let fixture = Fixture::new()?;
    let mut tasks = Vec::new();
    for round in 0..PUBLISHERS {
        let broker = fixture.broker.clone();
        tasks.push(tokio::spawn(async move {
            super::tests::upload(&broker, &format!("k-{round}"), TAR).await
        }));
    }
    let mut answers = Vec::new();
    for task in tasks {
        answers.push(task.await?);
    }
    let failed: Vec<_> = answers.iter().filter(|answer| !published(answer)).collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(fixture.incoming_names(), Vec::<String>::new());
    Ok(())
}
