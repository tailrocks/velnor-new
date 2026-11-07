// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use rustix::process::{Pid, Signal, kill_process_group};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use velnor_actions_mise::{CheckDeadline, IsolatedCommand};

mod support;

use support::{ReleaseOnDrop, deadline, in_fixture_child, run_fixture, shell, shell_with_args};

#[test]
fn anchored_transient_permission_reaches_public_receipt() -> Result<(), Box<dyn std::error::Error>>
{
    if in_fixture_child() {
        let command = shell("printf ready")?;
        let (output, _receipt) =
            command.run_quiescent_until(32, deadline(Duration::from_secs(2))?)?;
        assert!(output.success);
        assert_eq!(output.stdout, b"ready");
        return Ok(());
    }
    run_fixture(
        "anchored_transient_permission_reaches_public_receipt",
        "once",
        |log| {
            assert!(
                log.contains("errno=1"),
                "fixture did not inject EPERM: {log}"
            );
            assert!(log.contains("errno=3"), "real SRCH was not observed: {log}");
        },
    )
}

#[test]
fn anchored_persistent_permission_returns_error_without_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let started = Instant::now();
        let error = shell("printf ready")?
            .run_quiescent_until(32, deadline(Duration::from_secs(2))?)
            .err()
            .ok_or_else(|| io::Error::other("persistent EPERM returned a receipt"))?;
        assert!(
            error
                .to_string()
                .contains("observe_group:Operation not permitted")
        );
        assert!(
            started.elapsed() >= Duration::from_millis(1800),
            "persistent EPERM returned before the drain deadline"
        );
        return Ok(());
    }
    run_fixture(
        "anchored_persistent_permission_returns_error_without_receipt",
        "persistent",
        |log| {
            assert!(
                log.contains("errno=1"),
                "fixture did not inject EPERM: {log}"
            );
        },
    )
}

#[test]
fn anchored_unrelated_observation_error_returns_immediately()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let error = shell("printf ready")?
            .run_quiescent_until(32, deadline(Duration::from_secs(2))?)
            .err()
            .ok_or_else(|| io::Error::other("EINVAL was retried as success"))?;
        assert!(error.to_string().contains("observe_group:Invalid argument"));
        return Ok(());
    }
    run_fixture(
        "anchored_unrelated_observation_error_returns_immediately",
        "other_once",
        |log| {
            assert!(
                log.contains("errno=22"),
                "fixture did not inject EINVAL: {log}"
            );
            assert!(
                !log.contains("errno=3"),
                "unrelated error was followed by observation: {log}"
            );
        },
    )
}

#[test]
fn unanchored_cap_cleanup_retries_permission_and_stops_descendant()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let directory = PathBuf::from(
            std::env::var_os("VELNOR_GROUP_PROBE_DIRECTORY")
                .ok_or_else(|| io::Error::other("fixture directory is missing"))?,
        );
        let ready = directory.join("ready");
        let release = directory.join("release");
        let continued = directory.join("continued");
        let _release_guard = ReleaseOnDrop(release.clone());
        let script = concat!(
            "(touch \"$1\"; attempt=0; while [ \"$attempt\" -lt 400 ]; do ",
            "[ -f \"$2\" ] && break; sleep 0.01; attempt=$((attempt + 1)); ",
            "done; [ -f \"$2\" ] || exit 1; touch \"$3\") & ",
            "attempt=0; while [ \"$attempt\" -lt 400 ]; do ",
            "[ -f \"$1\" ] && break; sleep 0.01; attempt=$((attempt + 1)); ",
            "done; [ -f \"$1\" ] || exit 1; printf overflow"
        );
        let command = shell_with_args(script, &[&ready, &release, &continued])?;
        let error = command
            .run_bounded(1, Duration::from_secs(3))
            .err()
            .ok_or_else(|| io::Error::other("stdout cap did not fail"))?;
        let message = error.to_string();
        assert!(message.contains("stdout_limit_exceeded:1"), "{message}");
        assert!(!message.contains("cleanup_failed:"), "{message}");
        assert!(ready.exists(), "descendant readiness was not observed");
        fs::write(release, b"release")?;
        let until = Instant::now() + Duration::from_millis(500);
        while Instant::now() < until && !continued.exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!continued.exists(), "descendant continued after cleanup");
        return Ok(());
    }
    run_fixture(
        "unanchored_cap_cleanup_retries_permission_and_stops_descendant",
        "once",
        |log| {
            assert!(
                log.contains("errno=1"),
                "fixture did not inject EPERM: {log}"
            );
            assert!(log.contains("errno=3"), "real SRCH was not observed: {log}");
        },
    )
}

#[test]
fn anchored_present_group_exhausts_deadline_without_receipt()
-> Result<(), Box<dyn std::error::Error>> {
    if in_fixture_child() {
        let error = shell("printf ready")?
            .run_quiescent_until(32, deadline(Duration::from_millis(300))?)
            .err()
            .ok_or_else(|| io::Error::other("present group returned a receipt"))?;
        assert!(error.to_string().contains("group_drain_deadline_exhausted"));
        return Ok(());
    }
    run_fixture(
        "anchored_present_group_exhausts_deadline_without_receipt",
        "present",
        |log| {
            assert!(
                log.contains("result=0 errno=0"),
                "fixture did not report group present: {log}"
            );
        },
    )
}
