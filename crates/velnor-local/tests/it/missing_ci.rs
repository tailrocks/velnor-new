// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! A run that succeeded keeps `run` successful when velnor-ci is not on the driver's PATH.

use std::path::Path;
use std::process::Command;

use velnor_local::scenario::{Metrics, Outcome, metrics_of};
use velnor_local::verdict::{Check, conclude, verdict};

use crate::support::temp_dir;

fn succeeded(metrics: Metrics) -> Outcome {
    Outcome {
        id: 1,
        workflow: "ci".to_owned(),
        event: "push".to_owned(),
        conclusion: "success",
        started: Some(5),
        metrics,
        caches: Vec::new(),
        trusted: true,
        client_log: None,
    }
}

#[test]
fn a_missing_velnor_ci_is_a_warning_and_the_successful_run_still_passes() {
    let empty = temp_dir("velnor-ci-missing-path");
    let collected = Command::new("velnor-ci").env("PATH", &empty).output();
    let path = empty.join("metrics/1.json");
    let outcomes = [succeeded(metrics_of(
        Path::new("velnor-ci"),
        collected,
        false,
        path,
    ))];
    assert_eq!(conclude(&Check::Plain, &outcomes), Ok(()));
    let text = verdict("leaf", &outcomes, (0, 1.0), &Check::Plain, "");
    assert!(
        text.contains("velnor-ci not found; skipping the report"),
        "{text}"
    );
    assert!(!text.contains("no metrics file"), "{text}");
}

#[test]
fn a_velnor_ci_that_runs_and_fails_still_fails_the_run() {
    let empty = temp_dir("velnor-ci-failing-path");
    let collected = Command::new("sh")
        .args(["-c", "echo broken >&2; exit 3"])
        .output();
    let outcomes = [succeeded(metrics_of(
        Path::new("velnor-ci"),
        collected,
        false,
        empty.join("metrics/1.json"),
    ))];
    assert_eq!(
        conclude(&Check::Plain, &outcomes),
        Err("a run did not succeed")
    );
}
