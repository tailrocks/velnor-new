// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use rustix::fs::{FlockOperation, flock};
use velnor_cache_rules::Scope;

use super::Broker;
use super::cache::{CacheOwner, MARKER, entry_dir, scope_dir, scope_hash};
use super::layout::Clock;
use super::publish::Target;
use super::tests::{BRANCH, Fixture, facts_of, published, upload, version};
use super::tests_entry::wait_until;

const START: u64 = 1_800_000_000;

fn clock() -> Clock {
    let now = Arc::new(AtomicU64::new(START));
    Arc::new(move || now.load(Ordering::SeqCst))
}

fn branch(name: &str) -> String {
    format!(r#"{{"kind": "branch", "branch": "{name}"}}"#)
}

fn broker_on(fixture: &Fixture, name: &str, budget: u64) -> Result<Broker> {
    let mut facts = facts_of(&branch(name))?;
    facts.repository_id = fixture.facts.repository_id;
    Ok(Broker::new(facts, fixture.root())
        .with_budget(budget)
        .with_clock(clock()))
}

fn fixed(fixture: &Fixture, branch: &str, key: &str) -> Result<()> {
    let marker = entry_dir(
        &fixture.root(),
        &CacheOwner::of(&fixture.facts),
        &Scope(format!("refs/heads/{branch}")),
        &version(),
        key,
    )
    .join(MARKER);
    File::open(marker)?.set_modified(UNIX_EPOCH + Duration::from_secs(START))?;
    Ok(())
}

fn present(fixture: &Fixture, branch: &str, key: &str) -> bool {
    entry_dir(
        &fixture.root(),
        &CacheOwner::of(&fixture.facts),
        &Scope(format!("refs/heads/{branch}")),
        &version(),
        key,
    )
    .join(MARKER)
    .is_file()
}

async fn saved(broker: &Broker, key: &str) -> Result<()> {
    let response = upload(broker, key, &[b'x'; 100]).await;
    anyhow::ensure!(published(&response), "{key}: {response:?}");
    Ok(())
}

fn lock_path(fixture: &Fixture, name: &str) -> PathBuf {
    let scope = Scope(format!("refs/heads/{name}"));
    let hash = scope_hash(&CacheOwner::of(&fixture.facts), &scope);
    fixture.root().join("locks").join(hash)
}

fn scope_path(fixture: &Fixture, name: &str) -> PathBuf {
    scope_dir(
        &fixture.root(),
        &CacheOwner::of(&fixture.facts),
        &Scope(format!("refs/heads/{name}")),
    )
}

#[tokio::test]
async fn an_eviction_tie_goes_to_the_smaller_key_not_the_smaller_stored_hash() -> Result<()> {
    let fixture = Fixture::with(BRANCH, |broker| broker.with_budget(250).with_clock(clock()))?;
    for key in ["k-a", "k-b"] {
        saved(&fixture.broker, key).await?;
        fixed(&fixture, "main", key)?;
    }
    saved(&fixture.broker, "k-c").await?;
    assert_eq!(
        [
            present(&fixture, "main", "k-a"),
            present(&fixture, "main", "k-b"),
            present(&fixture, "main", "k-c")
        ],
        [false, true, true]
    );
    Ok(())
}

#[tokio::test]
async fn an_eviction_tie_goes_to_the_smaller_scope_not_the_smaller_stored_hash() -> Result<()> {
    let fixture = Fixture::new()?;
    let owner = CacheOwner::of(&fixture.facts);
    let hash = |name: &str| scope_hash(&owner, &Scope(format!("refs/heads/{name}")));
    let names = ["alpha", "beta", "gamma", "delta", "omega", "sigma"];
    let (first, second) = names
        .iter()
        .flat_map(|first| names.iter().map(move |second| (*first, *second)))
        .find(|(first, second)| first < second && hash(first) > hash(second))
        .ok_or_else(|| anyhow!("no scope pair with reversed hashes"))?;
    let brokers = [
        broker_on(&fixture, first, 250)?,
        broker_on(&fixture, second, 250)?,
        broker_on(&fixture, "zeta", 250)?,
    ];
    for (broker, name) in brokers.iter().zip([first, second]) {
        saved(broker, "k-a").await?;
        fixed(&fixture, name, "k-a")?;
    }
    saved(&brokers[2], "k-a").await?;
    assert_eq!(
        [
            present(&fixture, first, "k-a"),
            present(&fixture, second, "k-a")
        ],
        [false, true]
    );
    Ok(())
}

#[tokio::test]
async fn a_scope_emptied_by_eviction_is_removed_with_its_lock_and_can_be_saved_to_again()
-> Result<()> {
    let fixture = Fixture::with(BRANCH, |broker| broker.with_budget(350).with_clock(clock()))?;
    let other = broker_on(&fixture, "other", 350)?;
    let bytes = [b'x'; 300];
    assert!(published(&upload(&fixture.broker, "k-a", &bytes).await));
    assert!(lock_path(&fixture, "main").exists());
    assert!(published(&upload(&other, "k-b", &bytes).await));
    assert!(!scope_path(&fixture, "main").exists());
    assert!(!lock_path(&fixture, "main").exists());
    assert!(published(
        &upload(&fixture.broker, "k-c", &bytes[..10]).await
    ));
    assert!(present(&fixture, "main", "k-c"));
    Ok(())
}

#[tokio::test]
async fn a_scope_whose_lock_is_held_is_never_removed_from_under_its_holder() -> Result<()> {
    let fixture = Fixture::with(BRANCH, |broker| broker.with_budget(350).with_clock(clock()))?;
    let other = broker_on(&fixture, "other", 350)?;
    let bytes = [b'x'; 300];
    assert!(published(&upload(&fixture.broker, "k-a", &bytes).await));
    let held = OpenOptions::new()
        .write(true)
        .open(lock_path(&fixture, "main"))?;
    flock(&held, FlockOperation::NonBlockingLockExclusive)?;
    assert!(published(&upload(&other, "k-b", &bytes).await));
    assert!(scope_path(&fixture, "main").is_dir());
    assert!(lock_path(&fixture, "main").exists());
    drop(held);
    Ok(())
}

#[tokio::test]
async fn a_lock_waiter_reopens_a_lock_file_that_was_unlinked_while_it_waited() -> Result<()> {
    let fixture = Fixture::new()?;
    let layout = fixture.broker.layout.clone();
    let target = Target {
        owner: CacheOwner::of(&fixture.facts),
        scope: Scope("refs/heads/main".to_owned()),
        key: "k-1".to_owned(),
        version: version(),
    };
    let abandoned = Arc::new(AtomicBool::new(false));
    let held = layout.lock(&target, &abandoned)?;
    let started = Arc::new(AtomicBool::new(false));
    let waiter = {
        let (layout, target, abandoned, started) = (
            layout.clone(),
            target.clone(),
            abandoned.clone(),
            started.clone(),
        );
        tokio::task::spawn_blocking(move || {
            started.store(true, Ordering::SeqCst);
            layout.lock(&target, &abandoned)
        })
    };
    wait_until("the waiter to start", || started.load(Ordering::SeqCst)).await?;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let path = lock_path(&fixture, "main");
    fs::remove_file(&path)?;
    drop(held);
    let acquired = waiter.await??;
    let named = fs::metadata(&path)?;
    let owned = File::from(acquired).metadata()?;
    assert_eq!((named.dev(), named.ino()), (owned.dev(), owned.ino()));
    Ok(())
}
