// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitCode, ExitStatus, Stdio};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::time::{MissedTickBehavior, interval};

use super::args::SandboxArgs;
use super::cgroup::Cgroup;
use super::trusted::Header;
use super::{Layout, dirs, disk};

const SETPRIV: &str = "/usr/bin/setpriv";
const UNSHARE: &str = "/usr/bin/unshare";
const KILLED_BASE: u8 = 128;
const DISK_LIMIT_EXIT: u8 = 125;
const WATCH_EVERY: Duration = Duration::from_secs(1);

pub(super) fn chain(layout: &Layout, runner: &Path, command: &[String]) -> Result<Vec<String>> {
    let mut argv: Vec<String> = [
        "--pdeathsig",
        "KILL",
        "--",
        UNSHARE,
        "--mount",
        "--pid",
        "--fork",
        "--kill-child",
        "--propagation",
        "private",
        "--",
    ]
    .iter()
    .map(|part| (*part).to_owned())
    .collect();
    argv.push(runner.display().to_string());
    argv.extend(["sandbox-apply".to_owned(), "--layout".to_owned()]);
    argv.push(serde_json::to_string(layout).context("serialize the sandbox layout")?);
    argv.push("--".to_owned());
    argv.extend(command.iter().cloned());
    Ok(argv)
}

fn exit_code(status: ExitStatus) -> ExitCode {
    let code = status.code().map_or_else(
        || {
            u8::try_from(status.signal().unwrap_or(0))
                .map_or(u8::MAX, |signal| KILLED_BASE.saturating_add(signal))
        },
        |code| u8::try_from(code).unwrap_or(u8::MAX),
    );
    ExitCode::from(code)
}

struct Interrupts {
    terminate: Signal,
    interrupt: Signal,
    hangup: Signal,
}

impl Interrupts {
    fn listen() -> Result<Self> {
        Ok(Self {
            terminate: signal(SignalKind::terminate()).context("listen for SIGTERM")?,
            interrupt: signal(SignalKind::interrupt()).context("listen for SIGINT")?,
            hangup: signal(SignalKind::hangup()).context("listen for SIGHUP")?,
        })
    }

    async fn next(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = self.interrupt.recv() => {}
            _ = self.hangup.recv() => {}
        }
    }
}

async fn watch(layout: &Layout, cgroup: &Cgroup, limit_gib: u64) -> Option<u64> {
    let directories = [layout.work_dir.clone(), layout.stage_dir.clone()];
    let held = cgroup.path().to_path_buf();
    let used = tokio::task::spawn_blocking(move || {
        let directories: Vec<&Path> = directories
            .iter()
            .map(std::path::PathBuf::as_path)
            .collect();
        disk::used(&directories, &held)
    })
    .await
    .ok()?;
    (used > disk::limit_bytes(limit_gib)).then_some(used)
}

struct Finished {
    status: ExitStatus,
    overage: Option<u64>,
}

async fn launch(args: &SandboxArgs, layout: &Layout, cgroup: &Cgroup) -> Result<Finished> {
    dirs::prepare(layout)?;
    let stdin = match &args.jit {
        Some(path) => Stdio::from(
            File::open(path).with_context(|| format!("open the JIT file {}", path.display()))?,
        ),
        None => Stdio::null(),
    };
    let runner = std::env::current_exe().context("find the runner binary")?;
    let mut interrupts = Interrupts::listen()?;
    let mut child = tokio::process::Command::new(SETPRIV)
        .args(chain(layout, &runner, &args.command)?)
        .env_clear()
        .stdin(stdin)
        .kill_on_drop(true)
        .spawn()
        .context("start the sandbox")?;
    let mut ticker = interval(WATCH_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut asked = false;
    let mut overage = None;
    loop {
        tokio::select! {
            status = child.wait() => {
                return Ok(Finished { status: status.context("wait for the sandbox")?, overage });
            }
            () = interrupts.next() => {
                if asked {
                    cgroup.kill();
                } else {
                    asked = true;
                    cgroup.terminate();
                }
            }
            _ = ticker.tick(), if overage.is_none() => {
                overage = watch(layout, cgroup, args.disk_limit_gib).await;
                if overage.is_some() {
                    cgroup.kill();
                }
            }
        }
    }
}

pub async fn run(args: &SandboxArgs) -> Result<ExitCode> {
    args.validate()?;
    if let Some(path) = &args.trusted {
        eprintln!(
            "velnor sandbox: reservation {} {}",
            args.reservation,
            Header::read(path)?.describe()
        );
    }
    let layout = dirs::layout(args)?;
    let cgroup = Cgroup::create(layout.cgroup.clone())?;
    let outcome = launch(args, &layout, &cgroup).await;
    let removed = cgroup.remove();
    for left in dirs::discard(&layout) {
        eprintln!("velnor sandbox: could not remove {}", left.display());
    }
    removed?;
    let finished = outcome?;
    if let Some(used) = finished.overage
        && let Some(message) = disk::overage(used, args.disk_limit_gib)
    {
        eprintln!("{message}");
        return Ok(ExitCode::from(DISK_LIMIT_EXIT));
    }
    Ok(exit_code(finished.status))
}
