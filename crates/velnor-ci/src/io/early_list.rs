// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use super::early_list_dir::{ExecProbe, ListDir, can_execute_from};
use crate::bounded::{Outcome, capture_streams};
use crate::tests_run::{Listing, list_argv, runnable_matches};

const EXPANSION_HEADROOM: u64 = 8;
const LIST_GRACE: Duration = Duration::from_secs(60);
const JOIN_SLACK: Duration = Duration::from_secs(10);
const STDOUT_CEILING: usize = 128 << 20;
const STDERR_KEEP: usize = 64 << 10;
const GRACE_VARIABLE: &str = "VELNOR_CI_RUN_EARLY_LIST_GRACE_MS";
const CEILING_VARIABLE: &str = "VELNOR_CI_RUN_EARLY_LIST_MAX_BYTES";

fn grace() -> Duration {
    std::env::var(GRACE_VARIABLE)
        .ok()
        .and_then(|millis| millis.parse().ok())
        .map_or(LIST_GRACE, Duration::from_millis)
}

fn ceiling() -> usize {
    std::env::var(CEILING_VARIABLE)
        .ok()
        .and_then(|bytes| bytes.parse().ok())
        .unwrap_or(STDOUT_CEILING)
}

struct Bounds<'a> {
    stdout_bytes: usize,
    grace: Duration,
    deadline: &'a dyn Fn() -> Option<Instant>,
}

type FreeProbe = dyn Fn(&Path) -> io::Result<u64>;

fn free_bytes(directory: &Path) -> io::Result<u64> {
    let stats = nix::sys::statvfs::statvfs(directory).map_err(io::Error::from)?;
    Ok(widen(stats.blocks_available()).saturating_mul(widen(stats.fragment_size())))
}

fn widen<T: Into<u64>>(value: T) -> u64 {
    value.into()
}

const STEP_FILE_VARIABLES: &[&str] = &[
    "GITHUB_OUTPUT",
    "GITHUB_ENV",
    "GITHUB_PATH",
    "GITHUB_STEP_SUMMARY",
    "GITHUB_STATE",
];

enum State {
    Idle,
    Running(mpsc::Receiver<Option<Listing>>),
    Taken,
}

pub(super) struct EarlyList {
    argv: Vec<String>,
    state: Mutex<State>,
    run_ended: Arc<OnceLock<Instant>>,
}

impl EarlyList {
    pub(super) fn new(run: &[String]) -> Self {
        Self {
            argv: run.to_vec(),
            state: Mutex::new(State::Idle),
            run_ended: Arc::new(OnceLock::new()),
        }
    }

    pub(super) fn start(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*state, State::Idle) {
            let argv = self.argv.clone();
            let ended = Arc::clone(&self.run_ended);
            let (sender, receiver) = mpsc::channel();
            thread::spawn(move || sender.send(list_matches(&argv, &ended)).ok());
            *state = State::Running(receiver);
        }
    }

    pub(super) fn finish(&self, wanted: bool) -> Option<Listing> {
        self.finish_within(wanted, grace() + JOIN_SLACK)
    }

    fn finish_within(&self, wanted: bool, bound: Duration) -> Option<Listing> {
        self.run_ended.get_or_init(Instant::now);
        let state = std::mem::replace(
            &mut *self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            State::Taken,
        );
        match state {
            State::Running(receiver) => match receiver.recv_timeout(bound) {
                Ok(listing) => listing,
                Err(RecvTimeoutError::Timeout) => {
                    eprintln!(
                        "::warning::early test listing did not finish within {} s after the test run finished; continuing without test counts",
                        bound.as_secs_f64()
                    );
                    None
                }
                Err(RecvTimeoutError::Disconnected) => {
                    eprintln!(
                        "velnor-ci: the early nextest list failed: its thread panicked; the step reports no tests_listed, tests_by_package or tests_ignored"
                    );
                    None
                }
            },
            State::Idle if wanted => list_matches(&self.argv, &self.run_ended),
            State::Idle | State::Taken => None,
        }
    }
}

