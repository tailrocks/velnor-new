// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::fs;
use tokio::net::TcpListener;
use tokio::process::Command;
use tokio::signal::unix::{SignalKind, signal};

use super::authority::{Authority, LEDGER, Runners};
use super::env::{DockerCli, Environment, program};
use super::keys;
use super::pair::write_private;
use super::run::accept;
use crate::cli::SuperviseArgs;

const START_TIMEOUT: Duration = Duration::from_secs(20);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(super) struct Endpoint {
    pub(super) address: String,
    pub(super) token: String,
}

fn authority_dir(dir: &Path) -> PathBuf {
    dir.join("authority")
}

async fn forget(path: &Path) -> io::Result<()> {
    match fs::remove_file(path).await {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

pub(super) async fn start(dir: &Path) -> io::Result<Endpoint> {
    stop(dir).await?;
    let authority = authority_dir(dir);
    fs::create_dir_all(&authority).await?;
    fs::create_dir_all(dir.join("keys")).await?;
    for file in [LEDGER, "address", "pid"] {
        forget(&authority.join(file)).await?;
    }
    let token = keys::fresh(&dir.join("keys/authority")).await?;
    let log = std::fs::File::create(authority.join("supervise.log"))?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(["supervise", "--dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    if let Some(pid) = child.id() {
        write_private(&authority.join("pid"), &pid.to_string()).await?;
    }
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(address) = fs::read_to_string(authority.join("address")).await {
            return Ok(Endpoint {
                address: address.trim().to_owned(),
                token,
            });
        }
        if let Some(status) = child.try_wait()? {
            let message =
                format!("the authority exited with {status}; see authority/supervise.log");
            return Err(io::Error::other(message));
        }
        if tokio::time::Instant::now() > deadline {
            return Err(io::Error::other("the authority did not start within 20 s"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn ours(pid: &str, dir: &Path) -> bool {
    let command = program("ps", &["-p", pid, "-o", "command="], None).await;
    command.is_ok_and(|bytes| {
        let line = String::from_utf8_lossy(&bytes);
        line.contains(" supervise --dir ") && line.contains(&dir.display().to_string())
    })
}

pub(super) async fn stop(dir: &Path) -> io::Result<()> {
    let authority = authority_dir(dir);
    let pid = fs::read_to_string(authority.join("pid")).await;
    if let Some(pid) = pid
        .ok()
        .map(|pid| pid.trim().to_owned())
        .filter(|pid| pid.parse::<u32>().is_ok())
    {
        for signal in ["-TERM", "-KILL"] {
            if !ours(&pid, dir).await {
                break;
            }
            program("kill", &[signal, &pid], None).await?;
            let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
            while ours(&pid, dir).await && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        if ours(&pid, dir).await {
            return Err(io::Error::other(format!(
                "the authority, process {pid}, did not stop"
            )));
        }
    }
    forget(&authority.join("pid")).await?;
    forget(&authority.join("address")).await
}

pub(super) async fn serve(args: SuperviseArgs) -> io::Result<()> {
    let env = Environment::at(&args.dir, DockerCli).await?;
    let dir = authority_dir(&env.dir);
    let token = fs::read_to_string(env.dir.join("keys/authority")).await?;
    let token = token.trim().to_owned();
    if token.len() != 64 {
        return Err(io::Error::other(
            "keys/authority does not hold a 32-byte token",
        ));
    }
    let authority = Arc::new(Authority::open(Runners::open(env).await?, token, &dir).await?);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    write_private(&dir.join("address"), &address.to_string()).await?;
    let mut terminated = signal(SignalKind::terminate())?;
    let handler = move |request| {
        let authority = Arc::clone(&authority);
        async move { authority.handle(request).await }
    };
    tokio::select! {
        served = accept(listener, handler) => served,
        _ = terminated.recv() => Ok(()),
        _ = tokio::signal::ctrl_c() => Ok(()),
    }
}
