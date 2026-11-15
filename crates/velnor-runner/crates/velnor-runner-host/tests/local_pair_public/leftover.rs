// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{BufReader, Cursor, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use velnor_runner_host::HostError;
use velnor_runner_host::local::{OnePairError, START_GRACE, remove_pair_named, run_one_pair};

use super::support::{Fixture, TestResult, labelled};

struct NoDelete {
    directory: tempfile::TempDir,
}

impl NoDelete {
    fn start(blocked_from_start: bool) -> TestResult<Self> {
        let upstream = std::env::var("DOCKER_HOST")
            .unwrap_or_else(|_| "unix:///var/run/docker.sock".to_owned());
        let upstream = PathBuf::from(upstream.trim_start_matches("unix://"));
        let directory = tempfile::tempdir()?;
        let listener = UnixListener::bind(directory.path().join("proxy.sock"))?;
        std::thread::spawn(move || {
            let armed = Arc::new(AtomicBool::new(blocked_from_start));
            for client in listener.incoming().flatten() {
                if let Ok(server) = UnixStream::connect(&upstream) {
                    forward(client, server, &armed);
                }
            }
        });
        Ok(Self { directory })
    }

    fn url(&self) -> String {
        format!(
            "unix://{}",
            self.directory.path().join("proxy.sock").display()
        )
    }
}

fn forward(client: UnixStream, server: UnixStream, armed: &Arc<AtomicBool>) {
    let armed = Arc::clone(armed);
    let (Ok(mut client_read), Ok(mut server_write)) = (client.try_clone(), server.try_clone())
    else {
        return;
    };
    let (mut server_read, mut client_write) = (server, client);
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 16384];
        while let Ok(read) = client_read.read(&mut buffer) {
            let head = &buffer[..read];
            if head.starts_with(b"POST ")
                && head.windows(8).any(|window| window == b"-runner ")
                && head
                    .windows(18)
                    .any(|window| window == b"/containers/create")
            {
                armed.store(true, Ordering::SeqCst);
            }
            if read == 0 || (head.starts_with(b"DELETE ") && armed.load(Ordering::SeqCst)) {
                break;
            }
            if server_write.write_all(&buffer[..read]).is_err() {
                break;
            }
        }
        drop(server_write.shutdown(std::net::Shutdown::Both));
        drop(client_read.shutdown(std::net::Shutdown::Both));
    });
    std::thread::spawn(move || {
        drop(std::io::copy(&mut server_read, &mut client_write));
        drop(client_write.shutdown(std::net::Shutdown::Both));
    });
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        Err(std::io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn request_line(fixture: &Fixture, docker: &str, runner_image: &str) -> String {
    let pair = fixture.request();
    let line = serde_json::json!({"docker": docker, "pair": {
        "environment": pair.environment, "network": pair.network, "platform": pair.platform,
        "runner_image": runner_image, "dind_image": pair.dind_image,
        "sequence": pair.sequence,
        "runner_cpu_millicores": pair.runner_cpu_millicores,
        "runner_memory_bytes": pair.runner_memory_bytes,
        "dind_cpu_millicores": pair.dind_cpu_millicores,
        "dind_memory_bytes": pair.dind_memory_bytes,
    }});
    format!("{line}\n")
}

fn run(line: String, mut output: impl Write + Send + 'static) -> Result<(), OnePairError> {
    let mut input = BufReader::new(Cursor::new(line.into_bytes()));
    std::thread::spawn(move || run_one_pair(&mut input, &mut output))
        .join()
        .unwrap_or(Err(HostError::Path.into()))
}

async fn remove_and_count(fixture: &Fixture, name: &str) -> TestResult<(usize, usize)> {
    remove_pair_named(&fixture.docker, name).await?;
    labelled(&fixture.docker, &fixture.environment).await
}

fn named(outcome: Result<(), OnePairError>) -> TestResult<(HostError, String)> {
    let failure = outcome.err().ok_or("one-shot succeeded")?;
    let name = failure.leftover.clone().ok_or("no leftover pair name")?;
    assert!(failure.to_string().contains(&name));
    Ok((failure.error, name))
}

#[tokio::test]
async fn local_pair_one_shot_names_the_pair_when_a_failed_start_cannot_be_cleaned_up()
-> TestResult<()> {
    let fixture = Fixture::new().await?;
    let proxy = NoDelete::start(true)?;
    let bogus = format!("sha256:{}", "b".repeat(64));
    let outcome = run(request_line(&fixture, &proxy.url(), &bogus), Vec::new());
    let named = named(outcome);
    let left = match &named {
        Ok((_, name)) => remove_and_count(&fixture, name).await,
        Err(_) => labelled(&fixture.docker, &fixture.environment).await,
    };
    fixture.finish().await?;
    assert_eq!(named?.0, HostError::Docker);
    assert_eq!(left?, (0, 0));
    Ok(())
}

#[tokio::test]
async fn local_pair_one_shot_names_the_pair_when_the_startup_write_fails_and_cleanup_fails()
-> TestResult<()> {
    let fixture = Fixture::new().await?;
    let proxy = NoDelete::start(false)?;
    let outcome = run(request_line(&fixture, &proxy.url(), &fixture.image), Broken);
    let named = named(outcome);
    let left = match &named {
        Ok((_, name)) => remove_and_count(&fixture, name).await,
        Err(_) => labelled(&fixture.docker, &fixture.environment).await,
    };
    fixture.finish().await?;
    assert_eq!(named?.0, HostError::Path);
    assert_eq!(left?, (0, 0));
    Ok(())
}

#[test]
fn local_pair_start_grace_is_pinned_at_250_milliseconds() {
    assert_eq!(START_GRACE, std::time::Duration::from_millis(250));
}
