// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;

fn gone(pid: Pid) -> bool {
    kill(pid, None) == Err(Errno::ESRCH)
}

const SPAWN_GRANDCHILD: &str = r#"sh -c 'echo $$; exec sleep 300'; echo done"#;

struct GroupGuard(Pid);

impl Drop for GroupGuard {
    fn drop(&mut self) {
        killpg(self.0, Signal::SIGKILL).ok();
    }
}

fn grandchild_is_stopped_by(signal: &str) {
    let temp = std::env::temp_dir().join(format!(
        "velnor-ci-run-group-{}-{signal}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp).unwrap_or_else(|e| unreachable!("{e}"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "sh", "-c", SPAWN_GRANDCHILD])
        .env("RUNNER_TEMP", &temp)
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "100")
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let helper = Pid::from_raw(i32::try_from(child.id()).unwrap_or_else(|e| unreachable!("{e}")));
    let _group = GroupGuard(helper);
    let stdout = child.stdout.take().unwrap_or_else(|| unreachable!("piped"));
    let first = BufReader::new(stdout)
        .lines()
        .next()
        .and_then(Result::ok)
        .unwrap_or_default();
    let grandchild = Pid::from_raw(first.trim().parse().unwrap_or_else(|e| unreachable!("{e}")));
    kill(helper, Signal::try_from(signal_number(signal)).ok()).ok();
    child.wait().unwrap_or_else(|e| unreachable!("{e}"));
    let deadline = Instant::now() + Duration::from_millis(1500);
    while !gone(grandchild) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let survived = !gone(grandchild);
    if survived {
        kill(grandchild, Signal::SIGKILL).ok();
    }
    std::fs::remove_dir_all(&temp).ok();
    assert!(!survived, "the grandchild outlived {signal}");
}

fn signal_number(name: &str) -> i32 {
    match name {
        "INT" => Signal::SIGINT as i32,
        _ => Signal::SIGTERM as i32,
    }
}

#[test]
fn run_stops_a_grandchild_on_sigterm() {
    grandchild_is_stopped_by("TERM");
}

#[test]
fn run_stops_a_grandchild_on_sigint() {
    grandchild_is_stopped_by("INT");
}
