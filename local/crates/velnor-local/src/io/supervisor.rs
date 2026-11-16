// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod host;

use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use tokio::fs;
use tokio::net::TcpListener;
use tokio::process::{Child, Command};
use tokio::signal::unix::{SignalKind, signal};

pub(super) use host::{Pairs, pairs};
pub use host::{default_host, write_plan};

use super::authority::{Authority, LEDGER, Runners};
use super::env::{DockerCli, Environment, program};
use super::keys;
use super::pair::{private_folder, write_private};
use super::run::accept;
use crate::cli::SuperviseArgs;
use crate::pairs::DECISIONS;
use host::{LOG, PID, runner_dir};

const START_TIMEOUT: Duration = Duration::from_secs(20);
const STOP_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug)]
pub struct Endpoint {
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

struct Starting(Option<Child>);

impl Starting {
    fn child(&mut self) -> io::Result<&mut Child> {
        self.0
            .as_mut()
            .ok_or_else(|| io::Error::other("the authority process was released"))
    }

    fn release(mut self) {
        self.0 = None;
    }
}

impl Drop for Starting {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.start_kill();
        }
    }
}

pub async fn start(dir: &Path) -> io::Result<Endpoint> {
    start_binary(dir, &std::env::current_exe()?).await
}

pub async fn start_binary(dir: &Path, binary: &Path) -> io::Result<Endpoint> {
    launch(dir, binary, None).await
}

pub(super) async fn start_with(dir: &Path, pairs: Option<&Pairs>) -> io::Result<Endpoint> {
    launch(dir, &std::env::current_exe()?, pairs).await
}

async fn launch(dir: &Path, binary: &Path, pairs: Option<&Pairs>) -> io::Result<Endpoint> {
    stop(dir).await?;
    let authority = authority_dir(dir);
    private_folder(&authority).await?;
    private_folder(&dir.join("keys")).await?;
    for file in [LEDGER, "address", "pid", "binary"] {
        forget(&authority.join(file)).await?;
    }
    for file in [DECISIONS, LOG, PID] {
        forget(&runner_dir(dir).join(file)).await?;
    }
    let token = keys::fresh(&dir.join("keys/authority")).await?;
    let log = std::fs::File::create(authority.join("supervise.log"))?;
    let mut starting = Starting(Some(
        Command::new(binary)
            .args(["supervise", "--dir"])
            .arg(dir)
            .args(pairs.map(Pairs::arguments).unwrap_or_default())
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log)
            .process_group(0)
            .spawn()?,
    ));
    if let Some(pid) = starting.child()?.id() {
        write_private(&authority.join("binary"), &binary.display().to_string()).await?;
        write_private(&authority.join("pid"), &pid.to_string()).await?;
    }
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        if let Ok(address) = fs::read_to_string(authority.join("address")).await {
            let pairs = match pairs {
                Some(pairs) => pairs.wait(dir, starting.child()?).await?,
                None => Vec::new(),
            };
            starting.release();
            return Ok(Endpoint {
                address: address.trim().to_owned(),
                token,
                pairs,
            });
        }
        if let Some(status) = starting.child()?.try_wait()? {
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

async fn command_of(pid: &str) -> io::Result<Option<String>> {
    let output = Command::new("ps")
        .args(["-p", pid, "-o", "command="])
        .stdin(Stdio::null())
        .output()
        .await?;
    if output.status.success() {
        let line = String::from_utf8_lossy(&output.stdout);
        return Ok(Some(line.trim().to_owned()));
    }
    if output.status.code() == Some(1) && output.stderr.is_empty() {
        return Ok(None);
    }
    let error = String::from_utf8_lossy(&output.stderr);
    Err(io::Error::other(format!(
        "ps -p {pid} -o command=: {}",
        error.trim()
    )))
}

async fn ours(pid: &str, dir: &Path, binary: &str) -> io::Result<bool> {
    let expected = format!("{binary} supervise --dir {}", dir.display());
    Ok(command_of(pid).await?.is_some_and(|line| {
        line == expected || line.starts_with(&format!("{expected} --velnor-host "))
    }))
}

async fn stop_host(dir: &Path) -> io::Result<Option<String>> {
    let runner = runner_dir(dir);
    let pid = fs::read_to_string(runner.join(PID)).await;
    let Some(pid) = pid
        .ok()
        .map(|pid| pid.trim().to_owned())
        .filter(|pid| pid.parse::<u32>().is_ok())
    else {
        return Ok(None);
    };
    let expected = format!(" --state {} local", runner.display());
    if command_of(&pid)
        .await?
        .is_some_and(|line| line.ends_with(&expected))
    {
        program("kill", &["-KILL", &pid], None).await?;
    }
    forget(&runner.join(PID)).await?;
    let log = fs::read_to_string(runner.join(LOG))
        .await
        .unwrap_or_default();
    let stopped = log
        .lines()
        .rev()
        .find(|line| line.ends_with(" agents removed"));
    Ok(stopped.map(str::to_owned))
}

fn alive(pid: &str) -> bool {
    let Ok(pid) = pid.parse::<i32>() else {
        return true;
    };
    match kill(Pid::from_raw(pid), None) {
        Ok(()) | Err(Errno::EPERM) => true,
        Err(_) => false,
    }
}

async fn terminate(dir: &Path, pid: &str, binary: &str) -> io::Result<()> {
    for signal in ["-TERM", "-KILL"] {
        if !ours(pid, dir, binary).await? {
            break;
        }
        program("kill", &[signal, pid], None).await?;
        let deadline = tokio::time::Instant::now() + STOP_TIMEOUT;
        while ours(pid, dir, binary).await? && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    if ours(pid, dir, binary).await? {
        return Err(io::Error::other(format!(
            "the authority, process {pid}, did not stop"
        )));
    }
    Ok(())
}

pub(super) async fn stop(dir: &Path) -> io::Result<Option<String>> {
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
            None if alive(&pid) => {
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
    forget(&authority.join("address")).await?;
    stop_host(dir).await
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
