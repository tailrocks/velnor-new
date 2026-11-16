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

const SPAWN_GRANDCHILD: &str = r"sh -c 'echo $$; exec sleep 300'; echo done";

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
        .env("VELNOR_CI_RUN_TERMINAL", "0")
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
    let exited = exits_within(&mut child, Duration::from_secs(5));
    if !exited {
        killpg(helper, Signal::SIGKILL).ok();
        kill(grandchild, Signal::SIGKILL).ok();
    }
    child.wait().unwrap_or_else(|e| unreachable!("{e}"));
    assert!(exited, "the helper outlived {signal}");
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

fn exits_within(child: &mut std::process::Child, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn child_group_with_terminal(terminal: &str) -> (i32, i32) {
    let mut helper = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--", "sh", "-c", "ps -o pgid= -p $$"])
        .env("RUNNER_TEMP", std::env::temp_dir())
        .env("VELNOR_CI_RUN_TERMINAL", terminal)
        .stdout(Stdio::piped())
        .process_group(0)
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let helper_group = i32::try_from(helper.id()).unwrap_or_else(|e| unreachable!("{e}"));
    let mut printed = String::new();
    std::io::Read::read_to_string(
        &mut helper
            .stdout
            .take()
            .unwrap_or_else(|| unreachable!("piped")),
        &mut printed,
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    helper.wait().unwrap_or_else(|e| unreachable!("{e}"));
    (
        helper_group,
        printed
            .trim()
            .parse()
            .unwrap_or_else(|e| unreachable!("{e}")),
    )
}

#[test]
fn run_keeps_the_child_in_its_group_when_a_terminal_is_present() {
    let (helper, child) = child_group_with_terminal("1");
    assert_eq!(child, helper);
}

#[test]
fn run_moves_the_child_to_its_own_group_without_a_terminal() {
    let (helper, child) = child_group_with_terminal("0");
    assert_ne!(child, helper);
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
