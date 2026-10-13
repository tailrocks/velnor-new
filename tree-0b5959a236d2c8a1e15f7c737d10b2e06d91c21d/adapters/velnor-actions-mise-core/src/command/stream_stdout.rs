//! Bounded streaming of command stdout to a caller-owned sink.

#[cfg(unix)]
use super::CancelHandle;
use super::IsolatedCommand;
#[cfg(unix)]
use super::cancellable::{cleanup_context, poll_pipe, set_nonblocking, terminate_group};
#[cfg(unix)]
use crate::CheckDeadline;
use crate::error::MiseError;
#[cfg(unix)]
use std::process::{Child, Stdio};
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::CommandExt;

impl IsolatedCommand {
    /// Stream stdout to a bounded sink without retaining the payload in memory.
    ///
    /// The child retains this command's environment policy and process-group
    /// cleanup. At most `max_stdout_bytes` reaches `on_chunk`; one additional
    /// read detects an oversized stream and terminates the process group.
    /// Stderr remains separately captured under the normal output cap. Deadline
    /// and cancellation are checked between reads and around each synchronous
    /// sink callback; a callback already executing cannot be interrupted.
    /// # Errors
    /// Returns `SpawnFailed` on output, sink, timeout, or cleanup failure and
    /// `NonZeroExit` when the child exits unsuccessfully.
    #[cfg(unix)]
    pub fn run_stdout_to(
        &self,
        max_stdout_bytes: u64,
        timeout: Duration,
        mut on_chunk: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<u64, MiseError> {
        let program = self.program.to_string_lossy().into_owned();
        let fail = |message: &str| MiseError::SpawnFailed {
            program: program.clone(),
            message: message.to_owned(),
        };
        let deadline = CheckDeadline::after(timeout)?;
        deadline.remaining().map_err(|_| {
            fail(&format!(
                "{}{}",
                super::SPAWN_TIMEOUT_MESSAGE_PREFIX,
                timeout.as_secs()
            ))
        })?;
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
        let cancel = CancelHandle::new();
        let mut poll = StreamPollPolicy {
            max_stdout_bytes,
            deadline,
            cancel: &cancel,
            on_stdout: &mut on_chunk,
            timeout_seconds: timeout.as_secs(),
        };
        let output = match poll_child_stream(&mut child, stdout, stderr, &mut poll) {
            Ok(output) => output,
            Err(problem) => {
                let message = cleanup_context(&problem, terminate_group(&mut child));
                return Err(fail(&message));
            }
        };
        if output.success {
            Ok(output.bytes_written)
        } else {
            Err(MiseError::NonZeroExit {
                program,
                code: output.code,
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            })
        }
    }

    /// Non-Unix targets have no supported process-group cleanup for streams.
    /// # Errors
    /// Returns `SpawnFailed` without starting a process.
    #[cfg(not(unix))]
    pub fn run_stdout_to(
        &self,
        _max_stdout_bytes: u64,
        _timeout: Duration,
        _on_chunk: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<u64, MiseError> {
        Err(MiseError::SpawnFailed {
            program: self.program.to_string_lossy().into_owned(),
            message: "streaming_process_groups_require_unix".to_owned(),
        })
    }
}

#[cfg(unix)]
struct StreamProcessOutput {
    bytes_written: u64,
    stderr: Vec<u8>,
    code: Option<i32>,
    success: bool,
}

#[cfg(unix)]
struct StreamPollPolicy<'a, F> {
    max_stdout_bytes: u64,
    deadline: CheckDeadline,
    cancel: &'a CancelHandle,
    on_stdout: &'a mut F,
    timeout_seconds: u64,
}

#[cfg(unix)]
fn poll_child_stream<F: FnMut(&[u8]) -> Result<(), String>>(
    child: &mut Child,
    stdout: std::process::ChildStdout,
    stderr: std::process::ChildStderr,
    policy: &mut StreamPollPolicy<'_, F>,
) -> Result<StreamProcessOutput, String> {
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    let mut err = Vec::with_capacity(64 * 1024);
    let mut bytes_written = 0_u64;
    loop {
        let remaining = stream_checkpoint(policy.deadline, policy.cancel, policy.timeout_seconds)?;
        poll_pipe_to(
            &mut stdout,
            &mut bytes_written,
            policy.max_stdout_bytes,
            policy.on_stdout,
            policy.deadline,
            policy.cancel,
            policy.timeout_seconds,
        )?;
        poll_pipe(
            &mut stderr,
            &mut err,
            super::OUTPUT_CAPTURE_LIMIT_BYTES,
            "stderr",
        )?;
        if stdout.is_none() && stderr.is_none() {
            stream_checkpoint(policy.deadline, policy.cancel, policy.timeout_seconds)?;
            if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
                stream_checkpoint(policy.deadline, policy.cancel, policy.timeout_seconds)?;
                return Ok(StreamProcessOutput {
                    bytes_written,
                    stderr: err,
                    code: status.code(),
                    success: status.success(),
                });
            }
        }
        std::thread::sleep(Duration::from_millis(5).min(remaining));
    }
}

#[cfg(unix)]
fn stream_checkpoint(
    deadline: CheckDeadline,
    cancel: &CancelHandle,
    timeout_seconds: u64,
) -> Result<Duration, String> {
    if cancel.is_cancelled() {
        return Err(super::SPAWN_CANCELLED_MESSAGE.to_owned());
    }
    deadline
        .remaining()
        .map_err(|_| format!("{}{}", super::SPAWN_TIMEOUT_MESSAGE_PREFIX, timeout_seconds))
}

#[cfg(unix)]
fn poll_pipe_to<Fd: std::os::fd::AsFd>(
    pipe: &mut Option<Fd>,
    bytes_written: &mut u64,
    cap: u64,
    on_chunk: &mut impl FnMut(&[u8]) -> Result<(), String>,
    deadline: CheckDeadline,
    cancel: &CancelHandle,
    timeout_seconds: u64,
) -> Result<bool, String> {
    let Some(reader) = pipe.as_ref() else {
        return Ok(true);
    };
    let mut buffer = [0_u8; 8192];
    loop {
        stream_checkpoint(deadline, cancel, timeout_seconds)?;
        match rustix::io::read(reader, &mut buffer[..]) {
            Ok(0) => {
                pipe.take();
                return Ok(true);
            }
            Ok(count) => {
                let count = u64::try_from(count).map_err(|_| "stdout_size_overflow".to_owned())?;
                let next = bytes_written
                    .checked_add(count)
                    .ok_or_else(|| "stdout_size_overflow".to_owned())?;
                if next > cap {
                    return Err(format!("stdout_limit_exceeded:{cap}"));
                }
                let count =
                    usize::try_from(count).map_err(|_| "stdout_size_overflow".to_owned())?;
                stream_checkpoint(deadline, cancel, timeout_seconds)?;
                on_chunk(&buffer[..count])
                    .map_err(|error| format!("stdout_sink_failed:{error}"))?;
                *bytes_written = next;
                stream_checkpoint(deadline, cancel, timeout_seconds)?;
            }
            Err(error) if error == rustix::io::Errno::INTR => {}
            Err(error) if error == rustix::io::Errno::AGAIN => return Ok(false),
            Err(error) => return Err(error.to_string()),
        }
    }
}
