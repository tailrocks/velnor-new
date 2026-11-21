// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::{scratch, velnor_ci_run};
use crate::run_nextest::archive_in;
use crate::run_nextest_extract::{Scenario, archive_run, assert_withheld};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

const HANGING_RUN: &str = "echo '        PASS [   0.012s] (1/1) kept alpha' >&2\necho '     Summary [   0.012s] 1 test run: 1 passed, 0 skipped' >&2\nexec sleep 300";
const RUN_HOLDING_ITS_OUTPUT: &str = "sleep 300 &\necho $! > \"$RUNNER_TEMP/held-pid\"\nexit 0";

fn fake_nextest(run_tail: &str) -> String {
    format!(
        r#"#!/bin/sh
if [ "$2" = list ]; then
  trap '' TERM INT
  echo $$ > "$RUNNER_TEMP/list-pid"
  sh -c 'trap "" TERM INT; echo $$ > "$RUNNER_TEMP/gc-pid"; exec sleep 300' &
  wait
fi
echo '    Starting 1 tests across 1 binary' >&2
while [ ! -s "$RUNNER_TEMP/gc-pid" ]; do sleep 0.05; done
{run_tail}
"#
    )
}

fn pid_in(file: &Path) -> Option<Pid> {
    fs::read_to_string(file)
        .ok()
        .filter(|text| text.ends_with('\n'))
        .and_then(|text| text.trim().parse().ok())
        .map(Pid::from_raw)
}

fn gone(pid: Pid) -> bool {
    kill(pid, None) == Err(Errno::ESRCH)
}

fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if done() {
            return true;
        }
        std::thread::yield_now();
    }
    done()
}

struct Cleanup(Vec<Pid>, Pid);

impl Drop for Cleanup {
    fn drop(&mut self) {
        killpg(self.1, Signal::SIGKILL).ok();
        for pid in &self.0 {
            kill(*pid, Signal::SIGKILL).ok();
            killpg(*pid, Signal::SIGKILL).ok();
        }
    }
}

fn start(name: &str, terminal: &str, run_tail: &str) -> (Child, std::path::PathBuf) {
    let temp = scratch(name);
    archive_in(&temp);
    let program = temp.join("cargo-nextest");
    fs::write(&program, fake_nextest(run_tail)).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = temp.join("github-output");
    fs::write(&outputs, "").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let child = velnor_ci_run()
        .arg("run")
        .arg("--")
        .args(archive_run(&["--extract-to", "."]))
        .current_dir(&temp)
        .env("RUNNER_TEMP", &temp)
        .env("GITHUB_OUTPUT", &outputs)
        .env("PATH", path)
        .env("VELNOR_CI_RUN_TERMINAL", terminal)
        .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "100")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(
            fs::File::create(temp.join("velnor-stderr")).unwrap_or_else(|e| unreachable!("{e}")),
        )
        .process_group(0)
        .spawn()
        .unwrap_or_else(|e| unreachable!("{e}"));
    (child, temp)
}

fn interrupt_stops_the_list_tree(name: &str, terminal: &str, to_group: bool, signal: Signal) {
    let (mut child, temp) = start(name, terminal, HANGING_RUN);
    let velnor = Pid::from_raw(i32::try_from(child.id()).unwrap_or_else(|e| unreachable!("{e}")));
    let mut cleanup = Cleanup(Vec::new(), velnor);
    assert!(
        wait_until(Duration::from_secs(20), || pid_in(&temp.join("gc-pid"))
            .is_some()),
        "the list never started its grandchild"
    );
    let list = pid_in(&temp.join("list-pid")).unwrap_or_else(|| unreachable!("list pid"));
    let grandchild = pid_in(&temp.join("gc-pid")).unwrap_or_else(|| unreachable!("gc pid"));
    cleanup.0 = vec![list, grandchild];
    assert!(
        wait_until(Duration::from_secs(20), || {
            fs::read_to_string(temp.join("velnor-stderr"))
                .is_ok_and(|text| text.contains("Summary"))
        }),
        "the run's counts never reached velnor-ci"
    );
    let sent = if to_group {
        killpg(velnor, signal)
    } else {
        kill(velnor, signal)
    };
    sent.unwrap_or_else(|e| unreachable!("{e}"));
    let exited = wait_until(Duration::from_secs(10), || {
        child.try_wait().ok().flatten().is_some()
    });
    assert!(exited, "velnor-ci outlived {signal:?} while the list hung");
    assert!(
        wait_until(Duration::from_secs(5), || gone(list) && gone(grandchild)),
        "the early list tree survived {signal:?}"
    );
    child.wait().unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&temp).ok();
}

