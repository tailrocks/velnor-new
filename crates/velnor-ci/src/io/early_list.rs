// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use crate::tests_run::{Listing, list_argv, runnable_matches};

const STEP_FILE_VARIABLES: [&str; 5] = [
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
    let argv = list_argv(argv)?;
    let mut command = Command::new(&argv[0]);
    command
        .args(&argv[1..])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for name in STEP_FILE_VARIABLES {
        command.env_remove(name);
    }
    let output = command.output().ok()?;
    if !output.status.success() {
        drop(std::io::stderr().write_all(&output.stderr));
        return None;
    }
    runnable_matches(&output.stdout)
}
