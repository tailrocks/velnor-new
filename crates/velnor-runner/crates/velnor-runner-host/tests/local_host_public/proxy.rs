// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::task::JoinHandle;

use super::support::{Endpoint, TestResult};

#[derive(Clone, Copy)]
pub(crate) enum Wait {
    Forward,
    Fail,
    Finish,
}

pub(crate) struct Faults {
    waits: fn(usize) -> Wait,
    waited: AtomicUsize,
    removals: AtomicUsize,
    sweep_removals: AtomicUsize,
    retained_container: Option<String>,
    pub(crate) hang_creations: AtomicBool,
}

impl Faults {
    pub(crate) fn new(waits: fn(usize) -> Wait, removals: usize) -> Arc<Self> {
        Arc::new(Self {
            waits,
            waited: AtomicUsize::new(0),
            removals: AtomicUsize::new(removals),
            sweep_removals: AtomicUsize::new(usize::MAX),
            retained_container: None,
            hang_creations: AtomicBool::new(false),
        })
    }

    pub(crate) fn fail_second_removal() -> Arc<Self> {
        Arc::new(Self {
            waits: |_| Wait::Forward,
            waited: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
            sweep_removals: AtomicUsize::new(0),
            retained_container: None,
            hang_creations: AtomicBool::new(false),
        })
    }

    pub(crate) fn retain_container(container: &str) -> Arc<Self> {
        Arc::new(Self {
            waits: |_| Wait::Forward,
            waited: AtomicUsize::new(0),
            removals: AtomicUsize::new(0),
            sweep_removals: AtomicUsize::new(usize::MAX),
            retained_container: Some(container.to_owned()),
            hang_creations: AtomicBool::new(false),
        })
    }

    fn hangs(&self, line: &str) -> bool {
        self.hang_creations.load(Ordering::SeqCst)
            && line.starts_with("POST ")
            && (line.contains("/volumes/create") || line.contains("/containers/create"))
    }

    fn answer(&self, line: &str) -> Option<(u16, &'static str)> {
        if line.starts_with("DELETE ")
            && self.retained_container.as_ref().is_some_and(|container| {
                line.split_whitespace()
                    .nth(1)
                    .and_then(|path| path.split('?').next())
                    .is_some_and(|path| path.ends_with(&format!("/containers/{container}")))
            })
        {
            return Some((500, "{\"message\":\"injected\"}"));
        }
        if line.starts_with("DELETE ")
            && line.contains("/containers/")
            && self.sweep_removals.load(Ordering::SeqCst) != usize::MAX
            && self.sweep_removals.fetch_add(1, Ordering::SeqCst) == 1
        {
            return Some((500, "{\"message\":\"injected\"}"));
        }
        if line.starts_with("POST ")
            && line.contains("/containers/")
            && line.contains("/wait")
            && !line.contains("condition=")
        {
            return match (self.waits)(self.waited.fetch_add(1, Ordering::SeqCst)) {
                Wait::Forward => None,
                Wait::Fail => Some((500, "{\"message\":\"injected\"}")),
                Wait::Finish => Some((200, "{\"StatusCode\":0}")),
            };
        }
        let removal = line.starts_with("DELETE ")
            && line.contains("/containers/")
            && self.waited.load(Ordering::SeqCst) > 0;
        let failing = removal
            && self
                .removals
                .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok();
        failing.then_some((500, "{\"message\":\"injected\"}"))
    }
}

pub(crate) struct Proxy {
    pub(crate) docker: Docker,
    _directory: tempfile::TempDir,
    task: JoinHandle<()>,
}

impl Proxy {
    pub(crate) fn start(upstream: &Endpoint, faults: Arc<Faults>) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("docker.sock");
        let listener = UnixListener::bind(&socket)?;
        let upstream = upstream.clone();
        let task = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let upstream = upstream.clone();
                let faults = Arc::clone(&faults);
                tokio::spawn(async move {
                    let _ended = relay(client, &upstream, &faults).await;
                });
            }
        });
        let docker = velnor_runner_host::connect_unix(&format!("unix://{}", socket.display()))?;
        Ok(Self {
            docker,
            _directory: directory,
            task,
        })
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| at + 4)
}

async fn relay(mut client: UnixStream, upstream: &Endpoint, faults: &Faults) -> io::Result<()> {
    let mut pending = Vec::new();
    loop {
        let end = loop {
            if let Some(end) = head_end(&pending) {
                break end;
            }
            let mut chunk = [0_u8; 8192];
            let read = client.read(&mut chunk).await?;
            if read == 0 {
                return Ok(());
            }
            pending.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&pending[..end]).into_owned();
        let line = head.lines().next().unwrap_or_default();
        if faults.hangs(line) {
            tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        }
        if let Some((status, body)) = faults.answer(line) {
            let reply = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            client.write_all(reply.as_bytes()).await?;
            pending.drain(..end);
            continue;
        }
        let mut forwarded = pending[..end - 2].to_vec();
        if !head.to_ascii_lowercase().contains("upgrade") {
            forwarded.extend_from_slice(b"Connection: close\r\n");
        }
        forwarded.extend_from_slice(b"\r\n");
        forwarded.extend_from_slice(&pending[end..]);
        match upstream {
            Endpoint::Unix(path) => {
                let mut server = UnixStream::connect(path).await?;
                server.write_all(&forwarded).await?;
                copy_bidirectional(&mut client, &mut server).await?;
            }
            Endpoint::Tcp(address) => {
                let mut server = TcpStream::connect(address).await?;
                server.write_all(&forwarded).await?;
                copy_bidirectional(&mut client, &mut server).await?;
            }
        }
        return Ok(());
    }
}
