// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use super::early_list_dir::ListDir;
use crate::tests_run::{Listing, extracts, list_argv, retarget_extract, runnable_matches};

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
    let mut argv = list_argv(argv)?;
    let directory = match isolate(&mut argv) {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!(
                "velnor-ci: the early nextest list needs its own extraction directory: {error}; the step reports no per-package counts"
            );
            return None;
        }
    };
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in STEP_FILE_VARIABLES.iter().copied() {
        command.env_remove(name);
    }
    super::mbx_reports::assign(&mut command, &argv);
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => {
            eprintln!(
                "velnor-ci: cannot start the early nextest list: {error}; the step reports no per-package counts"
            );
            return None;
        }
    };
    drop(directory);
    if !output.status.success() {
        drop(std::io::stderr().write_all(&output.stderr));
        return None;
    }
    let listing = runnable_matches(&output.stdout);
    if listing.is_none() {
        eprintln!(
            "velnor-ci: the early nextest list printed no usable listing; the step reports no per-package counts"
        );
    }
    listing
}

fn isolate(argv: &mut [String]) -> Result<Option<ListDir>, String> {
    if !extracts(argv) {
        return Ok(None);
    }
    let directory = ListDir::create(std::env::var_os("RUNNER_TEMP"), std::process::id())?;
    let path = directory
        .path()
        .to_str()
        .ok_or("the directory is not valid UTF-8")?
        .to_owned();
    retarget_extract(argv, &path);
    Ok(Some(directory))
}
