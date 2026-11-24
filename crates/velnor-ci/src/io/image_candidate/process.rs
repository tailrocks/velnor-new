// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::{Handle, Signals};

use super::super::early_list_child::Supervisor;
use crate::error::CiError;
use crate::image_candidate::build::forwards;

const CAPTURE_LIMIT: u64 = 1 << 20;
const STDERR_SHOWN: usize = 2000;
const SIGNALLED: &str = "velnor-ci received a signal";
const SCRIPT_VARIABLES: &[&str] = &["PATH", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TZ"];

pub(super) struct Runner {
    supervisor: Arc<Supervisor>,
    close: Handle,
}

impl Runner {
    pub(super) fn start() -> Result<Self, CiError> {
        let supervisor = Arc::new(Supervisor::default());
        let mut signals = Signals::new([SIGINT, SIGTERM])
            .map_err(|error| CiError::new(format!("cannot watch for signals: {error}")))?;
        let close = signals.handle();
        let watcher = Arc::clone(&supervisor);
        thread::spawn(move || {
            for _ in signals.forever() {
                watcher.stop(SIGNALLED);
            }
        });
        Ok(Self { supervisor, close })
    }

    pub(super) fn checkpoint(&self) -> Result<(), CiError> {
        self.supervisor
            .reason()
            .map_or(Ok(()), |reason| Err(CiError::new(reason)))
    }

    pub(super) fn run(&self, command: &mut Command, timeout: Duration) -> Result<(), CiError> {
        self.execute(command, timeout, None).map(drop)
    }

    pub(super) fn text(&self, command: &mut Command, timeout: Duration) -> Result<String, CiError> {
        self.text_up_to(command, timeout, CAPTURE_LIMIT)
    }

    pub(super) fn text_up_to(
        &self,
        command: &mut Command,
        timeout: Duration,
        limit: u64,
    ) -> Result<String, CiError> {
        let bytes = self.execute(command, timeout, Some(limit))?;
        String::from_utf8(bytes).map_err(|_| CiError::new("command output is not UTF-8."))
    }

    fn execute(
        &self,
        command: &mut Command,
        timeout: Duration,
        capture: Option<u64>,
    ) -> Result<Vec<u8>, CiError> {
        let label = command.get_program().to_string_lossy().into_owned();
        let (done, waiting) = mpsc::channel::<()>();
        let supervisor = &self.supervisor;
        let name = &label;
        let finished = thread::scope(|scope| {
            scope.spawn(move || {
                if waiting.recv_timeout(timeout) == Err(RecvTimeoutError::Timeout) {
                    supervisor.stop(format!(
                        "{name} did not finish within {} seconds.",
                        timeout.as_secs()
                    ));
                }
            });
            let finished = match capture {
                Some(limit) => self.supervisor.run(command, limit),
                None => self.supervisor.run_streaming(command),
            };
            drop(done);
            finished
        });
        let finished =
            finished.map_err(|error| CiError::new(format!("cannot run {label}: {error}")))?;
        if let Some(reason) = finished.stopped {
            return Err(CiError::new(reason));
        }
        if !finished.status.success() {
            let shown = finished.stderr.len().saturating_sub(STDERR_SHOWN);
            let tail = String::from_utf8_lossy(&finished.stderr[shown..]);
            return Err(CiError::new(format!(
                "{label} failed: {}.\n{}",
                finished.status,
                tail.trim()
            )));
        }
        Ok(finished.stdout)
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.close.close();
    }
}

pub(super) fn script_environment<'a>(command: &'a mut Command, home: &Path) -> &'a mut Command {
    command.env_clear().env("HOME", home);
    for name in SCRIPT_VARIABLES {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

pub(super) fn clean_environment(command: &mut Command) -> &mut Command {
    command
        .env_clear()
        .envs(std::env::vars_os().filter(|(name, _)| name.to_str().is_some_and(forwards)))
}
