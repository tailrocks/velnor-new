// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::ErrorKind;
use std::process::Command;
use std::time::{Duration, Instant};

use velnor_ci::bounded::{Cancel, Outcome, capture, capture_streams};

const LONG: Duration = Duration::from_secs(60);

#[test]
fn output_is_captured_up_to_the_limit_and_the_child_is_not_waited_on_for_more() {
    let started = Instant::now();
    let mut command = Command::new("yes");
    let captured = capture(&mut command, 1000, LONG).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(captured.stdout.len(), 1001);
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn a_child_that_finishes_inside_the_limit_reports_its_whole_output_and_status() {
    let mut command = Command::new("sh");
    command.args(["-c", "printf abc; exit 3"]);
    let captured = capture(&mut command, 1000, LONG).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(captured.stdout, b"abc");
    assert!(!captured.success);
}

#[test]
fn a_child_that_outlives_the_timeout_is_killed_and_reported() {
    let started = Instant::now();
    let mut command = Command::new("sleep");
    command.arg("600");
    let error = capture(&mut command, 1000, Duration::from_millis(200))
        .err()
        .map(|error| error.kind());
    assert_eq!(error, Some(ErrorKind::TimedOut));
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn streams_over_the_stdout_limit_stop_the_child_even_when_it_ignores_a_closed_pipe() {
    let started = Instant::now();
    let mut command = Command::new("sh");
    command.args(["-c", "trap '' PIPE; yes; sleep 30"]);
    let outcome = capture_streams(&mut command, 1000, 100, &|| None, &Cancel::new());
    assert!(matches!(outcome, Ok(Outcome::Oversized)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn streams_keep_only_the_stderr_ceiling_but_drain_the_rest_so_the_child_finishes() {
    let mut command = Command::new("sh");
    command.args(["-c", "head -c 1048576 /dev/zero >&2; printf ok"]);
    let outcome = capture_streams(&mut command, 1000, 100, &|| None, &Cancel::new());
    let Ok(Outcome::Exited(streams)) = outcome else {
        unreachable!("{outcome:?}")
    };
    assert_eq!(streams.stdout, b"ok");
    assert_eq!(streams.stderr.len(), 100);
    assert!(streams.status.success());
}

#[test]
fn streams_kill_a_child_past_the_deadline_and_report_it() {
    let started = Instant::now();
    let at = started + Duration::from_millis(200);
    let mut command = Command::new("sleep");
    command.arg("600");
    let outcome = capture_streams(&mut command, 1000, 100, &move || Some(at), &Cancel::new());
    assert!(matches!(outcome, Ok(Outcome::TimedOut)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(15));
}

fn gone(pid: &str) -> bool {
    let stat = Command::new("ps")
        .args(["-o", "stat=", "-p", pid])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let stat = String::from_utf8_lossy(&stat.stdout);
    stat.trim().is_empty() || stat.trim_start().starts_with('Z')
}

#[test]
fn streams_past_the_deadline_take_the_whole_process_group_down_with_the_child() {
    let started = Instant::now();
    let pid_file = crate::scratch_path("velnor-ci-bounded", "grandchild-pid");
    fs::remove_file(&pid_file).ok();
    let mut command = Command::new("sh");
    command.arg("-c").arg(format!(
        "sleep 30 & echo $! > '{}'; wait",
        pid_file.display()
    ));
    let ready = pid_file.clone();
    let outcome = capture_streams(
        &mut command,
        1000,
        100,
        &move || {
            fs::read_to_string(&ready)
                .ok()
                .filter(|pid| pid.ends_with('\n'))
                .map(|_| Instant::now())
        },
        &Cancel::new(),
    );
    assert!(matches!(outcome, Ok(Outcome::TimedOut)), "{outcome:?}");
    let pid = fs::read_to_string(&pid_file)
        .unwrap_or_else(|e| unreachable!("{e}"))
        .trim()
        .to_owned();
    fs::remove_file(&pid_file).ok();
    let limit = Instant::now() + Duration::from_secs(10);
    while !gone(&pid) && Instant::now() < limit {
        std::thread::yield_now();
    }
    assert!(gone(&pid), "grandchild {pid} survived the deadline kill");
    assert!(started.elapsed() < Duration::from_secs(15));
}

fn started_pid(file: &std::path::Path) -> Option<String> {
    fs::read_to_string(file)
        .ok()
        .filter(|pid| pid.ends_with('\n'))
        .map(|pid| pid.trim().to_owned())
}

fn assert_gone_soon(pid: &str, what: &str) {
    let limit = Instant::now() + Duration::from_secs(10);
    while !gone(pid) && Instant::now() < limit {
        std::thread::yield_now();
    }
    let survived = !gone(pid);
    if survived {
        Command::new("kill").args(["-9", pid]).status().ok();
    }
    assert!(!survived, "grandchild {pid} survived {what}");
}

#[test]
fn a_child_that_exits_while_a_descendant_holds_its_output_is_stopped_with_it() {
    let started = Instant::now();
    let pid_file = crate::scratch_path("velnor-ci-bounded", "holder-pid");
    fs::remove_file(&pid_file).ok();
    let mut command = Command::new("sh");
    command.arg("-c").arg(format!(
        "sleep 30 & echo $! > '{}'; exit 0",
        pid_file.display()
    ));
    let outcome = capture_streams(&mut command, 1000, 100, &|| None, &Cancel::new());
    assert!(matches!(outcome, Ok(Outcome::Lingering)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(9));
    let pid = started_pid(&pid_file).unwrap_or_else(|| unreachable!("no pid"));
    fs::remove_file(&pid_file).ok();
    assert_gone_soon(&pid, "the post-exit drain timeout");
}

#[test]
fn a_cancel_stops_the_child_and_its_descendants_without_waiting_for_the_deadline() {
    let started = Instant::now();
    let pid_file = crate::scratch_path("velnor-ci-bounded", "cancel-pid");
    fs::remove_file(&pid_file).ok();
    let mut command = Command::new("sh");
    command.arg("-c").arg(format!(
        "trap '' TERM; sleep 30 & echo $! > '{}'; wait",
        pid_file.display()
    ));
    let cancel = std::sync::Arc::new(Cancel::new());
    let trigger = std::sync::Arc::clone(&cancel);
    let watched = pid_file.clone();
    let canceller = std::thread::spawn(move || {
        while started_pid(&watched).is_none() {
            std::thread::yield_now();
        }
        trigger.cancel(nix::sys::signal::Signal::SIGTERM);
    });
    let outcome = capture_streams(&mut command, 1000, 100, &|| None, &cancel);
    canceller
        .join()
        .unwrap_or_else(|_| unreachable!("canceller"));
    assert!(matches!(outcome, Ok(Outcome::Cancelled)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(9));
    let pid = started_pid(&pid_file).unwrap_or_else(|| unreachable!("no pid"));
    fs::remove_file(&pid_file).ok();
    assert_gone_soon(&pid, "the cancel");
}

#[test]
fn a_cancel_set_before_the_start_never_runs_the_command() {
    let marker = crate::scratch_path("velnor-ci-bounded", "never-ran");
    fs::remove_file(&marker).ok();
    let cancel = Cancel::new();
    cancel.cancel(nix::sys::signal::Signal::SIGKILL);
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("echo ran > '{}'", marker.display()));
    let outcome = capture_streams(&mut command, 1000, 100, &|| None, &cancel);
    assert!(matches!(outcome, Ok(Outcome::Cancelled)), "{outcome:?}");
    assert!(!marker.exists());
}