fn list_matches(argv: &[String], ended: &OnceLock<Instant>) -> Option<Listing> {
    let grace = grace();
    match try_list(
        argv,
        &Environment {
            runner_temp: std::env::var_os("RUNNER_TEMP"),
        },
        &can_execute_from,
        &free_bytes,
        &Bounds {
            stdout_bytes: ceiling(),
            grace,
            deadline: &|| ended.get().map(|at| *at + grace),
        },
    ) {
        Ok(listing) => Some(listing),
        Err(failure) => {
            drop(std::io::stderr().write_all(&failure.stderr));
            if failure.bounded {
                eprintln!(
                    "::warning::early test listing {}; continuing without test counts",
                    failure.reason
                );
            } else {
                eprintln!(
                    "velnor-ci: the early nextest list failed: {}; the step reports no tests_listed, tests_by_package or tests_ignored",
                    failure.reason
                );
            }
            None
        }
    }
}

#[derive(Debug)]
struct Failure {
    reason: String,
    stderr: Vec<u8>,
    bounded: bool,
}

impl Failure {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            stderr: Vec::new(),
            bounded: false,
        }
    }

    fn bounded(reason: String) -> Self {
        Self {
            bounded: true,
            ..Self::new(reason)
        }
    }
}

struct Environment {
    runner_temp: Option<OsString>,
}

fn try_list(
    argv: &[String],
    environment: &Environment,
    can_execute: &ExecProbe,
    free: &FreeProbe,
    bounds: &Bounds<'_>,
) -> Result<Listing, Failure> {
    let mut argv =
        list_argv(argv).ok_or_else(|| Failure::new("the command is not a nextest run"))?;
    let archive = archive_file(&argv);
    let directory = if let Some(archive) = archive {
        Some(
            extraction_directory(
                &mut argv,
                &archive,
                environment.runner_temp.clone(),
                can_execute,
                free,
            )
            .map_err(|error| {
                Failure::new(format!(
                    "cannot prepare its own extraction directory: {error}"
                ))
            })?,
        )
    } else {
        None
    };
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    for name in STEP_FILE_VARIABLES.iter().copied() {
        command.env_remove(name);
    }
    super::mbx_reports::assign(&mut command, &argv);
    let outcome = capture_streams(
        &mut command,
        bounds.stdout_bytes,
        STDERR_KEEP,
        bounds.deadline,
    )
    .map_err(|error| Failure::new(format!("cannot start it: {error}")))?;
    drop(directory);
    let output = match outcome {
        Outcome::Exited(output) => output,
        Outcome::TimedOut => {
            return Err(Failure::bounded(format!(
                "exceeded {} s after the test run finished",
                bounds.grace.as_secs_f64()
            )));
        }
        Outcome::Oversized => {
            return Err(Failure::bounded(format!(
                "exceeded {} bytes",
                bounds.stdout_bytes
            )));
        }
    };
    if !output.status.success() {
        return Err(Failure {
            reason: output.status.code().map_or_else(
                || output.status.to_string(),
                |code| format!("exit code {code}"),
            ),
            stderr: output.stderr,
            bounded: false,
        });
    }
    runnable_matches(&output.stdout).ok_or_else(|| Failure::new("it printed no usable listing"))
}

fn extraction_directory(
    argv: &mut Vec<String>,
    archive: &str,
    runner_temp: Option<OsString>,
    can_execute: &ExecProbe,
    free: &FreeProbe,
) -> Result<ListDir, String> {
    let directory = ListDir::create(runner_temp, std::process::id(), can_execute)?;
    admit(archive, directory.path(), free)?;
    let path = directory
        .path()
        .to_str()
        .ok_or("the directory is not valid UTF-8")?
        .to_owned();
    let at = argv
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(argv.len());
    argv.splice(at..at, ["--extract-to".to_owned(), path]);
    Ok(directory)
}

fn admit(archive: &str, directory: &Path, free: &FreeProbe) -> Result<(), String> {
    let size = fs::metadata(archive)
        .map_err(|error| format!("cannot read the archive size of {archive}: {error}"))?
        .len();
    let needed = size.saturating_mul(EXPANSION_HEADROOM);
    let available = free(directory).map_err(|error| {
        format!(
            "cannot measure the free space of {}: {error}",
            directory.display()
        )
    })?;
    if available < needed {
        return Err(format!(
            "{} has {available} bytes free, but the extraction may need {needed}",
            directory.display()
        ));
    }
    Ok(())
}

fn archive_file(argv: &[String]) -> Option<String> {
    let end = argv
        .iter()
        .position(|arg| arg == "--")
        .unwrap_or(argv.len());
    let argv = &argv[..end];
    argv.iter()
        .enumerate()
        .find_map(|(index, arg)| {
            if arg == "--archive-file" {
                return argv.get(index + 1).map(String::as_str);
            }
            arg.strip_prefix("--archive-file=")
        })
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "early_list_tests.rs"]
mod tests;
