// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{ExitCode, ExitStatus, Stdio};

use anyhow::{Context, Result};
use tokio::signal::unix::{Signal, SignalKind, signal};
use tokio::time::{Instant, MissedTickBehavior, interval_at};

use super::args::SandboxArgs;
use super::cgroup::Cgroup;
use super::jit::Jit;
use super::trusted::Header;
use super::watch::{Breach, Limits, WATCH_EVERY, Watch};
use super::{Layout, cleanup, dirs, net};

const SETPRIV: &str = "/usr/bin/setpriv";
const NSENTER: &str = "/usr/bin/nsenter";
const UNSHARE: &str = "/usr/bin/unshare";
const KILLED_BASE: u8 = 128;
const BREACH_EXIT: u8 = 125;

pub(super) fn chain(layout: &Layout, runner: &Path, command: &[String]) -> Result<Vec<String>> {
    let mut argv: Vec<String> = ["--pdeathsig", "KILL", "--", NSENTER]
        .iter()
        .map(|part| (*part).to_owned())
        .collect();
    argv.push(format!("--net={}", layout.netns.display()));
    argv.extend(
        [
            "--",
            UNSHARE,
            "--mount",
            "--pid",
            "--ipc",
            "--uts",
            "--fork",
            "--kill-child",
            "--propagation",
            "private",
            "--",
        ]
        .iter()
        .map(|part| (*part).to_owned()),
    );
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

struct Finished {
    status: ExitStatus,
    breach: Option<Breach>,
}

async fn launch(
    args: &SandboxArgs,
    layout: &Layout,
    cgroup: &Cgroup,
    jit: Option<Jit>,
) -> Result<Finished> {
    dirs::prepare(layout)?;
    net::up(args.reservation)?;
    let stdin = match jit {
        Some(jit) => Stdio::from(jit.pipe()?),
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
    let limits = Limits {
        disk_gib: args.disk_limit_gib,
        floor_gib: args.machine_floor_gib,
    };
    let mut ticker = interval_at(Instant::now() + WATCH_EVERY, WATCH_EVERY);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut watch = Watch::new();
    let mut asked = false;
    let mut breach = None;
    loop {
        tokio::select! {
            status = child.wait() => {
                return Ok(Finished { status: status.context("wait for the sandbox")?, breach });
            }
            () = interrupts.next() => {
                if asked {
                    cgroup.kill();
                } else {
                    asked = true;
                    cgroup.terminate();
                }
            }
            _ = ticker.tick() => {
                if breach.is_none() {
                    breach = watch.poll(layout, cgroup, &limits);
                }
                if breach.is_some() {
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
    let jit = args
        .jit_stdin
        .then(|| Jit::read(std::io::stdin().lock()))
        .transpose()?;
    let layout = dirs::layout(args)?;
    let cgroup = Cgroup::create(layout.cgroup.clone())?;
    let outcome = launch(args, &layout, &cgroup, jit).await;
    cleanup::reservation(args.reservation)?;
    let finished = outcome?;
    if let Some(breach) = finished.breach {
        eprintln!("{}", breach.message(args.disk_limit_gib));
        return Ok(ExitCode::from(BREACH_EXIT));
    }
    Ok(exit_code(finished.status))
}
