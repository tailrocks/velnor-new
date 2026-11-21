// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::process::Command;
use std::time::{Duration, Instant};

use velnor_ci::bounded::{Outcome, capture, capture_streams};

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
    let outcome = capture_streams(&mut command, 1000, 100, &|| None);
    assert!(matches!(outcome, Ok(Outcome::Oversized)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn streams_keep_only_the_stderr_ceiling_but_drain_the_rest_so_the_child_finishes() {
    let mut command = Command::new("sh");
    command.args(["-c", "head -c 1048576 /dev/zero >&2; printf ok"]);
    let outcome = capture_streams(&mut command, 1000, 100, &|| None);
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
    let outcome = capture_streams(&mut command, 1000, 100, &move || Some(at));
    assert!(matches!(outcome, Ok(Outcome::TimedOut)), "{outcome:?}");
    assert!(started.elapsed() < Duration::from_secs(15));
}
