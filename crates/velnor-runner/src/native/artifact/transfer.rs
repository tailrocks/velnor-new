// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/protocol.rs

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::header::{CONTENT_LENGTH, CONTENT_TYPE};
use ring::digest::{Context as Sha256, SHA256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::watch;
use tokio::time::Instant;

use super::blob::{Fetched, raw_name};
use super::redirect::{MAX_REDIRECTS, target};
use super::retry::{Failure, Mode, attempts, blob};
use super::twirp::call_timeout;
use super::zip::hex;
use crate::protocol::http::{http_client, redacted_reqwest_error, validate_authenticated_url};

const REPLY_WAIT: Duration = Duration::from_secs(300);
const CONTENT_TYPE_ZIP: &str = "application/zip";

fn counted(
    file: tokio::fs::File,
    sent: watch::Sender<u64>,
) -> impl futures_util::Stream<Item = std::io::Result<Vec<u8>>> {
    futures_util::stream::unfold((file, sent), |(mut file, sent)| async move {
        let mut buffer = vec![0_u8; 64 * 1024];
        match file.read(&mut buffer).await {
            Ok(0) => None,
            Ok(read) => {
                buffer.truncate(read);
                sent.send_modify(|total| {
                    *total = total.saturating_add(u64::try_from(read).unwrap_or(u64::MAX));
                });
                Some((Ok(buffer), (file, sent)))
            }
            Err(error) => Some((Err(error), (file, sent))),
        }
    })
}

fn put_failure(error: &reqwest::Error) -> Failure {
    Failure::transient(anyhow::anyhow!(
        "artifact blob PUT: {}",
        redacted_reqwest_error(error)
    ))
}

fn put_timed_out() -> Failure {
    Failure::transient(anyhow::anyhow!("artifact blob PUT: request timed out"))
}

async fn watched(
    request: impl std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
    mut sent: watch::Receiver<u64>,
    size: u64,
) -> Result<reqwest::Response, Failure> {
    tokio::pin!(request);
    while *sent.borrow() < size {
        tokio::select! {
            outcome = &mut request => return outcome.map_err(|error| put_failure(&error)),
            moved = tokio::time::timeout(call_timeout(), sent.changed()) => match moved {
                Ok(Ok(())) => {}
                Ok(Err(_)) => break,
                Err(_) => return Err(put_timed_out()),
            },
        }
    }
    tokio::time::timeout(REPLY_WAIT, request)
        .await
        .map_err(|_| put_timed_out())?
        .map_err(|error| put_failure(&error))
}

pub(super) async fn put_blob(url: &str, path: &Path, size: u64) -> Result<()> {
    validate_authenticated_url(url).context("validate the artifact upload URL")?;
    attempts(blob(), Mode::BlobUpload, || async {
        let file = tokio::fs::File::open(path)
            .await
            .map_err(|error| Failure::fatal(anyhow::Error::new(error).context("open the zip")))?;
        let (sent, progress) = watch::channel(0_u64);
        let request = http_client()
            .map_err(Failure::fatal)?
            .put(url)
            .header(CONTENT_TYPE, CONTENT_TYPE_ZIP)
            .header(CONTENT_LENGTH, size)
            .header("x-ms-blob-type", "BlockBlob")
            .body(reqwest::Body::wrap_stream(counted(file, sent)))
            .send();
        let response = watched(request, progress, size).await?;
        let status = response.status();
        if status == reqwest::StatusCode::CREATED {
            return Ok(());
        }
        let error = anyhow::anyhow!("artifact blob PUT failed: status={status}");
        if status.as_u16() == 429 || status.is_server_error() {
            Err(Failure::transient(error))
        } else {
            Err(Failure::fatal(error))
        }
    })
    .await
}

async fn within_idle<T>(
    wait: impl std::future::Future<Output = Result<T, reqwest::Error>>,
    idle: Duration,
) -> Result<T, Failure> {
    match tokio::time::timeout(idle, wait).await {
        Ok(outcome) => outcome.map_err(|error| {
            Failure::transient(anyhow::anyhow!(
                "artifact blob GET: {}",
                redacted_reqwest_error(&error)
            ))
        }),
        Err(_) => Err(Failure::transient(anyhow::anyhow!(
            "artifact blob GET: request timed out"
        ))),
    }
}

async fn before<T>(
    deadline: Instant,
    wait: impl std::future::Future<Output = Result<T, Failure>>,
) -> Result<T, Failure> {
    tokio::time::timeout_at(deadline, wait)
        .await
        .unwrap_or_else(|_| {
            Err(Failure::transient(anyhow::anyhow!(
                "artifact blob GET: request timed out"
            )))
        })
}

async fn receive(
    response: &mut reqwest::Response,
    destination: &Path,
    limit: u64,
    idle: Duration,
) -> Result<String, Failure> {
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(destination)
        .await
        .map_err(|error| Failure::fatal(anyhow::Error::new(error).context("create the zip")))?;
    let mut hash = Sha256::new(&SHA256);
    let mut received = 0_u64;
    let mut deadline = Instant::now() + idle;
    loop {
        let next = before(deadline, async {
            response.chunk().await.map_err(|error| {
                Failure::transient(anyhow::anyhow!(
                    "artifact blob GET: {}",
                    redacted_reqwest_error(&error)
                ))
            })
        })
        .await?;
        let Some(chunk) = next else {
            break;
        };
        deadline = Instant::now() + idle;
        received += u64::try_from(chunk.len()).unwrap_or(u64::MAX);
        if received > limit {
            return Err(Failure::fatal(anyhow::anyhow!(
                "artifact is larger than the {limit}-byte download limit"
            )));
        }
        hash.update(&chunk);
        before(deadline, async {
            file.write_all(&chunk)
                .await
                .map_err(|error| Failure::fatal(anyhow::Error::new(error).context("write the zip")))
        })
        .await?;
    }
    before(deadline, async {
        file.flush()
            .await
            .map_err(|error| Failure::fatal(anyhow::Error::new(error).context("write the zip")))
    })
    .await?;
    Ok(hex(hash.finish().as_ref()))
}

async fn get(url: &str) -> Result<reqwest::Response, Failure> {
    let send = |target: String| async move {
        within_idle(
            http_client().map_err(Failure::fatal)?.get(target).send(),
            call_timeout(),
        )
        .await
    };
    let mut response = send(url.to_owned()).await?;
    for _ in 0..MAX_REDIRECTS {
        let Some(next) = target(&response).map_err(Failure::transient)? else {
            break;
        };
        response = send(next.into()).await?;
    }
    Ok(response)
}

pub(super) async fn fetch_blob(url: &str, destination: &Path, limit: u64) -> Result<Fetched> {
    validate_authenticated_url(url).context("validate the artifact download URL")?;
    attempts(blob(), Mode::BlobDownload, || async {
        let mut response = get(url).await?;
        let status = response.status();
        if status != reqwest::StatusCode::OK {
            let error = anyhow::anyhow!("artifact blob GET failed: status={status}");
            return Err(if status.as_u16() == 429 || status.is_server_error() {
                Failure::transient(error)
            } else {
                Failure::fatal(error)
            });
        }
        let raw_name = raw_name(url, response.headers()).map_err(Failure::fatal)?;
        let sha256 = receive(&mut response, destination, limit, call_timeout()).await?;
        Ok(Fetched { sha256, raw_name })
    })
    .await
    .map_err(|error| error.context("Unable to download and extract artifact"))
}

#[cfg(test)]
mod tests {
    use std::fs::{File, OpenOptions};
    use std::io::{Read, Write};
    use std::process::Command;
    use std::time::Duration;

    use anyhow::{Context, Result};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uuid::Uuid;

    use super::receive;
    use crate::native::artifact::retry::Failure;
    use crate::protocol::http::http_client;

    const IDLE: Duration = Duration::from_millis(300);
    const WINDOW: Duration = Duration::from_millis(1500);
    const LIMIT: u64 = 1 << 30;

    struct Directory(std::path::PathBuf);

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    async fn serve(parts: Vec<(usize, Duration)>) -> Result<String> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/blob", listener.local_addr()?);
        let size: usize = parts.iter().map(|(bytes, _)| bytes).sum();
        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let mut request = [0_u8; 4096];
            let _ = stream.read(&mut request).await;
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {size}\r\n\r\n");
            let _ = stream.write_all(head.as_bytes()).await;
            for (bytes, pause) in parts {
                let _ = stream.write_all(&vec![7_u8; bytes]).await;
                let _ = stream.flush().await;
                tokio::time::sleep(pause).await;
            }
            std::future::pending::<()>().await;
        });
        Ok(url)
    }

    fn full_sink(path: &std::path::Path) -> Result<File> {
        let made = Command::new("mkfifo").arg(path).status()?;
        assert!(made.success());
        let sink = OpenOptions::new().read(true).write(true).open(path)?;
        let mut filler = sink.try_clone()?;
        std::thread::spawn(move || filler.write_all(&vec![0_u8; 1 << 20]));
        Ok(sink)
    }

    fn drain(mut sink: File, after: Duration) {
        std::thread::spawn(move || {
            std::thread::sleep(after);
            let mut buffer = vec![0_u8; 64 * 1024];
            while sink.read(&mut buffer).is_ok_and(|read| read > 0) {}
        });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_download_whose_disk_stops_accepting_bytes_fails_by_the_idle_timeout() -> Result<()> {
        let directory =
            Directory(std::env::temp_dir().join(format!("velnor-fifo-{}", Uuid::new_v4())));
        std::fs::create_dir(&directory.0)?;
        for size in [8 << 20, 100] {
            let fifo = directory.0.join(format!("sink-{size}"));
            let sink = full_sink(&fifo)?;
            tokio::time::sleep(Duration::from_millis(200)).await;
            let mut response = http_client()?
                .get(serve(vec![(size, Duration::ZERO)]).await?)
                .send()
                .await?;
            let outcome = tokio::time::timeout(
                Duration::from_secs(5),
                receive(&mut response, &fifo, LIMIT, IDLE),
            )
            .await;
            drain(sink, Duration::ZERO);
            let failure = outcome
                .map_err(|_| anyhow::anyhow!("{size}: the transfer hung on the stalled disk"))?
                .err()
                .context("the stalled transfer succeeded")?;
            assert!(failure.retryable, "{size}");
            assert!(
                failure.error.to_string().contains("timed out"),
                "{size}: {:#}",
                failure.error
            );
        }
        Ok(())
    }

    async fn transfer(
        parts: Vec<(usize, Duration)>,
        drain_after: Duration,
    ) -> Result<std::result::Result<String, Failure>> {
        let directory =
            Directory(std::env::temp_dir().join(format!("velnor-fifo-{}", Uuid::new_v4())));
        std::fs::create_dir(&directory.0)?;
        let fifo = directory.0.join("sink");
        let sink = full_sink(&fifo)?;
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut response = http_client()?.get(serve(parts).await?).send().await?;
        drain(sink, drain_after);
        Ok(receive(&mut response, &fifo, LIMIT, WINDOW).await)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_idle_deadline_restarts_when_a_chunk_arrives_and_covers_its_write() -> Result<()> {
        let outcome = transfer(
            vec![(0, Duration::from_millis(1000)), (100, Duration::ZERO)],
            Duration::from_millis(1900),
        )
        .await?;
        if let Err(failure) = outcome {
            anyhow::bail!("{:#}", failure.error);
        }
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_write_does_not_extend_the_wait_for_the_next_chunk() -> Result<()> {
        let outcome = transfer(
            vec![
                (100, Duration::from_millis(100)),
                (100, Duration::from_millis(2000)),
                (100, Duration::ZERO),
            ],
            Duration::from_millis(1200),
        )
        .await?;
        let outcome = outcome.err().context("the late chunk was accepted")?;
        assert!(outcome.retryable);
        assert!(
            outcome.error.to_string().contains("timed out"),
            "{:#}",
            outcome.error
        );
        Ok(())
    }
}
