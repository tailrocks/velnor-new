// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::artifact_support::{error_lines, spawn_job, write_file};
use super::exec_support::Scratch;
use super::fake_results::FakeResults;
use super::support::Reply;

const ATTEMPTS: usize = 5;
const BIG: usize = 24 << 20;
const FINALIZE_REFUSED: &str = "::error::Failed to FinalizeArtifact: Received non-retryable error: Failed request: (404) Not Found: fake results error";

fn noise() -> Vec<u8> {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    (0..BIG)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect()
}

fn declared_length(head: &str) -> Option<usize> {
    head.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse().ok())?
    })
}

async fn drain_slowly(mut stream: TcpStream, pause: Duration) -> Result<()> {
    let mut buffer = vec![0_u8; 512 * 1024];
    let (mut head, mut body, mut declared) = (Vec::new(), 0_usize, None);
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            return Ok(());
        }
        if declared.is_none() {
            head.extend_from_slice(&buffer[..read]);
            let Some(end) = head.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            declared = declared_length(&String::from_utf8_lossy(&head[..end]));
            body = head.len() - end - 4;
        } else {
            body += read;
        }
        if declared.is_some_and(|length| body >= length) {
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n")
                .await?;
            return Ok(());
        }
        tokio::time::sleep(pause).await;
    }
}

async fn upload_to(
    port: u16,
    window_ms: &str,
) -> Result<(super::artifact_support::Finished, FakeResults)> {
    let fake = FakeResults::start().await?;
    fake.script(
        "CreateArtifact",
        vec![Reply::json(
            200,
            &json!({
                "ok": true,
                "signed_upload_url": format!("http://127.0.0.1:{port}/blob?sig=x"),
            }),
        )],
    )?;
    let scratch = Scratch::new()?;
    write_file(&scratch, "big.bin", &noise())?;
    let env = [
        ("VELNOR_ARTIFACT_CALL_TIMEOUT_MS", window_ms),
        ("VELNOR_ARTIFACT_BLOB_BASE_MS", "1"),
    ];
    let finished = spawn_job(
        &scratch,
        "artifact_upload",
        &fake.url(),
        &[("name", "bundle"), ("path", "big.bin")],
        &env,
    )?
    .finish("the runner to finish the upload")?;
    Ok((finished, fake))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_that_stops_moving_is_given_up_on_after_one_idle_window() -> Result<()> {
    const WINDOW: Duration = Duration::from_millis(800);
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let arrivals = Arc::new(Mutex::new(Vec::<Instant>::new()));
    let seen = Arc::clone(&arrivals);
    let holder = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            if let Ok(mut all) = seen.lock() {
                all.push(Instant::now());
            }
            open.push(stream);
        }
    });
    let outcome = upload_to(port, "800").await;
    holder.abort();
    let (finished, _fake) = outcome?;
    assert_eq!(finished.code, Some(1), "{}", finished.stdout);
    assert_eq!(
        error_lines(&finished.stdout),
        ["::error::artifact blob PUT: request timed out"]
    );
    let arrivals = arrivals.lock().map_err(|_| anyhow::anyhow!("poisoned"))?;
    assert_eq!(arrivals.len(), ATTEMPTS, "{}", finished.stdout);
    for pair in arrivals.windows(2) {
        let gap = pair[1] - pair[0];
        assert!(gap >= WINDOW.mul_f64(0.9), "{gap:?}");
        assert!(gap < WINDOW.mul_f64(1.5), "{gap:?}");
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_upload_that_keeps_moving_slower_than_the_window_is_not_a_stall() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let accepted = Arc::new(Mutex::new(0_usize));
    let count = Arc::clone(&accepted);
    let serving = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            if let Ok(mut all) = count.lock() {
                *all += 1;
            }
            tokio::spawn(drain_slowly(stream, Duration::from_millis(50)));
        }
    });
    let started = Instant::now();
    let outcome = upload_to(port, "400").await;
    serving.abort();
    let (finished, _fake) = outcome?;
    assert_eq!(finished.code, Some(1), "{}", finished.stdout);
    assert_eq!(
        error_lines(&finished.stdout),
        [FINALIZE_REFUSED],
        "{}",
        finished.stdout
    );
    assert!(started.elapsed() > Duration::from_millis(800));
    assert_eq!(
        *accepted.lock().map_err(|_| anyhow::anyhow!("poisoned"))?,
        1,
        "{}",
        finished.stdout
    );
    Ok(())
}
