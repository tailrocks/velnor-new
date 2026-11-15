// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod host;

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

pub(super) use host::{Pairs, pairs};

use super::authority::{Authority, LEDGER, Runners};
use super::env::{DockerCli, Environment, program};
use super::keys;
use super::pair::write_private;
use super::run::accept;
use crate::cli::SuperviseArgs;
use crate::pairs::DECISIONS;
use host::{LOG, PID, runner_dir};

const START_TIMEOUT: Duration = Duration::from_secs(20);
const STOP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub(super) struct Endpoint {
    pub(super) address: String,
    pub(super) token: String,
    pub(super) pairs: Vec<String>,
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

pub(super) async fn start(dir: &Path, pairs: Option<&Pairs>) -> io::Result<Endpoint> {
    stop(dir).await?;
    let authority = authority_dir(dir);
    fs::create_dir_all(&authority).await?;
    fs::create_dir_all(dir.join("keys")).await?;
    for file in [LEDGER, "address", "pid"] {
        forget(&authority.join(file)).await?;
    }
    for file in [DECISIONS, LOG, PID] {
        forget(&runner_dir(dir).join(file)).await?;
    }
    let token = keys::fresh(&dir.join("keys/authority")).await?;
    let log = std::fs::File::create(authority.join("supervise.log"))?;
    let mut child = Command::new(std::env::current_exe()?)
        .args(["supervise", "--dir"])
        .arg(dir)
        .args(pairs.map(Pairs::arguments).unwrap_or_default())
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
            let pairs = match pairs {
                Some(pairs) => pairs.wait(dir, &mut child).await?,
                None => Vec::new(),
            };
            return Ok(Endpoint {
                address: address.trim().to_owned(),
                token,
                pairs,
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

async fn ours(pid: &str, command: &str) -> bool {
    let listed = program("ps", &["-p", pid, "-o", "command="], None).await;
    listed.is_ok_and(|bytes| String::from_utf8_lossy(&bytes).contains(command))
}

async fn kill(file: &Path, command: &str, what: &str) -> io::Result<()> {
    let pid = fs::read_to_string(file).await;
    if let Some(pid) = pid
        .ok()
        .map(|pid| pid.trim().to_owned())
        .filter(|pid| pid.parse::<u32>().is_ok())
    {
        for signal in ["-TERM", "-KILL"] {
            if !ours(&pid, command).await {
                break;
            }
            program("kill", &[signal, &pid], None).await?;
            let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
            while ours(&pid, command).await && tokio::time::Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        if ours(&pid, command).await {
            return Err(io::Error::other(format!(
                "{what}, process {pid}, did not stop"
            )));
        }
    }
    forget(file).await
}

pub(super) async fn stop(dir: &Path) -> io::Result<Option<String>> {
    let authority = authority_dir(dir);
    let supervise = format!(" supervise --dir {}", dir.display());
    kill(&authority.join("pid"), &supervise, "the authority").await?;
    forget(&authority.join("address")).await?;
    let runner = runner_dir(dir);
    let running = fs::try_exists(runner.join(PID)).await?;
    let host = format!(" --state {} local", runner.display());
    kill(&runner.join(PID), &host, "velnor-host").await?;
    let log = fs::read_to_string(runner.join(LOG))
        .await
        .unwrap_or_default();
    let stopped = log
        .lines()
        .rev()
        .find(|line| line.ends_with(" agents removed"));
    Ok(stopped.filter(|_| running).map(str::to_owned))
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
    let host_env = Environment::at(&args.dir, DockerCli).await?;
    let authority =
        Arc::new(Authority::open(Runners::open(env).await?, token.clone(), &dir).await?);
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await?;
    let address = listener.local_addr()?;
    let mut terminated = signal(SignalKind::terminate())?;
    let handler = move |request| {
        let authority = Arc::clone(&authority);
        async move { authority.handle(request).await }
    };
    let served = accept(listener, handler);
    tokio::pin!(served);
    let Some(path) = args.velnor_host else {
        write_private(&dir.join("address"), &address.to_string()).await?;
        return tokio::select! {
            served = &mut served => served,
            _ = terminated.recv() => Ok(()),
            _ = tokio::signal::ctrl_c() => Ok(()),
        };
    };
    let counts = (args.pairs, args.fork_pairs);
    let address = address.to_string();
    let mut child = host::spawn(&host_env, &path, &address, &token, counts).await?;
    write_private(&dir.join("address"), &address).await?;
    tokio::select! {
        served = &mut served => return served,
        status = child.wait() => {
            return Err(io::Error::other(format!("velnor-host exited with {}", status?)));
        }
        _ = terminated.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    if let Some(pid) = child.id() {
        program("kill", &["-TERM", &pid.to_string()], None).await?;
    }
    tokio::select! {
        served = &mut served => served,
        status = child.wait() => {
            eprintln!("velnor-host exited with {}", status?);
            Ok(())
        }
    }
}
