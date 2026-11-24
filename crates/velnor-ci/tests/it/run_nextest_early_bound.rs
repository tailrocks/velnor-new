// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run::{scratch, velnor_ci_run};
use crate::run_nextest::{archive_in, listed};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

const RECORD_LIST_PID: &str = "echo $$ > \"$RUNNER_TEMP/list-pid\"\n";
const RUN_START: &str = "echo '    Starting 1 tests across 1 binary' >&2\n";
const RUN_END: &str = "echo '        PASS [   0.012s] (1/1) kept alpha' >&2\necho '     Summary [   0.012s] 1 test run: 1 passed, 0 skipped' >&2\n";
const WAIT_FOR_LIST_PID: &str = "i=0\nwhile [ ! -s \"$RUNNER_TEMP/list-pid\" ] && [ $i -lt 100 ]; do sleep 0.05; i=$((i+1)); done\n";
const WITHHELD: &[&str] = &["tests_listed=", "tests_ignored=", "tests_by_package="];
const PATIENT: &str = "60000";
const GRACE_VARIABLE: &str = "VELNOR_CI_EARLY_LIST_GRACE_MS";
const TERMINAL_VARIABLE: &str = "VELNOR_CI_RUN_TERMINAL";
const STDOUT_LIMIT_VARIABLE: &str = "VELNOR_CI_EARLY_LIST_STDOUT_LIMIT_BYTES";

struct Launch {
    temp: velnor_test_support::ScratchDir,
    child: Child,
}

impl Drop for Launch {
    fn drop(&mut self) {
        killpg(pid_of(&self.child), Signal::SIGKILL).ok();
        if let Some(pid) = list_pid(self.temp.path()) {
            kill(pid, Signal::SIGKILL).ok();
        }
        self.child.wait().ok();
    }
}

fn pid_of(child: &Child) -> Pid {
    Pid::from_raw(i32::try_from(child.id()).unwrap_or_else(|e| unreachable!("{e}")))
}

fn list_pid(temp: &Path) -> Option<Pid> {
    fs::read_to_string(temp.join("list-pid"))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .map(Pid::from_raw)
}

fn gone(pid: Pid) -> bool {
    kill(pid, None) == Err(Errno::ESRCH)
}

fn launch(name: &str, list: &str, run: &str, environment: &[(&str, &str)]) -> Launch {
    let temp = scratch(name);
    archive_in(temp.path());
    let program = temp.join("cargo-nextest");
    let script = format!("#!/bin/sh\nif [ \"$2\" = list ]; then\n{list}\nfi\n{run}");
    fs::write(&program, script).unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    fs::write(temp.join("github-output"), "existing=1\n").unwrap_or_else(|e| unreachable!("{e}"));
    let path = format!(
        "{}:{}",
        temp.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let mut command = velnor_ci_run();
    command
        .args([
            "run",
            "--",
            "cargo-nextest",
            "nextest",
            "run",
            "--archive-file",
            "a.tar.zst",
            "--no-tests=pass",
        ])
        .current_dir(temp.path())
        .env("RUNNER_TEMP", temp.path())
        .env("GITHUB_OUTPUT", temp.join("github-output"))
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(
            fs::File::create(temp.join("velnor-stderr")).unwrap_or_else(|e| unreachable!("{e}")),
        ))
        .process_group(0);
    for (name, value) in environment {
        command.env(name, value);
    }
    let child = command.spawn().unwrap_or_else(|e| unreachable!("{e}"));
    Launch { temp, child }
}

fn finish(launch: &mut Launch, limit: Duration) -> (bool, String, String) {
    let deadline = Instant::now() + limit;
    let exited = loop {
        if launch.child.try_wait().ok().flatten().is_some() {
            break true;
        }
        if Instant::now() >= deadline {
            break false;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    if !exited {
        killpg(pid_of(&launch.child), Signal::SIGKILL).ok();
        launch.child.wait().ok();
    }
    let bytes = fs::read(launch.temp.join("velnor-stderr")).unwrap_or_default();
    let written = fs::read_to_string(launch.temp.join("github-output")).unwrap_or_default();
    (
        exited,
        String::from_utf8_lossy(&bytes).into_owned(),
        written,
    )
}

fn assert_withheld(written: &str) {
    assert!(written.contains("tests_run=1\n"), "{written}");
    for name in WITHHELD {
        assert!(!written.contains(name), "{name} leaked into {written}");
    }
}

fn assert_list_stopped(launch: &Launch) {
    let pid = list_pid(launch.temp.path()).unwrap_or_else(|| unreachable!("the list never ran"));
    let deadline = Instant::now() + Duration::from_secs(3);
    while !gone(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(gone(pid), "the list child {pid} outlived velnor-ci");
}

#[test]
fn a_list_that_hangs_is_stopped_a_grace_after_the_run_ends() {
    let mut run = launch(
        "early-bound-hang",
        &format!("{RECORD_LIST_PID}exec sleep 300"),
        &format!("{RUN_START}{WAIT_FOR_LIST_PID}{RUN_END}"),
        &[(GRACE_VARIABLE, "300")],
    );
    let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "velnor-ci waited on the hung list: {stderr}");
    assert!(
        run.child
            .try_wait()
            .ok()
            .flatten()
            .is_some_and(|status| status.success())
    );
    assert_withheld(&written);
    assert!(
        stderr.contains(
            "velnor-ci: the early nextest list failed: it did not finish within 300 ms of the run ending"
        ),
        "{stderr}"
    );
    assert_list_stopped(&run);
}

#[test]
fn a_list_still_working_inside_the_grace_is_kept() {
    let json = listed(&[("kept", &["alpha"])]);
    let mut run = launch(
        "early-bound-slow",
        &format!("sleep 0.5\nprintf '%s\\n' '{json}'"),
        &format!("{RUN_START}{RUN_END}"),
        &[(GRACE_VARIABLE, PATIENT)],
    );
    let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "{stderr}");
    assert!(written.contains("tests_by_package=kept=1/1"), "{written}");
}

#[test]
fn a_list_that_prints_more_than_the_output_limit_is_stopped() {
    let mut run = launch(
        "early-bound-stdout",
        &format!("{RECORD_LIST_PID}exec head -c 1000000 /dev/zero"),
        &format!("{RUN_START}{WAIT_FOR_LIST_PID}{RUN_END}"),
        &[(GRACE_VARIABLE, PATIENT), (STDOUT_LIMIT_VARIABLE, "1000")],
    );
    let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "{stderr}");
    assert_withheld(&written);
    assert!(
        stderr.contains(
            "velnor-ci: the early nextest list failed: its listing is longer than 1000 bytes"
        ),
        "{stderr}"
    );
    assert_list_stopped(&run);
}

