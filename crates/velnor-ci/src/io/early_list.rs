// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread::{self, JoinHandle};

use super::early_list_dir::{ExecProbe, ListDir, can_execute_from};
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
    match try_list(argv, std::env::var_os("RUNNER_TEMP"), &can_execute_from) {
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
) -> Result<Listing, Failure> {
    let mut argv = list_argv(argv).ok_or(Failure::NotListable)?;
    let directory = isolate(&mut argv, runner_temp, can_execute).map_err(|error| {
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
) -> Result<Option<ListDir>, String> {
    if !extracts(argv) {
        return Ok(None);
    }
    let directory = ListDir::create(runner_temp, std::process::id(), can_execute)?;
    let path = directory
        .path()
        .to_str()
        .ok_or("the directory is not valid UTF-8")?
        .to_owned();
    retarget_extract(argv, &path);
    Ok(Some(directory))
}

#[cfg(test)]
mod tests {
    use super::{Failure, try_list};
    use crate::tests_run::Listing;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("velnor-early-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&directory));
        fs::create_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
        directory
    }

    fn archive_run(program: &Path) -> Vec<String> {
        [
            &program.display().to_string(),
            "nextest",
            "run",
            "--archive-file",
            "a.tar.zst",
            "--extract-to",
            ".",
        ]
        .iter()
        .map(|word| (*word).to_owned())
        .collect()
    }

    fn report(result: Result<Listing, Failure>) -> String {
        match result {
            Err(Failure::Report(message)) => message,
            other => unreachable!("{other:?}"),
        }
    }

    #[test]
    fn a_list_program_that_cannot_be_executed_is_reported_as_unable_to_start() {
        let directory = scratch("spawn");
        let program = directory.join("cargo-nextest");
        fs::write(&program, "#!/bin/sh\n").unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&program, fs::Permissions::from_mode(0o644))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let message = report(try_list(
            &archive_run(&program),
            Some(OsString::from(&directory)),
            &|_| Ok(true),
        ));
        assert!(
            message.starts_with("cannot start the early nextest list: "),
            "{message}"
        );
        assert!(message.contains("Permission denied"), "{message}");
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_missing_runner_temp_is_reported_before_anything_is_started() {
        let message = report(try_list(
            &archive_run(Path::new("/nonexistent/cargo-nextest")),
            None,
            &|_| Ok(true),
        ));
        assert_eq!(
            message,
            "the early nextest list needs its own extraction directory: RUNNER_TEMP is not set"
        );
    }

    #[test]
    fn an_unwritable_runner_temp_is_reported_with_the_directory_it_could_not_create() {
        let directory = scratch("readonly");
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o555))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let message = report(try_list(
            &archive_run(Path::new("/nonexistent/cargo-nextest")),
            Some(OsString::from(&directory)),
            &|_| Ok(true),
        ));
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let expected = format!(
            "cannot create {}",
            directory
                .join(format!("velnor-nextest-list-{}", std::process::id()))
                .display()
        );
        assert!(message.contains(&expected), "{message}");
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_list_that_exits_non_zero_keeps_its_status_and_stderr() {
        let directory = scratch("exit");
        let program = directory.join("cargo-nextest");
        fs::write(
            &program,
            "#!/bin/sh\necho 'double-spawn failed' >&2\nexit 104\n",
        )
        .unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let result = try_list(
            &archive_run(&program),
            Some(OsString::from(&directory)),
            &|_| Ok(true),
        );
        let Err(Failure::Exited { status, stderr }) = result else {
            unreachable!("{result:?}")
        };
        assert_eq!(status.code(), Some(104));
        assert_eq!(String::from_utf8_lossy(&stderr), "double-spawn failed\n");
        fs::remove_dir_all(&directory).unwrap_or_else(|e| unreachable!("{e}"));
    }

    #[test]
    fn a_destination_that_does_not_allow_execution_is_reported_before_the_list_starts() {
        let directory = scratch("noexec");
        let message = report(try_list(
            &archive_run(Path::new("/nonexistent/cargo-nextest")),
            Some(OsString::from(&directory)),
            &|_| Ok(false),
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
}
