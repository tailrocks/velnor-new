// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use anyhow::Result;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use super::artifact_support::{Env, JOB_RUN, OLD_JOB, run_job, run_job_env, write_file, zip_of};
use super::exec_support::{Ran, Scratch};
use super::fake_results::FakeResults;
use super::support::Reply;

const BASE_MS: &str = "500";
const BASE: Duration = Duration::from_millis(500);

fn upload_to(url: &str, base_ms: &str) -> Result<Ran> {
    upload_with_env(url, &[("VELNOR_ARTIFACT_RPC_BASE_MS", base_ms)])
}

fn upload_with_env(url: &str, process: &[(&str, &str)]) -> Result<Ran> {
    let scratch = Scratch::new()?;
    write_file(&scratch, "a.txt", b"a")?;
    run_job_env(
        &scratch,
        "artifact_upload",
        url,
        &[("name", "bundle"), ("path", "a.txt")],
        Env {
            process,
            ..Env::default()
        },
    )
}

async fn upload_after(replies: Vec<Reply>, base_ms: &str) -> Result<(FakeResults, Ran)> {
    let fake = FakeResults::start().await?;
    fake.script("CreateArtifact", replies)?;
    let ran = upload_to(&fake.url(), base_ms)?;
    Ok((fake, ran))
}

const QUOTA_LINE: &str = "::error::Failed to CreateArtifact: Artifact storage quota has been hit. Unable to upload any new artifacts.%0AMore info on storage limits: https://docs.github.com/en/billing/managing-billing-for-github-actions/about-billing-for-github-actions#calculating-minute-and-storage-spending";
const NETWORK_LINE: &str = "::error::Failed to CreateArtifact: Unable to make request: ECONNREFUSED%0AIf you are using self-hosted runners, please make sure your runner has access to all GitHub endpoints: https://docs.github.com/en/actions/hosting-your-own-runners/managing-self-hosted-runners/about-self-hosted-runners#communication-between-self-hosted-runners-and-github";

fn assert_gap_at_least(fake: &FakeResults, least: Duration) -> Result<()> {
    let arrivals = fake.arrivals("CreateArtifact")?;
    let gap = arrivals[1].duration_since(arrivals[0]);
    assert!(gap >= least, "{gap:?}");
    Ok(())
}

fn assert_error_line(ran: &Ran, expected: &str) {
    let lines: Vec<&str> = ran
        .stdout
        .lines()
        .filter(|line| line.starts_with("::error::"))
        .collect();
    assert_eq!(lines, [expected], "{}", ran.stdout);
}

