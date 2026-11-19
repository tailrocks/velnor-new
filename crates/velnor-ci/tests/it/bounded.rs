// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::ErrorKind;
use std::process::Command;
use std::time::{Duration, Instant};

use velnor_ci::bounded::capture;

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
