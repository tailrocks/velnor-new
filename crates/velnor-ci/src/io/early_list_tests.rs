// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Bounds, EarlyList, Environment, Failure, State, try_list};
use crate::tests_run::Listing;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const UNBOUNDED: Bounds<'static> = Bounds {
    stdout_bytes: usize::MAX,
    grace: std::time::Duration::ZERO,
    deadline: &|| None,
};

fn scratch(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("velnor-early-{name}-{}", std::process::id()));
    drop(fs::remove_dir_all(&directory));
    fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    directory
}

fn archive_run(program: &Path) -> Vec<String> {
    let archive = program.with_file_name("a.tar.zst");
    drop(fs::write(&archive, b""));
    [
        &program.display().to_string(),
        "nextest",
        "run",
        "--archive-file",
        &archive.display().to_string(),
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect()
}

fn environment(runner_temp: Option<OsString>) -> Environment {
    Environment { runner_temp }
}

fn report(result: Result<Listing, Failure>) -> String {
    match result {
        Err(failure) => failure.reason,
        other => unreachable!("{other:?}"),
    }
}

#[test]
fn a_listing_that_never_reports_is_abandoned_at_the_join_bound_without_counts() {
    let list = EarlyList::new(&[]);
    let (sender, receiver) = mpsc::channel::<Option<Listing>>();
    *list
        .state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = State::Running(receiver);
    let started = Instant::now();
    let counts = list.finish_within(true, Duration::from_millis(100));
    assert!(counts.is_none());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    drop(sender);
}

#[test]
fn a_missing_runner_temp_is_reported_before_anything_is_started() {
    let message = report(try_list(
        &archive_run(Path::new("/nonexistent/cargo-nextest")),
        &environment(None),
        &|_| Ok(true),
        &|_| Ok(u64::MAX),
        &UNBOUNDED,
    ));
    assert_eq!(
        message,
        "cannot prepare its own extraction directory: RUNNER_TEMP is not set"
    );
}

#[test]
fn a_runner_temp_that_is_a_file_is_reported_with_the_directory_it_could_not_prepare() {
    let directory = scratch("file-as-temp");
    let temp = directory.join("not-a-directory");
    fs::write(&temp, b"").unwrap_or_else(|e| unreachable!("{e}"));
    let message = report(try_list(
        &archive_run(Path::new("/nonexistent/cargo-nextest")),
        &environment(Some(OsString::from(&temp))),
        &|_| Ok(true),
        &|_| Ok(u64::MAX),
        &UNBOUNDED,
    ));
    let expected = temp.join(format!("velnor-nextest-list-{}", std::process::id()));
    assert!(
        message.starts_with("cannot prepare its own extraction directory: cannot "),
        "{message}"
    );
    assert!(
        message.contains(&expected.display().to_string()),
        "{message}"
    );
    fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn an_extraction_larger_than_the_free_space_is_refused_before_the_list_runs() {
    let directory = scratch("no-room");
    let archive = directory.join("a.tar.zst");
    fs::write(&archive, vec![0_u8; 100]).unwrap_or_else(|e| unreachable!("{e}"));
    let run = [
        "/nonexistent/cargo-nextest".to_owned(),
        "nextest".to_owned(),
        "run".to_owned(),
        "--archive-file".to_owned(),
        archive.display().to_string(),
    ];
    let message = report(try_list(
        &run,
        &environment(Some(OsString::from(&directory))),
        &|_| Ok(true),
        &|_| Ok(799),
        &UNBOUNDED,
    ));
    assert!(
        message.contains("bytes free, but the extraction may need 800"),
        "{message}"
    );
    let list_dir = directory.join(format!("velnor-nextest-list-{}", std::process::id()));
    assert!(!list_dir.exists(), "{}", list_dir.display());
    fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn an_extraction_that_fits_the_headroom_is_admitted() {
    let directory = scratch("fits");
    let archive = directory.join("a.tar.zst");
    fs::write(&archive, vec![0_u8; 100]).unwrap_or_else(|e| unreachable!("{e}"));
    let program = directory.join("cargo-nextest");
    fs::write(&program, "#!/bin/sh\nexit 0\n").unwrap_or_else(|e| unreachable!("{e}"));
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let run = [
        program.display().to_string(),
        "nextest".to_owned(),
        "run".to_owned(),
        "--archive-file".to_owned(),
        archive.display().to_string(),
    ];
    let message = report(try_list(
        &run,
        &environment(Some(OsString::from(&directory))),
        &|_| Ok(true),
        &|_| Ok(800),
        &UNBOUNDED,
    ));
    assert_eq!(message, "it printed no usable listing");
    fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_destination_that_does_not_allow_execution_is_reported_before_the_list_starts() {
    let directory = scratch("noexec");
    let message = report(try_list(
        &archive_run(Path::new("/nonexistent/cargo-nextest")),
        &environment(Some(OsString::from(&directory))),
        &|_| Ok(false),
        &|_| Ok(u64::MAX),
        &UNBOUNDED,
    ));
    assert!(
        message.contains("does not allow executing files"),
        "{message}"
    );
    assert!(
        message.contains(&directory.display().to_string()),
        "{message}"
    );
    fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
}