#[test]
fn a_failing_list_that_floods_stderr_is_echoed_bounded() {
    let mut run = launch(
        "early-bound-stderr",
        "head -c 3000000 /dev/zero | tr '\\0' x >&2\nexit 3",
        &format!("{RUN_START}{RUN_END}"),
        &[(GRACE_VARIABLE, PATIENT)],
    );
    let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "{}", stderr.len());
    assert_withheld(&written);
    assert!(stderr.contains("exit code 3"), "{}", stderr.len());
    assert!(stderr.len() < 2_000_000, "{}", stderr.len());
}

#[test]
fn a_signal_stops_the_list_with_the_run() {
    for terminal in ["0", "1"] {
        for signal in [Signal::SIGTERM, Signal::SIGINT] {
            let name = format!("early-bound-signal-{terminal}-{}", signal as i32);
            let mut run = launch(
                &name,
                &format!("{RECORD_LIST_PID}exec sleep 300"),
                &format!("{RUN_START}exec sleep 300"),
                &[(GRACE_VARIABLE, PATIENT), (TERMINAL_VARIABLE, terminal)],
            );
            wait_for(&run.temp.join("list-pid"));
            kill(pid_of(&run.child), signal).ok();
            let (exited, stderr, _) = finish(&mut run, Duration::from_secs(10));
            assert!(
                exited,
                "{signal} with terminal {terminal}: velnor-ci outlived the signal: {stderr}"
            );
            assert_list_stopped(&run);
        }
    }
}

fn wait_for(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !path.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_signal_after_the_run_ended_still_stops_the_list() {
    let mut run = launch(
        "early-bound-signal-late",
        &format!("{RECORD_LIST_PID}exec sleep 300"),
        &format!("{RUN_START}{WAIT_FOR_LIST_PID}{RUN_END}"),
        &[(GRACE_VARIABLE, PATIENT)],
    );
    wait_for(&run.temp.join("list-pid"));
    std::thread::sleep(Duration::from_secs(1));
    kill(pid_of(&run.child), Signal::SIGTERM).ok();
    let (exited, stderr, _) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "velnor-ci outlived the signal: {stderr}");
    assert_list_stopped(&run);
}

#[test]
fn a_descendant_that_outlives_the_list_is_stopped_with_it() {
    let mut run = launch(
        "early-bound-descendant",
        "sleep 300 &\necho $! > \"$RUNNER_TEMP/list-pid\"\nexit 0",
        &format!("{RUN_START}{WAIT_FOR_LIST_PID}{RUN_END}"),
        &[(GRACE_VARIABLE, "300")],
    );
    let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "{stderr}");
    assert_withheld(&written);
    assert_list_stopped(&run);
}

#[test]
fn a_signal_before_the_list_starts_is_reported() {
    let mut run = launch(
        "early-bound-signal-early",
        "exit 0",
        "echo started > \"$RUNNER_TEMP/run-started\"\nexec sleep 300",
        &[(GRACE_VARIABLE, PATIENT)],
    );
    wait_for(&run.temp.join("run-started"));
    kill(pid_of(&run.child), Signal::SIGTERM).ok();
    let (exited, stderr, _) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "{stderr}");
    assert!(
        stderr.contains("velnor-ci: the early nextest list failed: velnor-ci received a signal"),
        "{stderr}"
    );
}

#[test]
fn a_list_nobody_waits_for_is_stopped_at_once() {
    let mut run = launch(
        "early-bound-unwanted",
        &format!("{RECORD_LIST_PID}exec sleep 300"),
        &format!("{RUN_START}{WAIT_FOR_LIST_PID}"),
        &[(GRACE_VARIABLE, PATIENT)],
    );
    let (exited, stderr, written) = finish(&mut run, Duration::from_secs(10));
    assert!(
        exited,
        "velnor-ci waited for a listing it discards: {stderr}"
    );
    assert!(!written.contains("tests_by_package="), "{written}");
    assert_list_stopped(&run);
}

#[test]
fn a_failure_before_the_list_is_collected_still_stops_it() {
    let mut run = launch(
        "early-bound-early-return",
        &format!("{RECORD_LIST_PID}exec sleep 300"),
        &format!("{RUN_START}{WAIT_FOR_LIST_PID}{RUN_END}"),
        &[
            (GRACE_VARIABLE, PATIENT),
            ("GITHUB_OUTPUT", "/nonexistent-directory/output"),
        ],
    );
    let (exited, stderr, _) = finish(&mut run, Duration::from_secs(10));
    assert!(exited, "{stderr}");
    assert!(stderr.contains("cannot write the step output"), "{stderr}");
    assert_list_stopped(&run);
}
