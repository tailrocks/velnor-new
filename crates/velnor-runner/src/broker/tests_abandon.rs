// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use rustix::fs::{FlockOperation, flock};

use super::cache::{CacheOwner, MARKER};
use super::incoming::Upload;
use super::publish::{Target, publish};
use super::tests::{BRANCH, Fixture, failed, published, version};
use super::tests_entry::{BOUND, scope_name};

const ENTRY: usize = 100;
const BUDGET: u64 = 150;

fn prepared(fixture: &Fixture) -> Result<(Arc<Upload>, File, Target)> {
    let layout = &fixture.broker.layout;
    let (upload, mut data) = layout.create_incoming(&layout.next_name())?;
    data.write_all(&[b'x'; ENTRY])?;
    let target = Target {
        owner: CacheOwner::of(&fixture.facts),
        scope: fixture
            .facts
            .cache_write_scope()
            .map_err(|denied| anyhow!("{denied}"))?,
        key: "k-new".to_owned(),
        version: version(),
    };
    Ok((upload, data, target))
}

fn open_lock(fixture: &Fixture, name: &str) -> Result<File> {
    let locks = fixture.root().join("locks");
    fs::create_dir_all(&locks)?;
    Ok(OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(locks.join(name))?)
}

#[test]
fn a_publish_abandoned_while_it_made_room_never_installs_its_entry() -> Result<()> {
    let tripped = Arc::new(AtomicBool::new(false));
    let trip = tripped.clone();
    let reads = AtomicUsize::new(0);
    let fixture = Fixture::with(BRANCH, |broker| {
        broker.with_clock(Arc::new(move || {
            if reads.fetch_add(1, Ordering::SeqCst) > 0 {
                trip.store(true, Ordering::SeqCst);
            }
            1_800_000_000
        }))
    })?;
    let (upload, data, target) = prepared(&fixture)?;
    let response = publish(&fixture.broker.layout, &target, &upload, &data, &tripped);
    failed(&response, "abandoned before it was committed");
    fixture.assert_nothing_published();
    fixture.assert_no_leftover();
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_publish_abandoned_while_it_waited_for_the_store_lock_evicts_nothing() -> Result<()> {
    let fixture = Fixture::with(BRANCH, |broker| broker.with_budget(BUDGET))?;
    let response = fixture.upload("k-old", &[b'x'; ENTRY]).await;
    assert!(published(&response), "{response:?}");
    let (upload, data, target) = prepared(&fixture)?;
    let abandoned = AtomicBool::new(false);
    let store = open_lock(&fixture, "store")?;
    flock(&store, FlockOperation::NonBlockingLockExclusive)?;
    let scope = open_lock(&fixture, &scope_name(&fixture)?)?;
    let response = std::thread::scope(|threads| {
        let publishing =
            threads.spawn(|| publish(&fixture.broker.layout, &target, &upload, &data, &abandoned));
        let started = Instant::now();
        while started.elapsed() < BOUND
            && flock(&scope, FlockOperation::NonBlockingLockExclusive).is_ok()
        {
            let _ = flock(&scope, FlockOperation::Unlock);
            std::thread::sleep(Duration::from_millis(2));
        }
        std::thread::sleep(Duration::from_millis(100));
        abandoned.store(true, Ordering::SeqCst);
        drop(store);
        publishing.join()
    });
    let response = response.map_err(|_| anyhow!("the publish thread panicked"))?;
    failed(&response, "abandoned");
    assert!(fixture.entry("k-old")?.join(MARKER).is_file());
    Ok(())
}
