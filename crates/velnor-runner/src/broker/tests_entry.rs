// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::{self, File, OpenOptions};
use std::io::Write as _;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use rustix::fs::{FlockOperation, flock};

use super::cache::{CacheOwner, MARKER, scope_dir, scope_hash};
use super::proto::Response;
use super::publish::Target;
use super::tests::{BRANCH, Fixture, TAR, begin, failed, published, upload, version, write_all};

pub(super) const BOUND: Duration = Duration::from_secs(30);

pub(super) fn scope_name(fixture: &Fixture) -> Result<String> {
    let scope = fixture
        .facts
        .cache_write_scope()
        .map_err(|denied| anyhow!("{denied}"))?;
    Ok(scope_hash(&CacheOwner::of(&fixture.facts), &scope))
}

pub(super) fn hold_the_lock(fixture: &Fixture) -> Result<File> {
    let locks = fixture.root().join("locks");
    fs::create_dir_all(&locks)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(locks.join(scope_name(fixture)?))?;
    flock(&file, FlockOperation::NonBlockingLockExclusive)?;
    Ok(file)
}

pub(super) async fn wait_until(what: &str, mut ready: impl FnMut() -> bool) -> Result<()> {
    let started = Instant::now();
    while !ready() {
        if started.elapsed() > BOUND {
            bail!("timed out waiting for {what}");
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    Ok(())
}

#[tokio::test]
async fn a_crashed_leftover_is_replaced_by_the_published_entry_and_leaves_nothing_behind()
-> Result<()> {
    let fixture = Fixture::new()?;
    let leftover = fixture.entry("k-1")?;
    fs::create_dir_all(&leftover)?;
    fs::write(leftover.join("junk"), b"x")?;
    let response = fixture.upload("k-1", TAR).await;
    assert!(published(&response), "{response:?}");
    assert_eq!(fs::read(fixture.entry("k-1")?.join("data.tar"))?, TAR);
    assert!(!fixture.entry("k-1")?.join("junk").exists());
    let junk: Vec<_> = fixture
        .paths_in_the_store()
        .into_iter()
        .filter(|path| path.file_name().is_some_and(|name| name == "junk"))
        .collect();
    assert_eq!(junk, Vec::<PathBuf>::new());
    Ok(())
}

#[tokio::test]
async fn a_published_entry_is_read_only_and_holds_exactly_what_was_streamed() -> Result<()> {
    let fixture = Fixture::new()?;
    let response = fixture.upload("k-1", TAR).await;
    assert!(published(&response), "{response:?}");
    let entry = fixture.entry("k-1")?;
    assert_eq!(fs::metadata(&entry)?.mode() & 0o7777, 0o555);
    assert_eq!(fs::metadata(entry.join("data.tar"))?.mode() & 0o7777, 0o444);
    assert_eq!(fs::metadata(entry.join("data.tar"))?.nlink(), 1);
    assert_eq!(fs::read(entry.join("data.tar"))?, TAR);
    assert!(fs::symlink_metadata(entry.join(MARKER))?.is_file());
    assert_eq!(fixture.incoming_names(), Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn an_incoming_upload_is_enterable_but_sits_in_a_private_directory_while_it_streams()
-> Result<()> {
    let fixture = Fixture::new()?;
    let mut stream = begin(&fixture.broker, "k-1").await?;
    let mut writer = stream.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    writer.write_all(b"partial")?;
    let names = fixture.incoming_names();
    assert_eq!(names.len(), 1);
    let directory = fixture.incoming().join(&names[0]);
    assert_eq!(fs::metadata(&directory)?.mode() & 0o7777, 0o755);
    assert_eq!(fs::metadata(fixture.incoming())?.mode() & 0o7777, 0o700);
    assert_eq!(
        fs::metadata(directory.join("data.tar"))?.mode() & 0o7777,
        0o600
    );
    drop(writer);
    stream.abort().await;
    Ok(())
}

#[tokio::test]
async fn a_lock_held_on_the_scope_directory_does_not_stall_a_publish() -> Result<()> {
    let fixture = Fixture::new()?;
    let scope = scope_dir(
        &fixture.root(),
        &CacheOwner::of(&fixture.facts),
        &fixture
            .facts
            .cache_write_scope()
            .map_err(|denied| anyhow!("{denied}"))?,
    );
    fs::create_dir_all(&scope)?;
    let held = File::open(&scope)?;
    flock(&held, FlockOperation::NonBlockingLockExclusive)?;
    let broker = fixture.broker.clone();
    let response = tokio::time::timeout(
        BOUND,
        tokio::spawn(async move { upload(&broker, "k-1", TAR).await }),
    )
    .await??;
    assert!(published(&response), "{response:?}");
    Ok(())
}

#[tokio::test]
async fn a_publish_gives_up_at_the_lock_deadline() -> Result<()> {
    let fixture = Fixture::with(BRANCH, |broker| {
        broker.with_lock_wait(Duration::from_millis(300))
    })?;
    let _lock = hold_the_lock(&fixture)?;
    let started = Instant::now();
    let response = tokio::time::timeout(BOUND, fixture.upload("k-1", TAR)).await?;
    failed(&response, "timed out waiting for the cache scope lock");
    assert!(started.elapsed() < BOUND);
    fixture.assert_nothing_published();
    assert_eq!(fixture.incoming_names(), Vec::<String>::new());
    Ok(())
}

#[tokio::test]
async fn an_abandoned_publish_stops_waiting_for_the_lock() -> Result<()> {
    let fixture = Fixture::new()?;
    let _lock = hold_the_lock(&fixture)?;
    let mut upload = begin(&fixture.broker, "k-1").await?;
    let count = write_all(&mut upload, TAR.to_vec()).await?;
    let abandoned = tokio::time::timeout(Duration::from_millis(200), upload.seal(count));
    assert!(abandoned.await.is_err());
    wait_until("the abandoned publish to clean its upload", || {
        fixture.incoming_names().is_empty()
    })
    .await?;
    fixture.assert_nothing_published();
    Ok(())
}

#[test]
fn a_publish_abandoned_before_the_rename_publishes_nothing() -> Result<()> {
    let fixture = Fixture::new()?;
    let layout = &fixture.broker.layout;
    let name = layout.next_name();
    let (upload, mut data) = layout.create_incoming(&name)?;
    data.write_all(TAR)?;
    let target = Target {
        owner: CacheOwner::of(&fixture.facts),
        scope: fixture
            .facts
            .cache_write_scope()
            .map_err(|denied| anyhow!("{denied}"))?,
        key: "k-1".to_owned(),
        version: version(),
    };
    let abandoned = AtomicBool::new(true);
    let response = super::publish::publish(layout, &target, &upload, &data, &abandoned);
    failed(&response, "abandoned");
    fixture.assert_nothing_published();
    fixture.assert_no_leftover();
    Ok(())
}

#[tokio::test]
async fn a_key_is_measured_in_utf16_units_like_the_cache_action() -> Result<()> {
    let fixture = Fixture::new()?;
    for (key, accepted) in [
        ("é".repeat(300), true),
        ("😀".repeat(256), true),
        ("😀".repeat(257), false),
    ] {
        let response = fixture.upload(&key, TAR).await;
        assert_eq!(
            published(&response),
            accepted,
            "{} UTF-16 units: {response:?}",
            key.encode_utf16().count()
        );
    }
    Ok(())
}

#[tokio::test]
async fn a_key_over_the_limit_is_refused_before_a_pipe_is_handed_out() -> Result<()> {
    let fixture = Fixture::new()?;
    let refused = fixture
        .broker
        .begin_cache(&"k".repeat(513), &version())
        .await;
    assert!(matches!(refused, Err(Response::Failed { .. })));
    assert!(!fixture.incoming().exists());
    Ok(())
}
