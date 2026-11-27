// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File};
use std::num::NonZeroU64;
use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Result;
use rustix::fs::{FlockOperation, flock};
use velnor_cache_rules::DEFAULT_CACHE_BUDGET_BYTES;

use super::Broker;
use super::cache::{ACCESSED, MARKER};
use super::layout::Clock;
use super::proto::{Request, Response};
use super::publish::SYNCED;
use super::tests::{BRANCH, Fixture, facts_of, failed, published, upload, version};
use super::tests_entry::BOUND;

const DAY: u64 = 24 * 60 * 60;
const START: u64 = 1_800_000_000;
const BEFORE_THE_FILES: u64 = 1_000_000_000;

fn clock_of(now: &Arc<AtomicU64>) -> Clock {
    let reading = now.clone();
    Arc::new(move || reading.load(Ordering::SeqCst))
}

fn clock() -> (Arc<AtomicU64>, Clock) {
    let now = Arc::new(AtomicU64::new(START));
    let source = clock_of(&now);
    (now, source)
}

fn at(now: &AtomicU64, seconds: u64) {
    now.store(START + seconds, Ordering::SeqCst);
}

fn timed(budget: u64) -> Result<(Fixture, Arc<AtomicU64>)> {
    let (now, source) = clock();
    let fixture = Fixture::with(BRANCH, |broker| {
        broker.with_budget(budget).with_clock(source)
    })?;
    Ok((fixture, now))
}

fn present(fixture: &Fixture, key: &str) -> Result<bool> {
    Ok(fixture.entry(key)?.join(MARKER).is_file())
}

async fn touch(fixture: &Fixture, key: &str) -> Response {
    touch_scope(&fixture.broker, "refs/heads/main", key).await
}

async fn touch_scope(broker: &Broker, scope: &str, key: &str) -> Response {
    broker
        .handle(
            &mut None,
            Request::TouchCache {
                scope: scope.to_owned(),
                key: key.to_owned(),
                version: version(),
            },
        )
        .await
        .response
}

fn files_below(directory: &Path) -> usize {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .map(|item| {
            let path = item.path();
            if fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
                files_below(&path)
            } else {
                1
            }
        })
        .sum()
}

fn bytes(size: usize) -> Vec<u8> {
    vec![b'x'; size]
}

#[tokio::test]
async fn the_least_recently_used_entry_is_evicted_first_and_a_restore_hit_renews_it() -> Result<()>
{
    let (fixture, now) = timed(350)?;
    for (second, key) in [(1, "k-a"), (2, "k-b"), (3, "k-c")] {
        at(&now, second);
        let response = fixture.upload(key, &bytes(100)).await;
        assert!(published(&response), "{key}: {response:?}");
    }
    at(&now, 4);
    let renewed = touch(&fixture, "k-a").await;
    assert!(matches!(renewed, Response::Done { .. }), "{renewed:?}");
    at(&now, 5);
    let response = fixture.upload("k-d", &bytes(100)).await;
    assert!(published(&response), "{response:?}");
    let kept = [
        present(&fixture, "k-a")?,
        present(&fixture, "k-b")?,
        present(&fixture, "k-c")?,
        present(&fixture, "k-d")?,
    ];
    assert_eq!(kept, [true, false, true, true]);
    assert_eq!(files_below(&fixture.root().join("aside")), 0);
    Ok(())
}

