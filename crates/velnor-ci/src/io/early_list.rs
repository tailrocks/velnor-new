// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use super::early_list_dir::{ExecProbe, ListDir, can_execute_from};
use crate::tests_run::{Listing, extracts, list_argv, retarget_extract, runnable_matches};

const EXPANSION_HEADROOM: u64 = 8;

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
    Running(JoinHandle<Option<Listing>>),
    Taken,
}

pub(super) struct EarlyList {
    argv: Vec<String>,
    state: Mutex<State>,
}

impl EarlyList {
    pub(super) fn new(run: &[String]) -> Self {
        Self {
            argv: run.to_vec(),
            state: Mutex::new(State::Idle),
        }
    }

    pub(super) fn start(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if matches!(*state, State::Idle) {
            let argv = self.argv.clone();
            *state = State::Running(thread::spawn(move || list_matches(&argv)));
        }
    }

    pub(super) fn finish(&self, wanted: bool) -> Option<Listing> {
        let state = std::mem::replace(
            &mut *self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            State::Taken,
        );
        match state {
            State::Running(handle) => handle.join().ok().flatten(),
            State::Idle if wanted => list_matches(&self.argv),
            State::Idle | State::Taken => None,
        }
    }
}

fn list_matches(argv: &[String]) -> Option<Listing> {
    match try_list(
        argv,
        std::env::var_os("RUNNER_TEMP"),
        &can_execute_from,
        &free_bytes,
    ) {
        Ok(listing) => Some(listing),
        Err(Failure::NotListable) => None,
        Err(Failure::Exited { status, stderr }) => {
            drop(std::io::stderr().write_all(&stderr));
            eprintln!(
                "velnor-ci: the early nextest list exited with {status}; the step reports no per-package counts"
            );
            None
        }
        Err(Failure::Report(message)) => {
            eprintln!("velnor-ci: {message}; the step reports no per-package counts");
            None
        }
    }
}

#[derive(Debug)]
enum Failure {
    NotListable,
    Exited {
        status: std::process::ExitStatus,
        stderr: Vec<u8>,
    },
    Report(String),
}

fn try_list(
    argv: &[String],
    runner_temp: Option<OsString>,
    can_execute: &ExecProbe,
    free: &FreeProbe,
) -> Result<Listing, Failure> {
    let mut argv = list_argv(argv).ok_or(Failure::NotListable)?;
    let directory = isolate(&mut argv, runner_temp, can_execute, free).map_err(|error| {
        Failure::Report(format!(
            "the early nextest list needs its own extraction directory: {error}"
        ))
    })?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in STEP_FILE_VARIABLES.iter().copied() {
        command.env_remove(name);
    }
    super::mbx_reports::assign(&mut command, &argv);
    let output = command.output().map_err(|error| {
        Failure::Report(format!("cannot start the early nextest list: {error}"))
    })?;
    drop(directory);
    if !output.status.success() {
        return Err(Failure::Exited {
            status: output.status,
            stderr: output.stderr,
        });
    }
    runnable_matches(&output.stdout).ok_or_else(|| {
        Failure::Report("the early nextest list printed no usable listing".to_owned())
    })
}

fn isolate(
    argv: &mut [String],
    runner_temp: Option<OsString>,
    can_execute: &ExecProbe,
    free: &FreeProbe,
) -> Result<Option<ListDir>, String> {
    if !extracts(argv) {
        return Ok(None);
    }
    let directory = ListDir::create(runner_temp, std::process::id(), can_execute)?;
    admit(argv, directory.path(), free)?;
    let path = directory
        .path()
        .to_str()
        .ok_or("the directory is not valid UTF-8")?
        .to_owned();
    retarget_extract(argv, &path);
    Ok(Some(directory))
}

fn admit(argv: &[String], directory: &Path, free: &FreeProbe) -> Result<(), String> {
    let archive = match archive_file(argv) {
        Some(path) => fs::metadata(path)
            .map_err(|error| format!("cannot read the archive size of {path}: {error}"))?
            .len(),
        None => 0,
    };
    let needed = archive.saturating_mul(EXPANSION_HEADROOM);
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

fn archive_file(argv: &[String]) -> Option<&str> {
    argv.iter().enumerate().find_map(|(index, arg)| {
        if arg == "--archive-file" {
            return argv.get(index + 1).map(String::as_str);
        }
        arg.strip_prefix("--archive-file=")
    })
}

#[cfg(test)]
#[path = "early_list_tests.rs"]
mod tests;
