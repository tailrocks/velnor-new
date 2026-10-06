//! Deadline-bound subprocess polling without blocking pipe-reader threads.
use super::{CancelHandle, IsolatedCommand, ProcessOutput};
use crate::CheckDeadline;
use crate::error::MiseError;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
const PROCESS_CLEANUP_GRACE: Duration = Duration::from_millis(250);

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
        command.process_group(0);
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| fail(&error.to_string()))?;
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            let message = cleanup_context("child_pipe_missing", terminate_group(&mut child));
            return Err(fail(&message));
        };
        if let Err(error) = set_nonblocking(&stdout) {
            let message = cleanup_context(&error, terminate_group(&mut child));
            return Err(fail(&message));
        }
        if let Err(error) = set_nonblocking(&stderr) {
            let message = cleanup_context(&error, terminate_group(&mut child));
            return Err(fail(&message));
        }
        match poll_child(
            &mut child,
            stdout,
            stderr,
            cap,
            deadline,
            cancel,
            timeout_message,
        ) {
            Ok(output) => Ok(output),
            Err(problem) => {
                let message = cleanup_context(&problem, terminate_group(&mut child));
                Err(fail(&message))
            }
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
fn poll_child(
    child: &mut Child,
    stdout: std::process::ChildStdout,
    stderr: std::process::ChildStderr,
    cap: usize,
    deadline: CheckDeadline,
    cancel: &CancelHandle,
    timeout_message: &str,
) -> Result<ProcessOutput, String> {
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    let mut out = Vec::with_capacity(cap.min(64 * 1024));
    let mut err = Vec::with_capacity(cap.min(64 * 1024));
    let mut status = None;
    loop {
        if cancel.is_cancelled() {
            return Err(super::SPAWN_CANCELLED_MESSAGE.to_owned());
        }
        let Ok(remaining) = deadline.remaining() else {
            return Err(timeout_message.to_owned());
        };
        if remaining.is_zero() {
            return Err(timeout_message.to_owned());
        }
        poll_pipe(&mut stdout, &mut out, cap, "stdout")?;
        poll_pipe(&mut stderr, &mut err, cap, "stderr")?;
        status = child
            .try_wait()
            .map_err(|error| error.to_string())?
            .or(status);
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
            Err(error) if error == rustix::io::Errno::INTR => {}
            Err(error) if error == rustix::io::Errno::AGAIN => return Ok(false),
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(unix)]
fn cleanup_context(primary: &str, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => primary.to_owned(),
        Err(problem) => {
            let mut end = problem.len().min(super::output::MAX_CLEANUP_FAILURE_BYTES);
            while !problem.is_char_boundary(end) {
                end -= 1;
            }
            format!("{primary};cleanup_failed:{}", &problem[..end])
        }
    }
}

#[cfg(unix)]
fn terminate_group(child: &mut Child) -> Result<(), String> {
    let mut failures = Vec::new();
    match i32::try_from(child.id())
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    {
        Some(pid) => {
            match rustix::process::kill_process_group(pid, rustix::process::Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => failures.push(format!("kill_group:{error}")),
            }
        }
        None => failures.push("kill_group:invalid_child_id".to_owned()),
    }
    match child.try_wait() {
        Ok(Some(_)) => {}
        Ok(None) => {
            if let Err(error) = child.kill()
                && error.kind() != std::io::ErrorKind::InvalidInput
            {
                failures.push(format!("kill_child:{error}"));
            }
        }
        Err(error) => {
            failures.push(format!("check_child:{error}"));
            if let Err(kill_error) = child.kill()
                && kill_error.kind() != std::io::ErrorKind::InvalidInput
            {
                failures.push(format!("kill_child:{kill_error}"));
            }
        }
    }
    if let Err(error) = reap_child(child) {
        failures.push(format!("reap_child:{error}"));
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join(","))
    }
}

#[cfg(unix)]
fn reap_child(child: &mut Child) -> Result<(), String> {
    let deadline = Instant::now()
        .checked_add(PROCESS_CLEANUP_GRACE)
        .ok_or_else(|| "cleanup_deadline_overflow".to_owned())?;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => return Err("cleanup_deadline_exhausted".to_owned()),
            Err(error) => return Err(error.to_string()),
        }
    }
}
