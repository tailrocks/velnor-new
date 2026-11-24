// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs::File;
use std::io::{self, ErrorKind, Read};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;

const POLL_FIRST: Duration = Duration::from_millis(1);
const POLL_LAST: Duration = Duration::from_millis(10);
const DRAIN_GRACE: Duration = Duration::from_secs(2);
const DRAIN_LIMIT: Duration = Duration::from_secs(4);
const STDERR_KEPT: u64 = 1 << 20;
const CHUNK: usize = 1 << 14;

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

struct Pipe {
    file: Option<File>,
    kept: Vec<u8>,
    keep: usize,
}

impl Supervisor {
    pub(super) fn stop(&self, reason: impl Into<String>) {
        let mut slot = self.lock();
        slot.stopped.get_or_insert_with(|| reason.into());
        if let Some(child) = slot.child.as_mut() {
            kill(child);
        }
    }

    pub(super) fn reason(&self) -> Option<String> {
        self.lock().stopped.clone()
    }

    pub(super) fn run(&self, command: &mut Command, stdout_limit: u64) -> io::Result<Finished> {
        let (stdout, stderr) = self.spawn(command)?;
        let mut stdout = Pipe::new(stdout, stdout_limit.saturating_add(1));
        let mut stderr = Pipe::new(stderr, STDERR_KEPT);
        let status = self.drain(command, &mut stdout, &mut stderr, stdout_limit);
        let stopped = self.release();
        Ok(Finished {
            status: status?,
            stdout: stdout.kept,
            stderr: stderr.kept,
            stopped,
        })
    }

    pub(super) fn run_streaming(&self, command: &mut Command) -> io::Result<Finished> {
        {
            let mut slot = self.lock();
            if let Some(reason) = &slot.stopped {
                return Err(io::Error::other(reason.clone()));
            }
            let output = io::stderr().as_fd().try_clone_to_owned()?;
            slot.child = Some(
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::from(output))
                    .stderr(Stdio::inherit())
                    .process_group(0)
                    .spawn()?,
            );
        }
        let status = self.wait();
        let stopped = self.release();
        Ok(Finished {
            status: status?,
            stdout: Vec::new(),
            stderr: Vec::new(),
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

    fn drain(
        &self,
        command: &Command,
        stdout: &mut Pipe,
        stderr: &mut Pipe,
        limit: u64,
    ) -> io::Result<ExitStatus> {
        stdout.unblock()?;
        stderr.unblock()?;
        let mut poll = POLL_FIRST;
        let mut exited = None;
        let mut overflowed = false;
        loop {
            if exited.is_none() {
                exited = self.exited()?.map(|status| (status, Instant::now()));
            }
            let idle = read_ready([&mut *stdout, &mut *stderr], poll)?;
            if !overflowed && u64::try_from(stdout.kept.len()).is_ok_and(|read| read > limit) {
                overflowed = true;
                self.stop(format!("its listing is longer than {limit} bytes"));
                stdout.file = None;
            }
            if let Some((status, at)) = exited {
                if stdout.file.is_none() && stderr.file.is_none() {
                    return Ok(status);
                }
                if idle && at.elapsed() >= DRAIN_GRACE || at.elapsed() >= DRAIN_LIMIT {
                    self.stop(format!(
                        "{} exited, but a process it started left its process group and still held its output {} ms later",
                        command.get_program().to_string_lossy(),
                        DRAIN_GRACE.as_millis()
                    ));
                    return Ok(status);
                }
            }
            poll = (poll * 2).min(POLL_LAST);
        }
    }

    fn wait(&self) -> io::Result<ExitStatus> {
        let mut poll = POLL_FIRST;
        loop {
            if let Some(status) = self.exited()? {
                return Ok(status);
            }
            thread::sleep(poll);
            poll = (poll * 2).min(POLL_LAST);
        }
    }

    fn exited(&self) -> io::Result<Option<ExitStatus>> {
        let mut slot = self.lock();
        let status = slot
            .child
            .as_mut()
            .ok_or_else(|| io::Error::other("no child"))?
            .try_wait()?;
        if status.is_some()
            && let Some(mut child) = slot.child.take()
        {
            kill(&mut child);
        }
        Ok(status)
    }

    fn release(&self) -> Option<String> {
        let (child, stopped) = {
            let mut slot = self.lock();
            (slot.child.take(), slot.stopped.clone())
        };
        if let Some(mut child) = child {
            kill(&mut child);
            child.wait().ok();
        }
        stopped
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Slot> {
        self.slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Pipe {
    fn new(source: impl Into<OwnedFd>, keep: u64) -> Self {
        Self {
            file: Some(File::from(source.into())),
            kept: Vec::new(),
            keep: usize::try_from(keep).unwrap_or(usize::MAX),
        }
    }

    fn unblock(&self) -> io::Result<()> {
        if let Some(file) = &self.file {
            fcntl(file, FcntlArg::F_SETFL(OFlag::O_NONBLOCK))?;
        }
        Ok(())
    }

    fn read(&mut self) -> io::Result<()> {
        let Some(file) = self.file.as_mut() else {
            return Ok(());
        };
        let mut chunk = [0; CHUNK];
        match file.read(&mut chunk) {
            Ok(0) => self.file = None,
            Ok(read) => {
                let room = self.keep.saturating_sub(self.kept.len()).min(read);
                self.kept.extend_from_slice(&chunk[..room]);
            }
            Err(error)
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::Interrupted) => {}
            Err(error) => return Err(error),
        }
        Ok(())
    }
}

fn read_ready(pipes: [&mut Pipe; 2], wait: Duration) -> io::Result<bool> {
    let idle = {
        let mut fds: Vec<PollFd<'_>> = pipes
            .iter()
            .filter_map(|pipe| pipe.file.as_ref())
            .map(|file| PollFd::new(file.as_fd(), PollFlags::POLLIN))
            .collect();
        match poll(
            &mut fds,
            PollTimeout::try_from(wait).unwrap_or(PollTimeout::MAX),
        ) {
            Ok(ready) => ready == 0,
            Err(Errno::EINTR) => false,
            Err(Errno::EAGAIN) => {
                thread::sleep(wait);
                false
            }
            Err(errno) => return Err(errno.into()),
        }
    };
    for pipe in pipes {
        pipe.read()?;
    }
    Ok(idle)
}

fn kill(child: &mut Child) {
    if let Ok(group) = i32::try_from(child.id()) {
        killpg(Pid::from_raw(group), Signal::SIGKILL).ok();
    }
    child.kill().ok();
}