fn error_body(status: u16, message: &str) -> Reply {
    Reply::json(status, &json!({ "code": "unavailable", "msg": message }))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retries_a_503_after_the_base_interval_and_ignores_retry_after() -> Result<()> {
    let reply = error_body(503, "down").with_header("retry-after", "200");
    let (fake, ran) = upload_after(vec![reply], BASE_MS).await?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 2);
    assert_gap_at_least(&fake, BASE)?;
    assert!(
        ran.has_line(
            "Attempt 1 of 5 failed with error: Failed request: (503) Service Unavailable: down. Retrying request in 500 ms..."
        ),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retries_a_body_that_is_not_json_whatever_the_status() -> Result<()> {
    let (fake, ran) =
        upload_after(vec![Reply::text(400, "<html>bad gateway</html>")], BASE_MS).await?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 2);
    assert_gap_at_least(&fake, BASE)?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn honours_retry_after_on_a_429() -> Result<()> {
    let reply = error_body(429, "slow down").with_header("retry-after", "1");
    let (fake, ran) = upload_after(vec![reply], "200").await?;
    assert_eq!(ran.code, Some(0), "{}", ran.stdout);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 2);
    assert_gap_at_least(&fake, Duration::from_secs(1))?;
    assert!(
        ran.has_line(
            "Attempt 1 of 5 failed with error: Failed request: (429) Too Many Requests: slow down. Retrying request in 1000 ms..."
        ),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn does_not_retry_a_status_outside_the_toolkit_list() -> Result<()> {
    let (fake, ran) = upload_after(vec![error_body(501, "no")], BASE_MS).await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(
        &ran,
        "::error::Failed to CreateArtifact: Received non-retryable error: Failed request: (501) Not Implemented: no",
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fails_at_once_when_retry_after_exceeds_the_total_wait() -> Result<()> {
    let reply = error_body(429, "slow down").with_header("retry-after", "200");
    let (fake, ran) = upload_after(vec![reply], BASE_MS).await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(
        &ran,
        "::error::Failed to CreateArtifact: Retry wait of 200000 ms would exceed the maximum total retry wait of 120000 ms: Failed request: (429) Too Many Requests: slow down",
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_oversized_retry_after_after_a_retry_fails_the_step_without_a_panic() -> Result<()> {
    let slow = error_body(429, "slow down").with_header("retry-after", "18446744073709551615");
    let (fake, ran) = upload_after(vec![error_body(503, "down"), slow], "1000").await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(
        &ran,
        "::error::Failed to CreateArtifact: Retry wait of 18446744073709551615000 ms would exceed the maximum total retry wait of 120000 ms: Failed request: (429) Too Many Requests: slow down",
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 2);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_after_is_honoured_when_the_throttling_body_is_not_json() -> Result<()> {
    let reply = Reply::text(429, "slow down").with_header("retry-after", "200");
    let (fake, ran) = upload_after(vec![reply], BASE_MS).await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout.contains(
            "::error::Failed to CreateArtifact: Retry wait of 200000 ms would exceed the maximum total retry wait of 120000 ms: "
        ),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_after_survives_a_body_that_cannot_be_read() -> Result<()> {
    let reply = error_body(429, "slow down")
        .with_header("retry-after", "200")
        .half_body();
    let fake = FakeResults::start().await?;
    fake.script("CreateArtifact", vec![reply])?;
    let ran = upload_with_env(
        &fake.url(),
        &[
            ("VELNOR_ARTIFACT_RPC_BASE_MS", BASE_MS),
            ("VELNOR_ARTIFACT_CALL_TIMEOUT_MS", "300"),
        ],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert!(
        ran.stdout.contains(
            "::error::Failed to CreateArtifact: Retry wait of 200000 ms would exceed the maximum total retry wait of 120000 ms: "
        ),
        "{}",
        ran.stdout
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_quota_error_is_not_retried() -> Result<()> {
    let (fake, ran) = upload_after(vec![error_body(429, "insufficient usage")], BASE_MS).await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(&ran, QUOTA_LINE);
    assert_eq!(fake.calls("CreateArtifact")?.len(), 1);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_connection_fails_at_once_with_the_toolkit_network_message() -> Result<()> {
    let ran = upload_to("http://127.0.0.1:1", "3000")?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(&ran, NETWORK_LINE);
    assert!(
        !ran.stdout.lines().any(|line| line.starts_with("Attempt ")),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_retry_after_beyond_the_integer_range_fails_at_once_on_the_budget() -> Result<()> {
    let slow = error_body(429, "slow down").with_header("retry-after", "99999999999999999999");
    let (fake, ran) = upload_after(vec![slow], BASE_MS).await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(
        &ran,
        "::error::Failed to CreateArtifact: Retry wait of 18446744073709551615000 ms would exceed the maximum total retry wait of 120000 ms: Failed request: (429) Too Many Requests: slow down",
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), 1);
    Ok(())
}

const ATTEMPTS: usize = 5;
const CLIENT_HELLO_IDLE: Duration = Duration::from_millis(100);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retry_exhaustion_prints_the_toolkit_text() -> Result<()> {
    let replies = (0..ATTEMPTS).map(|_| error_body(503, "down")).collect();
    let (fake, ran) = upload_after(replies, "20").await?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(
        &ran,
        "::error::Failed to CreateArtifact: Failed to make request after 5 attempts: Failed request: (503) Service Unavailable: down",
    );
    assert_eq!(fake.calls("CreateArtifact")?.len(), ATTEMPTS);
    let notes: Vec<&str> = ran
        .stdout
        .lines()
        .filter(|line| line.starts_with("Attempt "))
        .collect();
    assert_eq!(notes.len(), ATTEMPTS - 1, "{}", ran.stdout);
    for (index, note) in notes.iter().enumerate() {
        let head = format!(
            "Attempt {} of 5 failed with error: Failed request: (503) Service Unavailable: down. Retrying request in ",
            index + 1
        );
        let wait = note
            .strip_prefix(&head)
            .and_then(|rest| rest.strip_suffix(" ms..."))
            .and_then(|wait| wait.parse::<u64>().ok());
        assert!(wait.is_some(), "{note}");
        if index == 0 {
            assert_eq!(wait, Some(20), "{note}");
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refused_signed_url_prints_the_toolkit_download_exhaustion_text() -> Result<()> {
    let fake = FakeResults::start().await?;
    fake.seed("bundle", JOB_RUN, OLD_JOB, zip_of(&[("a.txt", b"a")])?)?;
    fake.script(
        "GetSignedArtifactURL",
        vec![Reply::json(
            200,
            &json!({ "signed_url": "http://127.0.0.1:1/blob?sig=x" }),
        )],
    )?;
    let scratch = Scratch::new()?;
    let ran = run_job(
        &scratch,
        "artifact_download",
        &fake.url(),
        &[("pattern", "bundle"), ("path", "restored")],
    )?;
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_error_line(
        &ran,
        "::error::Unable to download artifact(s): Unable to download and extract artifact: Artifact download failed after 5 retries.",
    );
    assert!(!ran.stdout.contains("Attempt "), "{}", ran.stdout);
    Ok(())
}

async fn plain_listener(
    hold: bool,
) -> Result<(u16, Arc<AtomicUsize>, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let accepted = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&accepted);
    let task = tokio::spawn(async move {
        let mut open = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            if hold {
                open.push(stream);
            } else {
                let mut stream = stream;
                let mut chunk = [0u8; 4096];
                while let Ok(Ok(read)) =
                    tokio::time::timeout(CLIENT_HELLO_IDLE, stream.read(&mut chunk)).await
                {
                    if read == 0 {
                        break;
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            }
        }
    });
    Ok((port, accepted, task))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stalled_response_is_retried_not_reported_as_a_network_error() -> Result<()> {
    let (port, accepted, task) = plain_listener(true).await?;
    let ran = upload_with_env(
        &format!("http://127.0.0.1:{port}"),
        &[
            ("VELNOR_ARTIFACT_RPC_BASE_MS", "20"),
            ("VELNOR_ARTIFACT_CALL_TIMEOUT_MS", "300"),
        ],
    )?;
    task.abort();
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_eq!(accepted.load(Ordering::SeqCst), ATTEMPTS, "{}", ran.stdout);
    assert!(
        !ran.stdout.contains("Unable to make request"),
        "{}",
        ran.stdout
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tls_handshake_failure_is_retried_not_reported_as_a_network_error() -> Result<()> {
    let (port, accepted, task) = plain_listener(false).await?;
    let ran = upload_with_env(
        &format!("https://127.0.0.1:{port}"),
        &[
            ("VELNOR_ARTIFACT_RPC_BASE_MS", "20"),
            ("VELNOR_ARTIFACT_CALL_TIMEOUT_MS", "300"),
        ],
    )?;
    task.abort();
    assert_eq!(ran.code, Some(1), "{}", ran.stdout);
    assert_eq!(accepted.load(Ordering::SeqCst), ATTEMPTS, "{}", ran.stdout);
    assert!(
        !ran.stdout.contains("Unable to make request"),
        "{}",
        ran.stdout
    );
    Ok(())
}
