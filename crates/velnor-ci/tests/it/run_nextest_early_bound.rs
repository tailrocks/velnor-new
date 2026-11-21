// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use crate::run_nextest::listed;
use crate::run_nextest_extract::{Scenario, archive_run, assert_withheld};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::fs;
use std::time::{Duration, Instant};

const GRACE: (&str, &str) = ("VELNOR_CI_RUN_EARLY_LIST_GRACE_MS", "300");
const CEILING: (&str, &str) = ("VELNOR_CI_RUN_EARLY_LIST_MAX_BYTES", "1000");

#[test]
fn a_list_that_hangs_is_killed_after_the_run_and_the_run_returns_without_counts() {
    let mut scenario = Scenario::failing("echo $$ > \"$RUNNER_TEMP/list-pid\"\nexec sleep 30");
    scenario.settings = vec![GRACE];
    let started = Instant::now();
    let (output, written, temp) = scenario.execute(
        "tests-run-early-hang",
        &archive_run(&["--extract-to", "."]),
        None,
    );
    let elapsed = started.elapsed();
    assert!(output.status.success(), "{output:?}");
    assert!(elapsed < Duration::from_secs(15), "{elapsed:?}");
    assert_withheld(&written);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "::warning::early test listing exceeded 0.3 s after the test run finished; continuing without test counts"
        ),
        "{stderr}"
    );
    let pid: i32 = fs::read_to_string(temp.join("list-pid"))
        .unwrap_or_else(|e| unreachable!("{e}"))
        .trim()
        .parse()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(kill(Pid::from_raw(pid), None), Err(Errno::ESRCH));
}

#[test]
fn a_listing_over_the_byte_ceiling_is_discarded_and_withholds_every_count() {
    let names: Vec<String> = (0..200).map(|index| format!("case_{index}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut scenario = Scenario::listing(&listed(&[("kept", &names)]));
    scenario.settings = vec![CEILING];
    let (output, written, _) = scenario.execute(
        "tests-run-early-oversized",
        &archive_run(&["--extract-to", "."]),
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert_withheld(&written);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(
            "::warning::early test listing exceeded 1000 bytes; continuing without test counts"
        ),
        "{stderr}"
    );
}

#[test]
fn a_normal_listing_with_a_noisy_stderr_keeps_its_counts() {
    let mut scenario = Scenario::listing(&listed(&[("kept", &["alpha", "beta"])]));
    scenario.list = format!("head -c 1048576 /dev/zero >&2\n{}", scenario.list);
    scenario.settings = vec![GRACE, ("VELNOR_CI_RUN_EARLY_LIST_MAX_BYTES", "100000")];
    let (output, written, _) = scenario.execute(
        "tests-run-early-normal",
        &archive_run(&["--extract-to", "."]),
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert!(written.contains("tests_by_package=kept=1/2"), "{written}");
    assert!(written.contains("tests_listed=2"), "{written}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("::warning::early test listing"),
        "{stderr}"
    );
}