#[tokio::test]
async fn an_entry_unused_for_more_than_seven_days_is_evicted_and_seven_days_is_kept() -> Result<()>
{
    let (fixture, now) = timed(u64::MAX)?;
    let response = fixture.upload("k-old", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    at(&now, 7 * DAY);
    let response = fixture.upload("k-seven", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    assert!(present(&fixture, "k-old")?);
    at(&now, 7 * DAY + 1);
    let response = fixture.upload("k-late", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    assert_eq!(
        [
            present(&fixture, "k-old")?,
            present(&fixture, "k-seven")?,
            present(&fixture, "k-late")?,
        ],
        [false, true, true]
    );
    Ok(())
}

#[tokio::test]
async fn a_touch_restarts_the_seven_days() -> Result<()> {
    let (fixture, now) = timed(u64::MAX)?;
    let response = fixture.upload("k-a", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    at(&now, 6 * DAY);
    let renewed = touch(&fixture, "k-a").await;
    assert!(matches!(renewed, Response::Done { .. }), "{renewed:?}");
    at(&now, 8 * DAY);
    let response = fixture.upload("k-b", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    assert!(present(&fixture, "k-a")?);
    Ok(())
}

#[tokio::test]
async fn an_entry_that_cannot_fit_the_budget_is_refused_and_evicts_nothing() -> Result<()> {
    let (fixture, _) = timed(250)?;
    let response = fixture.upload("k-a", &bytes(100)).await;
    assert!(published(&response), "{response:?}");
    let response = fixture.upload("k-big", &bytes(300)).await;
    failed(&response, "does not fit in the 250 byte cache budget");
    assert!(present(&fixture, "k-a")?);
    assert!(!fixture.entry("k-big")?.exists());
    Ok(())
}

#[tokio::test]
async fn siblings_with_the_same_access_second_never_get_a_save_that_fits_refused() -> Result<()> {
    let now = Arc::new(AtomicU64::new(BEFORE_THE_FILES));
    let fixture = Fixture::with(BRANCH, |broker| {
        broker.with_budget(250).with_clock(clock_of(&now))
    })?;
    for index in 0..8 {
        let key = format!("k-{index}");
        let response = fixture.upload(&key, &bytes(100)).await;
        assert!(published(&response), "{key}: {response:?}");
        assert!(present(&fixture, &key)?, "{key}");
    }
    let kept = (0..8)
        .filter(|index| present(&fixture, &format!("k-{index}")).unwrap_or(false))
        .count();
    assert_eq!(kept, 2);
    Ok(())
}

#[tokio::test]
async fn an_entry_saved_after_the_clock_stepped_back_is_admitted() -> Result<()> {
    let (fixture, now) = timed(150)?;
    at(&now, 100);
    let response = fixture.upload("k-a", &bytes(100)).await;
    assert!(published(&response), "{response:?}");
    at(&now, 0);
    let response = fixture.upload("k-b", &bytes(100)).await;
    assert!(published(&response), "{response:?}");
    assert_eq!(
        [present(&fixture, "k-a")?, present(&fixture, "k-b")?],
        [false, true]
    );
    Ok(())
}

#[test]
fn a_broker_budgets_each_repository_at_the_default_cache_budget() -> Result<()> {
    let fixture = Fixture::new()?;
    assert_eq!(fixture.broker.layout.budget, DEFAULT_CACHE_BUDGET_BYTES);
    Ok(())
}

#[tokio::test]
async fn the_budget_counts_one_repository_and_never_evicts_another() -> Result<()> {
    let (fixture, now) = timed(350)?;
    let response = fixture.upload("k-a", &bytes(300)).await;
    assert!(published(&response), "{response:?}");
    let mut other = fixture.facts.clone();
    other.repository_id = NonZeroU64::new(4243).unwrap_or(NonZeroU64::MIN);
    let neighbour = Broker::new(other, fixture.root())
        .with_budget(350)
        .with_clock(clock_of(&now));
    let response = upload(&neighbour, "k-a", &bytes(300)).await;
    assert!(published(&response), "{response:?}");
    assert!(present(&fixture, "k-a")?);
    Ok(())
}

#[tokio::test]
async fn a_save_of_exactly_the_budget_is_published_and_evicts_the_older_entries() -> Result<()> {
    let (fixture, now) = timed(250)?;
    for (second, key) in [(0, "k-a"), (10, "k-b")] {
        at(&now, second);
        let response = fixture.upload(key, &bytes(100)).await;
        assert!(published(&response), "{key}: {response:?}");
    }
    at(&now, 20);
    let response = fixture.upload("k-c", &bytes(250)).await;
    assert!(published(&response), "{response:?}");
    assert_eq!(
        [
            present(&fixture, "k-a")?,
            present(&fixture, "k-b")?,
            present(&fixture, "k-c")?
        ],
        [false, false, true]
    );
    Ok(())
}

fn synced_count(directory: &Path) -> Result<usize> {
    let stat = fs::metadata(directory)?;
    let identity = (stat.dev(), stat.ino());
    let synced = SYNCED
        .lock()
        .map_err(|_| anyhow::anyhow!("the sync record is poisoned"))?;
    Ok(synced
        .iter()
        .filter(|seen| seen.identity == identity)
        .count())
}

#[tokio::test]
async fn an_eviction_syncs_the_scope_it_removed_an_entry_from_before_the_save_returns() -> Result<()>
{
    let (fixture, now) = timed(150)?;
    let mut elsewhere = facts_of(r#"{"kind": "branch", "branch": "other"}"#)?;
    elsewhere.repository_id = fixture.facts.repository_id;
    let writer = Broker::new(elsewhere, fixture.root())
        .with_budget(150)
        .with_clock(clock_of(&now));
    let response = upload(&writer, "k-a", &bytes(100)).await;
    assert!(published(&response), "{response:?}");
    let owner = super::cache::CacheOwner::of(&fixture.facts);
    let other = velnor_cache_rules::Scope("refs/heads/other".to_owned());
    let evicted = super::cache::entry_dir(&fixture.root(), &owner, &other, &version(), "k-a");
    let scope = evicted
        .parent()
        .ok_or_else(|| anyhow::anyhow!("an entry has no scope directory"))?;
    let before = synced_count(scope)?;
    at(&now, 10);
    let response = fixture.upload("k-b", &bytes(100)).await;
    assert!(published(&response), "{response:?}");
    assert!(!evicted.exists());
    assert_eq!(synced_count(scope)?, before + 1);
    Ok(())
}

#[tokio::test]
async fn a_publish_waits_for_the_store_wide_lock() -> Result<()> {
    let fixture = Fixture::with(BRANCH, |broker| {
        broker.with_lock_wait(Duration::from_millis(300))
    })?;
    let locks = fixture.root().join("locks");
    fs::create_dir_all(&locks)?;
    let held = File::create(locks.join("store"))?;
    flock(&held, FlockOperation::NonBlockingLockExclusive)?;
    let response = tokio::time::timeout(BOUND, fixture.upload("k-1", &bytes(10))).await?;
    failed(&response, "timed out waiting for the cache store lock");
    fixture.assert_nothing_published();
    drop(held);
    Ok(())
}

#[tokio::test]
async fn a_renewed_access_stamp_is_synced_before_the_touch_is_answered() -> Result<()> {
    let (fixture, now) = timed(u64::MAX)?;
    let response = fixture.upload("k-a", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    let stamp = fs::metadata(fixture.entry("k-a")?.join(ACCESSED))?;
    let identity = (stamp.dev(), stamp.ino());
    let synced = || -> Result<usize> {
        let seen = SYNCED
            .lock()
            .map_err(|_| anyhow::anyhow!("the sync record is poisoned"))?;
        Ok(seen
            .iter()
            .filter(|seen| seen.what.contains("access stamp") && seen.identity == identity)
            .count())
    };
    let before = synced()?;
    at(&now, DAY);
    let renewed = touch(&fixture, "k-a").await;
    assert!(matches!(renewed, Response::Done { .. }), "{renewed:?}");
    assert_eq!(synced()?, before + 1);
    Ok(())
}

#[tokio::test]
async fn a_job_cannot_renew_an_entry_of_a_scope_it_cannot_read() -> Result<()> {
    let (fixture, now) = timed(u64::MAX)?;
    let mut elsewhere = facts_of(r#"{"kind": "branch", "branch": "other"}"#)?;
    elsewhere.repository_id = fixture.facts.repository_id;
    let writer = Broker::new(elsewhere, fixture.root());
    let response = upload(&writer, "k-a", &bytes(10)).await;
    assert!(published(&response), "{response:?}");
    let stamp = |scope: &str| {
        let owner = super::cache::CacheOwner::of(&fixture.facts);
        let entry = super::cache::entry_dir(
            &fixture.root(),
            &owner,
            &velnor_cache_rules::Scope(scope.to_owned()),
            &version(),
            "k-a",
        );
        fs::metadata(entry.join(ACCESSED)).and_then(|meta| meta.modified())
    };
    let before = stamp("refs/heads/other")?;
    at(&now, 3 * DAY);
    let refused = touch_scope(&fixture.broker, "refs/heads/other", "k-a").await;
    failed(&refused, "cannot be read by this job");
    assert_eq!(stamp("refs/heads/other")?, before);
    Ok(())
}
