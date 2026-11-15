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
use super::pair::{private_folder, write_private};
use super::run::accept;
use crate::cli::SuperviseArgs;

const START_TIMEOUT: Duration = Duration::from_secs(20);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub struct Endpoint {
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

/// Public only for the integration tests of this crate.
pub async fn start(dir: &Path) -> io::Result<Endpoint> {
    stop(dir).await?;
    let authority = authority_dir(dir);
    private_folder(&authority).await?;
    private_folder(&dir.join("keys")).await?;
    for file in [LEDGER, "address", "pid", "binary"] {
        forget(&authority.join(file)).await?;
    }
    let token = keys::fresh(&dir.join("keys/authority")).await?;
    let log = std::fs::File::create(authority.join("supervise.log"))?;
    let binary = std::env::current_exe()?;
    let mut child = Command::new(&binary)
        .args(["supervise", "--dir"])
        .arg(dir)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    if let Some(pid) = child.id() {
        write_private(&authority.join("binary"), &binary.display().to_string()).await?;
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

async fn ours(pid: &str, dir: &Path, binary: &str) -> bool {
    let expected = format!("{binary} supervise --dir {}", dir.display());
    let command = program("ps", &["-p", pid, "-o", "command="], None).await;
    command.is_ok_and(|bytes| String::from_utf8_lossy(&bytes).trim() == expected)
}

async fn alive(pid: &str) -> bool {
    program("kill", &["-0", pid], None).await.is_ok()
}

async fn terminate(dir: &Path, pid: &str, binary: &str) -> io::Result<()> {
    for signal in ["-TERM", "-KILL"] {
        if !ours(pid, dir, binary).await {
            break;
        }
        program("kill", &[signal, pid], None).await?;
        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        while ours(pid, dir, binary).await && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    if ours(pid, dir, binary).await {
        return Err(io::Error::other(format!(
            "the authority, process {pid}, did not stop"
        )));
    }
    Ok(())
}

pub(super) async fn stop(dir: &Path) -> io::Result<()> {
    let authority = authority_dir(dir);
    let pid = fs::read_to_string(authority.join("pid")).await;
    if let Some(pid) = pid
        .ok()
        .map(|pid| pid.trim().to_owned())
        .filter(|pid| pid.parse::<u32>().is_ok())
    {
        let binary = match fs::read_to_string(authority.join("binary")).await {
            Ok(binary) => Some(binary),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        match binary {
            Some(binary) => terminate(dir, &pid, &binary).await?,
            None if alive(&pid).await => {
                return Err(io::Error::other(format!(
                    "process {pid} may be the authority, but its binary record is missing; stop process {pid} or remove {}, then run this again",
                    authority.join("pid").display()
                )));
            }
            None => {}
        }
    }
    forget(&authority.join("pid")).await?;
    forget(&authority.join("binary")).await?;
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
