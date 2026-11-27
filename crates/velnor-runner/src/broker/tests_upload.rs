// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::future::Future as _;
use std::io::{ErrorKind, Write as _};
use std::path::Path;
use std::task::{Context, Poll, Waker};

use anyhow::{Result, anyhow};
use rustix::io::Errno;

use super::cache::MAX_DATA_BYTES;
use super::proto::{Denied, Response};
use super::safe_fs::uninterrupted;
use super::tests::{BRANCH, FORK, Fixture, TAR, begin, failed, published, version, write_all};
use super::tests_entry::{BOUND, wait_until};

const MIB: u64 = 1 << 20;

fn limited() -> Result<Fixture> {
    Fixture::with(BRANCH, |broker| broker.with_max_data(MIB))
}

fn stream_of(size: u64) -> Vec<u8> {
    (0..size)
        .map(|index| u8::try_from(index % 251).unwrap_or(0))
        .collect()
}

fn files_under(path: &Path) -> usize {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|item| match item.metadata() {
            Ok(meta) if meta.is_dir() => files_under(&item.path()),
            _ => 1,
        })
        .sum()
}

async fn clean(fixture: &Fixture) -> Result<()> {
    wait_until("incoming/ and aside/ to hold no files", || {
        files_under(&fixture.root().join("incoming")) == 0
            && files_under(&fixture.root().join("aside")) == 0
    })
    .await
}

#[test]
fn the_production_limit_is_the_toolkit_ten_gibibytes() {
    assert_eq!(MAX_DATA_BYTES, 10 * 1024 * 1024 * 1024);
}

#[tokio::test]
async fn a_stream_of_exactly_the_limit_is_published_and_one_more_byte_is_refused() -> Result<()> {
    let fixture = limited()?;
    let exact = stream_of(MIB);
    let response = fixture.upload("k-exact", &exact).await;
    assert!(published(&response), "{response:?}");
    assert_eq!(fs::read(fixture.entry("k-exact")?.join("data.tar"))?, exact);
    clean(&fixture).await?;

    let mut stream = begin(&fixture.broker, "k-over").await?;
    let mut writer = stream.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    let (sent, broken) = tokio::task::spawn_blocking(move || {
        let chunk = vec![7_u8; 64 << 10];
        let mut sent = 0_u64;
        for _ in 0..1024 {
            if let Err(error) = writer.write_all(&chunk) {
                return (sent, Some(error.kind()));
            }
            sent += chunk.len() as u64;
        }
        (sent, None)
    })
    .await?;
    assert_eq!(broken, Some(ErrorKind::BrokenPipe));
    assert!(sent > MIB);
    let response = stream.seal(sent).await;
    failed(&response, "is over the 10GB limit, not saving cache.");
    assert!(!fixture.entry("k-over")?.exists());
    clean(&fixture).await
}

#[tokio::test]
async fn an_upload_is_cut_off_at_the_cache_budget_when_the_budget_is_below_the_limit() -> Result<()>
{
    const SPAN: u64 = 16 * MIB;
    let fixture = Fixture::with(BRANCH, |broker| {
        broker.with_max_data(SPAN).with_budget(1000)
    })?;
    let mut stream = begin(&fixture.broker, "k-over").await?;
    let mut writer = stream.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    let (sent, broken) = tokio::task::spawn_blocking(move || {
        let chunk = vec![7_u8; 64 << 10];
        let mut sent = 0_u64;
        for _ in 0..1024 {
            if let Err(error) = writer.write_all(&chunk) {
                return (sent, Some(error.kind()));
            }
            sent += chunk.len() as u64;
        }
        (sent, None)
    })
    .await?;
    assert_eq!(broken, Some(ErrorKind::BrokenPipe));
    assert!(sent < SPAN, "{sent} bytes were accepted");
    let response = stream.seal(sent).await;
    failed(&response, "does not fit in the 1000 byte cache budget");
    assert!(!fixture.entry("k-over")?.exists());
    clean(&fixture).await?;
    let response = fixture.upload("k-exact", &stream_of(1000)).await;
    assert!(published(&response), "{response:?}");
    clean(&fixture).await
}