#[test]
fn sigterm_to_velnor_ci_stops_a_hanging_early_list_and_its_grandchild() {
    interrupt_stops_the_list_tree("early-cancel-term", "0", false, Signal::SIGTERM);
}

#[test]
fn sigterm_to_the_whole_group_stops_a_hanging_early_list_and_its_grandchild() {
    interrupt_stops_the_list_tree("early-cancel-group-term", "0", true, Signal::SIGTERM);
}

#[test]
fn a_terminal_interrupt_stops_a_hanging_early_list_and_its_grandchild() {
    interrupt_stops_the_list_tree("early-cancel-terminal-int", "1", true, Signal::SIGINT);
}

#[test]
fn a_list_that_exits_while_a_descendant_holds_its_output_is_stopped_and_reported() {
    let mut scenario = Scenario::failing(
        "sh -c 'trap \"\" TERM; echo $$ > \"$RUNNER_TEMP/gc-pid\"; exec sleep 300' &\nwhile [ ! -s \"$RUNNER_TEMP/gc-pid\" ]; do sleep 0.05; done\nexit 0",
    );
    scenario.settings = vec![("VELNOR_CI_RUN_EARLY_LIST_GRACE_MS", "60000")];
    let started = Instant::now();
    let (output, written, temp) =
        scenario.execute("early-holders", &archive_run(&["--extract-to", "."]), None);
    assert!(output.status.success(), "{output:?}");
    assert!(started.elapsed() < Duration::from_secs(30));
    assert_withheld(&written);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "::warning::early test listing left processes holding its output; stopped them; continuing without test counts"
        ),
        "{stderr}"
    );
    let grandchild = pid_in(&temp.join("gc-pid")).unwrap_or_else(|| unreachable!("gc pid"));
    let survived = !wait_until(Duration::from_secs(5), || gone(grandchild));
    kill(grandchild, Signal::SIGKILL).ok();
    assert!(!survived, "the descendant holding the output survived");
}

#[test]
fn a_hanging_early_list_is_stopped_at_once_when_the_run_output_is_incomplete() {
    let (mut child, temp) = start("early-unwanted", "0", RUN_HOLDING_ITS_OUTPUT);
    let velnor = Pid::from_raw(i32::try_from(child.id()).unwrap_or_else(|e| unreachable!("{e}")));
    let mut cleanup = Cleanup(Vec::new(), velnor);
    let exited = wait_until(Duration::from_secs(5), || {
        child.try_wait().ok().flatten().is_some()
    });
    let held = pid_in(&temp.join("held-pid"));
    let list = pid_in(&temp.join("list-pid"));
    let grandchild = pid_in(&temp.join("gc-pid"));
    cleanup.0 = [held, list, grandchild].into_iter().flatten().collect();
    assert!(
        exited,
        "velnor-ci waited for a list whose counts are unusable"
    );
    let list = list.unwrap_or_else(|| unreachable!("list pid"));
    let grandchild = grandchild.unwrap_or_else(|| unreachable!("gc pid"));
    assert!(
        wait_until(Duration::from_secs(5), || gone(list) && gone(grandchild)),
        "the early list tree survived"
    );
    child.wait().unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(&temp).ok();
}
