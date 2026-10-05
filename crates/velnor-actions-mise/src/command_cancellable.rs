//! Deadline-bound subprocess polling without blocking pipe-reader threads.
use super::{CancelHandle, IsolatedCommand, ProcessOutput};
use crate::CheckDeadline;
use crate::error::MiseError;
use std::process::{Child, Stdio};
use std::time::Duration;

impl IsolatedCommand {
    /// Spawn the child under explicit bounds plus external cancellation.
    ///
    /// The child starts in a dedicated process group. Both pipes use nonblocking
    /// reads in this thread, so a descendant that inherits a pipe cannot block
    /// cleanup; timeout or cancellation kills the full process group.
    /// # Errors
    /// Returns `SpawnFailed` on spawn, output, timeout, or cancellation failure.
    #[cfg(unix)]
    pub fn run_cancellable(
        &self,
        cap: usize,
        timeout: Duration,
        cancel: &CancelHandle,
    ) -> Result<ProcessOutput, MiseError> {
        let deadline = CheckDeadline::after(timeout)?;
        let timeout_message = format!(
            "{}{seconds}",
            super::SPAWN_TIMEOUT_MESSAGE_PREFIX,
            seconds = timeout.as_secs()
        );
        self.run_cancellable_until_message(cap, deadline, cancel, &timeout_message)
    }

    /// Run under an already-started absolute deadline.
    /// # Errors
    /// Returns `SpawnFailed` on spawn, output, deadline, or cancellation failure.
    #[cfg(unix)]
    pub fn run_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable_until(cap, deadline, &CancelHandle::new())
    }

    /// Run under an absolute deadline and external cancellation.
    /// # Errors
    /// Returns `SpawnFailed` on spawn, output, deadline, or cancellation failure.
    #[cfg(unix)]
    pub fn run_cancellable_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        cancel: &CancelHandle,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable_until_message(cap, deadline, cancel, super::SPAWN_DEADLINE_MESSAGE)
    }

    #[cfg(unix)]
    fn run_cancellable_until_message(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        cancel: &CancelHandle,
        timeout_message: &str,
    ) -> Result<ProcessOutput, MiseError> {
        let program = self.program.to_string_lossy().into_owned();
        let fail = |message: &str| MiseError::SpawnFailed {
            program: program.clone(),
            message: message.to_owned(),
        };
        if cancel.is_cancelled() {
            return Err(fail(super::SPAWN_CANCELLED_MESSAGE));
        }
        deadline.remaining().map_err(|_| fail(timeout_message))?;
        let mut command = self.command();
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| fail(&error.to_string()))?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            terminate_group(&mut child);
            return Err(fail("child_pipe_missing"));
        };
        let mut stdout = Some(stdout);
        let mut stderr = Some(stderr);
        if let Some(reader) = stdout.as_ref()
            && let Err(error) = set_nonblocking(reader)
        {
            terminate_group(&mut child);
            return Err(fail(&error));
        }
        if let Some(reader) = stderr.as_ref()
            && let Err(error) = set_nonblocking(reader)
        {
            terminate_group(&mut child);
            return Err(fail(&error));
        }
        let mut out = Vec::with_capacity(cap.min(64 * 1024));
        let mut err = Vec::with_capacity(cap.min(64 * 1024));
        let mut status = None;
        loop {
            if cancel.is_cancelled() {
                terminate_group(&mut child);
                return Err(fail(super::SPAWN_CANCELLED_MESSAGE));
            }
            let remaining = match deadline.remaining() {
                Ok(remaining) => remaining,
                Err(_) => {
                    terminate_group(&mut child);
                    return Err(fail(timeout_message));
                }
            };
            if remaining.is_zero() {
                terminate_group(&mut child);
                return Err(fail(timeout_message));
            }
            match poll_pipe(&mut stdout, &mut out, cap, "stdout") {
                Ok(_) => {}
                Err(error) => {
                    terminate_group(&mut child);
                    return Err(fail(&error));
                }
            }
            match poll_pipe(&mut stderr, &mut err, cap, "stderr") {
                Ok(_) => {}
                Err(error) => {
                    terminate_group(&mut child);
                    return Err(fail(&error));
                }
            }
            status = match child.try_wait() {
                Ok(observed) => observed.or(status),
                Err(error) => {
                    terminate_group(&mut child);
                    return Err(fail(&error.to_string()));
                }
            };
            if let (Some(status), None, None) = (status, stdout.as_ref(), stderr.as_ref()) {
                return Ok(ProcessOutput {
                    stdout: out,
                    stderr: err,
                    code: status.code(),
                    signal: super::output::signal_of(status),
                    success: status.success(),
                });
            }
            std::thread::sleep(Duration::from_millis(5).min(remaining));
        }
    }

    /// Non-Unix named-check targets have no supported process-group cleanup.
    /// # Errors
    /// Returns `SpawnFailed` without starting a process.
    #[cfg(not(unix))]
    pub fn run_cancellable(
        &self,
        _cap: usize,
        _timeout: Duration,
        _cancel: &CancelHandle,
    ) -> Result<ProcessOutput, MiseError> {
        Err(MiseError::SpawnFailed {
            program: self.program.to_string_lossy().into_owned(),
            message: "cancellable_process_groups_require_unix".to_owned(),
        })
    }

    /// Run under an already-started absolute deadline.
    /// # Errors
    /// Returns `SpawnFailed` without starting a process.
    #[cfg(not(unix))]
    pub fn run_until(
        &self,
        _cap: usize,
        _deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable(0, Duration::ZERO, &CancelHandle::new())
    }

    /// Run under an absolute deadline and external cancellation.
    /// # Errors
    /// Returns `SpawnFailed` without starting a process.
    #[cfg(not(unix))]
    pub fn run_cancellable_until(
        &self,
        _cap: usize,
        _deadline: CheckDeadline,
        _cancel: &CancelHandle,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable(0, Duration::ZERO, &CancelHandle::new())
    }
}

#[cfg(unix)]
fn set_nonblocking<Fd: std::os::fd::AsFd>(fd: Fd) -> Result<(), String> {
    let flags = rustix::fs::fcntl_getfl(&fd).map_err(|error| error.to_string())?;
    rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
fn poll_pipe<Fd: std::os::fd::AsFd>(
    pipe: &mut Option<Fd>,
    output: &mut Vec<u8>,
    cap: usize,
    stream: &str,
) -> Result<bool, String> {
    let Some(reader) = pipe.as_ref() else {
        return Ok(true);
    };
    let mut buffer = [0_u8; 8192];
    loop {
        match rustix::io::read(reader, &mut buffer[..]) {
            Ok(0) => {
                pipe.take();
                return Ok(true);
            }
            Ok(count) => {
                let Some(next_len) = output.len().checked_add(count) else {
                    return Err("output_limit_exceeded".to_owned());
                };
                if next_len > cap {
                    return Err(format!("{stream}_limit_exceeded:{cap}"));
                }
                output.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error == rustix::io::Errno::INTR => continue,
            Err(error) if error == rustix::io::Errno::AGAIN => return Ok(false),
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(unix)]
fn terminate_group(child: &mut Child) {
    if let Ok(raw_pid) = i32::try_from(child.id())
        && let Some(pid) = rustix::process::Pid::from_raw(raw_pid)
    {
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}
