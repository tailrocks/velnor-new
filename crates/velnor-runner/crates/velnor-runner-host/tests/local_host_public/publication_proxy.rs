// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;

use bollard::Docker;
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpStream, UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use super::support::{Endpoint, TestResult};

pub(crate) struct Stage {
    pub(crate) name: String,
    pub(crate) bytes: Vec<u8>,
    pub(crate) document: Vec<u8>,
    pub(crate) resume: oneshot::Sender<()>,
}

pub(crate) struct PublicationProxy {
    pub(crate) docker: Docker,
    pub(crate) stages: mpsc::UnboundedReceiver<Stage>,
    _directory: tempfile::TempDir,
    task: JoinHandle<()>,
}

impl PublicationProxy {
    pub(crate) fn start(upstream: &Endpoint) -> TestResult<Self> {
        let directory = tempfile::tempdir()?;
        let socket = directory.path().join("publication.sock");
        let listener = UnixListener::bind(&socket)?;
        let (stages, receive) = mpsc::unbounded_channel();
        let endpoint = upstream.clone();
        let task = tokio::spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                let endpoint = endpoint.clone();
                let stages = stages.clone();
                tokio::spawn(async move {
                    let _relayed = match endpoint {
                        Endpoint::Unix(path) => match UnixStream::connect(path).await {
                            Ok(server) => relay(client, server, &stages).await,
                            Err(error) => Err(error),
                        },
                        Endpoint::Tcp(address) => match TcpStream::connect(address).await {
                            Ok(server) => relay(client, server, &stages).await,
                            Err(error) => Err(error),
                        },
                    };
                });
            }
        });
        Ok(Self {
            docker: velnor_runner_host::connect_unix(&format!("unix://{}", socket.display()))?,
            stages: receive,
            _directory: directory,
            task,
        })
    }
}

impl Drop for PublicationProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn pause(
    stages: &mpsc::UnboundedSender<Stage>,
    name: &str,
    bytes: &[u8],
    document: &[u8],
) -> io::Result<()> {
    let (resume, wait) = oneshot::channel();
    stages
        .send(Stage {
            name: name.to_owned(),
            bytes: bytes.to_vec(),
            document: document.to_vec(),
            resume,
        })
        .map_err(io::Error::other)?;
    wait.await.map_err(io::Error::other)
}

fn decision(body: &[u8]) -> io::Result<Option<(String, usize)>> {
    let mut archive = tar::Archive::new(body);
    let entry = archive.entries()?.next().transpose()?;
    let Some(entry) = entry else {
        return Ok(None);
    };
    let path = entry.path()?.into_owned();
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if !name.contains("decision.json") {
        return Ok(None);
    }
    Ok(Some((
        name.to_owned(),
        usize::try_from(entry.size()).map_err(io::Error::other)?,
    )))
}

async fn relay<S>(
    mut client: UnixStream,
    mut server: S,
    stages: &mpsc::UnboundedSender<Stage>,
) -> io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut pending = Vec::new();
    let end = loop {
        if let Some(end) = pending.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            break end + 4;
        }
        let mut chunk = [0_u8; 8192];
        let read = client.read(&mut chunk).await?;
        if read == 0 {
            return Ok(());
        }
        pending.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&pending[..end]);
    let upload = head
        .lines()
        .next()
        .is_some_and(|line| line.starts_with("PUT ") && line.contains("/archive?"));
    let mut forwarded_head = pending[..end - 2].to_vec();
    if !head.to_ascii_lowercase().contains("upgrade") {
        forwarded_head.extend_from_slice(b"Connection: close\r\n");
    }
    forwarded_head.extend_from_slice(b"\r\n");
    if upload {
        let length = head
            .lines()
            .filter_map(|line| line.split_once(':'))
            .find(|(key, _)| key.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .ok_or_else(|| io::Error::other("archive upload has no content length"))?;
        let already_read = pending.len();
        pending.resize(end + length, 0);
        let mut body = pending.split_off(end);
        let buffered = already_read - end;
        client.read_exact(&mut body[buffered..]).await?;
        if let Some((name, size)) = decision(&body)? {
            let split = size / 2;
            server.write_all(&forwarded_head).await?;
            server.write_all(&body[..512]).await?;
            pause(stages, &name, &[], &body[512..512 + size]).await?;
            server.write_all(&body[512..512 + split]).await?;
            pause(
                stages,
                &name,
                &body[512..512 + split],
                &body[512..512 + size],
            )
            .await?;
            server.write_all(&body[512 + split..]).await?;
            copy_bidirectional(&mut client, &mut server).await?;
            return Ok(());
        }
        pending.extend_from_slice(&body);
    }
    server.write_all(&forwarded_head).await?;
    server.write_all(&pending[end..]).await?;
    copy_bidirectional(&mut client, &mut server).await?;
    Ok(())
}
