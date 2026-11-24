// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

const POLL_FIRST: Duration = Duration::from_millis(1);
const POLL_LAST: Duration = Duration::from_millis(10);
const STDERR_KEPT: u64 = 1 << 20;

#[derive(Default)]
struct Slot {
    child: Option<Child>,
    stopped: Option<String>,
}

#[derive(Default)]
pub(super) struct Supervisor {
    slot: Mutex<Slot>,
}

pub(super) struct Finished {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
    pub(super) stopped: Option<String>,
}

impl Supervisor {
    pub(super) fn stop(&self, reason: impl Into<String>) {
        let mut slot = self.lock();
        slot.stopped.get_or_insert_with(|| reason.into());
        if let Some(child) = slot.child.as_ref() {
            kill_group(child);
        }
    }

    pub(super) fn reason(&self) -> Option<String> {
        self.lock().stopped.clone()
    }

    pub(super) fn run(&self, command: &mut Command, stdout_limit: u64) -> io::Result<Finished> {
        let (stdout, stderr) = self.spawn(command)?;
        let (status, stdout, stderr) = thread::scope(|scope| {
            let stderr = scope.spawn(move || keep(stderr, STDERR_KEPT));
            let stdout = scope.spawn(move || self.bounded(stdout, stdout_limit));
            let status = self.wait();
            if let Some(child) = self.lock().child.as_ref() {
                kill_group(child);
            }
            (status, stdout.join(), stderr.join())
        });
        let stopped = {
            let mut slot = self.lock();
            if let Some(child) = slot.child.take() {
                kill_group(&child);
            }
            slot.stopped.clone()
        };
        let stdout = stdout.map_err(|_| io::Error::other("its output reader panicked"))??;
        let stderr = stderr.map_err(|_| io::Error::other("its error reader panicked"))??;
        Ok(Finished {
            status: status?,
            stdout,
            stderr,
            stopped,
        })
    }

    fn spawn(
        &self,
        command: &mut Command,
    ) -> io::Result<(std::process::ChildStdout, std::process::ChildStderr)> {
        let mut slot = self.lock();
        if let Some(reason) = &slot.stopped {
            return Err(io::Error::other(reason.clone()));
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("no stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("no stderr"))?;
        slot.child = Some(child);
        Ok((stdout, stderr))
    }

    fn wait(&self) -> io::Result<ExitStatus> {
        let mut poll = POLL_FIRST;
        loop {
            let polled = self
                .lock()
                .child
                .as_mut()
                .ok_or_else(|| io::Error::other("no child"))?
                .try_wait()?;
            if let Some(status) = polled {
                return Ok(status);
            }
            thread::sleep(poll);
            poll = (poll * 2).min(POLL_LAST);
        }
    }

    fn bounded(&self, pipe: impl Read, limit: u64) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        pipe.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).is_ok_and(|read| read > limit) {
            self.stop(format!("its listing is longer than {limit} bytes"));
        }
        Ok(bytes)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn kill_group(child: &Child) {
    if let Ok(group) = i32::try_from(child.id()) {
        killpg(Pid::from_raw(group), Signal::SIGKILL).ok();
    }
}

fn keep(mut pipe: impl Read, limit: u64) -> io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    (&mut pipe).take(limit).read_to_end(&mut kept)?;
    io::copy(&mut pipe, &mut io::sink())?;
    Ok(kept)
}