#[tokio::test]
async fn a_seal_count_that_differs_from_the_stream_fails_and_leaves_things_clean() -> Result<()> {
    let fixture = Fixture::new()?;
    let count = TAR.len() as u64;
    for (round, sealed) in [count + 1, count - 1].into_iter().enumerate() {
        let mut stream = begin(&fixture.broker, &format!("k-{round}")).await?;
        write_all(&mut stream, TAR.to_vec()).await?;
        failed(&stream.seal(sealed).await, "sealed");
        clean(&fixture).await?;
    }
    let mut stream = begin(&fixture.broker, "k-before").await?;
    let writer = stream.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    let mut sealing = Box::pin(stream.seal(count - 1));
    let polled = sealing
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()));
    assert!(matches!(polled, Poll::Pending));
    tokio::task::spawn_blocking(move || {
        let mut writer = writer;
        writer.write_all(TAR)
    })
    .await??;
    failed(&sealing.await, "more than the 6 bytes");
    fixture.assert_nothing_published();
    clean(&fixture).await
}

#[tokio::test]
async fn a_hung_up_upload_publishes_nothing_and_the_same_key_then_publishes() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut stream = begin(&fixture.broker, "k-1").await?;
    write_all(&mut stream, TAR.to_vec()).await?;
    wait_until("the pump to finish the stream", || {
        stream
            .slot
            .as_ref()
            .is_some_and(super::upload::Session::pump_finished)
    })
    .await?;
    drop(stream);
    clean(&fixture).await?;
    fixture.assert_nothing_published();
    let response = fixture.upload("k-1", TAR).await;
    assert!(published(&response), "{response:?}");

    let mut open = begin(&fixture.broker, "k-2").await?;
    let mut writer = open.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    writer.write_all(b"partial")?;
    drop(open);
    let ended = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        while started.elapsed() < BOUND {
            if writer.write_all(b"x").is_err() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        false
    })
    .await?;
    assert!(ended, "the writer of a hung-up upload must get EPIPE");
    clean(&fixture).await?;
    assert!(!fixture.entry("k-2")?.exists());
    Ok(())
}

#[tokio::test]
async fn a_stalled_upload_holds_no_scope_lock() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut stalled = begin(&fixture.broker, "k-1").await?;
    let mut writer = stalled.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    writer.write_all(b"part")?;
    let response = tokio::time::timeout(BOUND, fixture.upload("k-1", TAR)).await?;
    assert!(published(&response), "{response:?}");
    writer.write_all(b"ial")?;
    drop(writer);
    assert_eq!(stalled.seal(7).await, Response::AlreadyExists);
    assert_eq!(fs::read(fixture.entry("k-1")?.join("data.tar"))?, TAR);

    let mut aborted = begin(&fixture.broker, "k-2").await?;
    let mut writer = aborted.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    writer.write_all(b"partial")?;
    assert_eq!(
        aborted.abort().await,
        Response::Done {
            notices: Vec::new()
        }
    );
    clean(&fixture).await?;
    assert!(!fixture.entry("k-2")?.exists());
    Ok(())
}

#[tokio::test]
async fn a_fork_is_denied_at_the_broker_before_ready() -> Result<()> {
    let fixture = Fixture::with(FORK, |broker| broker)?;
    let refused = fixture.broker.begin_cache("k-1", &version()).await;
    assert!(matches!(
        refused,
        Err(Response::Denied(Denied::ForkPullRequest))
    ));
    assert!(!fixture.incoming().exists());
    assert!(!fixture.root().join("cache").exists());
    Ok(())
}

#[tokio::test]
async fn a_writer_kept_after_the_seal_gets_epipe_and_the_published_bytes_stay() -> Result<()> {
    let fixture = Fixture::new()?;
    let mut stream = begin(&fixture.broker, "k-1").await?;
    let mut writer = stream.take_writer().ok_or_else(|| anyhow!("pipe"))?;
    let mut kept = writer.try_clone()?;
    writer.write_all(TAR)?;
    drop(writer);
    let response = tokio::time::timeout(BOUND, stream.seal(TAR.len() as u64)).await?;
    assert!(published(&response), "{response:?}");
    let error = kept.write_all(b"corrupt").err().map(|error| error.kind());
    assert_eq!(error, Some(ErrorKind::BrokenPipe));
    assert_eq!(fs::read(fixture.entry("k-1")?.join("data.tar"))?, TAR);
    Ok(())
}

#[test]
fn an_interrupted_call_is_retried_until_it_answers() {
    let mut calls = 0;
    let answer = uninterrupted(|| {
        calls += 1;
        if calls < 3 {
            Err(Errno::INTR)
        } else {
            Ok(calls)
        }
    });
    assert_eq!(answer, Ok(3));
}
