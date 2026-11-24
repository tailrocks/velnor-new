// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::early_list_child::Supervisor;
use super::early_list_dir::{ExecProbe, ListDir, can_execute_from};
use crate::tests_run::{Listing, list_argv, runnable_matches};

const EXPANSION_HEADROOM: u64 = 8;
const GRACE: Duration = Duration::from_secs(60);
const GRACE_VARIABLE: &str = "VELNOR_CI_EARLY_LIST_GRACE_MS";
const STOP_GRACE: Duration = Duration::from_secs(5);
const STDOUT_LIMIT: u64 = 128 << 20;
const STDOUT_LIMIT_VARIABLE: &str = "VELNOR_CI_EARLY_LIST_STDOUT_LIMIT_BYTES";

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
    Running(Receiver<Option<Listing>>),
    Taken,
}

pub(super) struct EarlyList {
    argv: Vec<String>,
    grace: Duration,
    supervisor: Arc<Supervisor>,
    state: Mutex<State>,
}

impl EarlyList {
    pub(super) fn new(run: &[String]) -> Self {
        Self {
            argv: run.to_vec(),
            grace: env_number(GRACE_VARIABLE).map_or(GRACE, Duration::from_millis),
            supervisor: Arc::new(Supervisor::default()),
            state: Mutex::new(State::Idle),
        }
    }

    pub(super) fn start(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*state, State::Idle) {
            *state = State::Running(self.spawn());
        }
    }

    pub(super) fn stop(&self, reason: &str) {
        self.supervisor.stop(reason);
    }

    pub(super) fn finish(&self, wanted: bool) -> Option<Listing> {
        let state = std::mem::replace(
            &mut *self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            State::Taken,
        );
        let receiver = match state {
            State::Running(receiver) => {
                if !wanted {
                    self.supervisor
                        .stop("the run's output was incomplete, so the listing was not needed");
                }
                receiver
            }
            State::Idle if wanted => self.spawn(),
            State::Idle => {
                if let Some(reason) = self.supervisor.reason() {
                    report(&reason);
                }
                return None;
            }
            State::Taken => return None,
        };
        match receiver.recv_timeout(self.grace) {
            Ok(listing) => listing,
            Err(RecvTimeoutError::Timeout) => {
                let reason = format!(
                    "it did not finish within {} ms of the run ending",
                    self.grace.as_millis()
                );
                self.supervisor.stop(reason.clone());
                receiver.recv_timeout(STOP_GRACE).unwrap_or_else(|_| {
                    report(&reason);
                    None
                })
            }
            Err(RecvTimeoutError::Disconnected) => {
                report("its thread panicked");
                None
            }
        }
    }

    fn spawn(&self) -> Receiver<Option<Listing>> {
        let (sender, receiver) = mpsc::channel();
        let argv = self.argv.clone();
        let supervisor = Arc::clone(&self.supervisor);
        thread::spawn(move || sender.send(list_matches(&argv, supervisor)).ok());
        receiver
    }
}

pub(super) struct StopOnDrop(Arc<EarlyList>);

impl StopOnDrop {
    pub(super) fn new(early: &Arc<EarlyList>) -> Self {
        Self(Arc::clone(early))
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        self.0
            .stop("velnor-ci returned before it collected the list");
    }
}

fn env_number(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

fn report(reason: &str) {
    eprintln!(
        "velnor-ci: the early nextest list failed: {reason}; the step reports no tests_listed, tests_by_package or tests_ignored"
    );
}

fn list_matches(argv: &[String], supervisor: Arc<Supervisor>) -> Option<Listing> {
    match try_list(
        argv,
        &Environment {
            runner_temp: std::env::var_os("RUNNER_TEMP"),
            stdout_limit: env_number(STDOUT_LIMIT_VARIABLE).unwrap_or(STDOUT_LIMIT),
            supervisor,
        },
        &can_execute_from,
        &free_bytes,
    ) {
        Ok(listing) => Some(listing),
        Err(failure) => {
            drop(std::io::stderr().write_all(&failure.stderr));
            report(&failure.reason);
            None
        }
    }
}

#[derive(Debug)]
struct Failure {
    reason: String,
    stderr: Vec<u8>,
}

impl Failure {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            stderr: Vec::new(),
        }
    }
}

struct Environment {
    runner_temp: Option<OsString>,
    stdout_limit: u64,
    supervisor: Arc<Supervisor>,
}

fn try_list(
    argv: &[String],
    environment: &Environment,
    can_execute: &ExecProbe,
    free: &FreeProbe,
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
    let output = environment
        .supervisor
        .run(&mut command, environment.stdout_limit)
        .map_err(|error| Failure::new(format!("cannot start it: {error}")))?;
    drop(directory);
    if let Some(reason) = output.stopped {
        return Err(Failure {
            reason,
            stderr: output.stderr,
        });
    }
    if !output.status.success() {
        return Err(Failure {
            reason: output.status.code().map_or_else(
                || output.status.to_string(),
                |code| format!("exit code {code}"),
            ),
            stderr: output.stderr,
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
