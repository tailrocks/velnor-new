//! Bounded process capture for repository-maintenance probes.

use std::process::{Command, ExitStatus};
use std::time::Duration;

/// Captured process result after bounded stdout/stderr reads and a deadline.
#[derive(Debug)]
pub(crate) struct ProcessOutput {
    /// Final child status.
    pub(crate) status: ExitStatus,
    /// Captured stdout, never larger than the selected cap.
    pub(crate) stdout: Vec<u8>,
    /// Captured stderr, never larger than the selected cap.
    pub(crate) stderr: Vec<u8>,
}

/// Run a command with independently capped streams and a wall-clock timeout.
pub(crate) fn run_bounded(
    command: &mut Command,
    cap: usize,
    timeout: Duration,
) -> Result<ProcessOutput, String> {
    #[cfg(unix)]
    {
        run_unix(command, cap, timeout)
    }
    #[cfg(not(unix))]
    {
        let _ = (command, cap, timeout);
        Err("bounded process capture requires a Unix host".to_owned())
    }
}

#[cfg(unix)]
mod capture;

#[cfg(unix)]
mod unix {
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, ChildStderr, ChildStdout, Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process_group};

    use super::ProcessOutput;
    use super::capture::{Capture, reserve_capture, set_nonblocking};

    const POLL_INTERVAL: Duration = Duration::from_millis(5);
    const MAX_CLEANUP_RESERVE: Duration = Duration::from_millis(100);

    pub(super) fn run_unix(
        command: &mut Command,
        cap: usize,
        timeout: Duration,
    ) -> Result<ProcessOutput, String> {
        let stdout_bytes = reserve_capture(cap)?;
        let stderr_bytes = reserve_capture(cap)?;
        let started = Instant::now();
        let deadline = started
            .checked_add(timeout)
            .ok_or_else(|| "process deadline overflow".to_owned())?;
        let work_deadline = started
            .checked_add(timeout.saturating_sub(cleanup_reserve(timeout)))
            .ok_or_else(|| "process work deadline overflow".to_owned())?;
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| format!("process could not start ({error})"))?;
        let group = child.id();
        let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
            return Err(abort_without_captures(
                &mut child,
                group,
                deadline,
                "process stdout or stderr was unavailable".to_owned(),
            ));
        };
        if let Err(error) = set_nonblocking(&stdout).and_then(|()| set_nonblocking(&stderr)) {
            drop(stdout);
            drop(stderr);
            return Err(abort_without_captures(
                &mut child,
                group,
                deadline,
                format!("process pipes could not become nonblocking ({error})"),
            ));
        }
        let mut stdout = Capture::new(stdout, stdout_bytes, cap);
        let mut stderr = Capture::new(stderr, stderr_bytes, cap);
        run_capture_loop(
            &mut child,
            group,
            deadline,
            work_deadline,
            &mut stdout,
            &mut stderr,
        )
    }

    fn run_capture_loop(
        child: &mut Child,
        group: u32,
        deadline: Instant,
        work_deadline: Instant,
        stdout: &mut Capture<ChildStdout>,
        stderr: &mut Capture<ChildStderr>,
    ) -> Result<ProcessOutput, String> {
        let mut status = None;
        loop {
            if let Err(error) = stdout.drain().and_then(|()| stderr.drain()) {
                return Err(abort(child, group, deadline, stdout, stderr, error));
            }
            if stdout.overflowed || stderr.overflowed {
                return Err(abort(
                    child,
                    group,
                    deadline,
                    stdout,
                    stderr,
                    format!("process output exceeds {} bytes", stdout.cap),
                ));
            }
            if status.is_none() {
                match child.try_wait() {
                    Ok(exit) => status = exit,
                    Err(error) => {
                        return Err(abort(
                            child,
                            group,
                            deadline,
                            stdout,
                            stderr,
                            format!("process status failed ({error})"),
                        ));
                    }
                }
            }
            if stdout.is_closed()
                && stderr.is_closed()
                && let Some(status) = status.take()
            {
                if Instant::now() >= work_deadline {
                    return Err(abort(
                        child,
                        group,
                        deadline,
                        stdout,
                        stderr,
                        "process timed out before capture completed".to_owned(),
                    ));
                }
                return Ok(ProcessOutput {
                    status,
                    stdout: std::mem::take(&mut stdout.bytes),
                    stderr: std::mem::take(&mut stderr.bytes),
                });
            }
            if Instant::now() >= work_deadline {
                return Err(abort(
                    child,
                    group,
                    deadline,
                    stdout,
                    stderr,
                    "process timed out before child exit and stream completion".to_owned(),
                ));
            }
            pause_until(work_deadline);
        }
    }

    fn abort<R1: Read, R2: Read>(
        child: &mut Child,
        group: u32,
        deadline: Instant,
        stdout: &mut Capture<R1>,
        stderr: &mut Capture<R2>,
        cause: String,
    ) -> String {
        let mut failures = Vec::new();
        if let Err(error) = terminate_group(child, group) {
            failures.push(error);
        }
        let cleanup = drain_until_closed(child, deadline, stdout, stderr);
        if let Err(error) = cleanup {
            failures.push(error);
        }
        combine_failure(cause, &failures)
    }

    fn drain_until_closed<R1: Read, R2: Read>(
        child: &mut Child,
        deadline: Instant,
        stdout: &mut Capture<R1>,
        stderr: &mut Capture<R2>,
    ) -> Result<(), String> {
        let mut reaped = false;
        let mut failures = Vec::new();
        while Instant::now() < deadline {
            record_drain(stdout, "stdout", &mut failures);
            record_drain(stderr, "stderr", &mut failures);
            if !reaped {
                match child.try_wait() {
                    Ok(Some(_)) => reaped = true,
                    Ok(None) => {}
                    Err(error) => failures.push(format!("process reaping failed ({error})")),
                }
            }
            if reaped && stdout.is_closed() && stderr.is_closed() {
                return failures_to_result(&failures);
            }
            pause_until(deadline);
        }
        if !reaped {
            match child.try_wait() {
                Ok(Some(_)) => {}
                Ok(None) => failures.push("child was not reaped before the deadline".to_owned()),
                Err(error) => failures.push(format!("process reaping failed ({error})")),
            }
        }
        if !stdout.is_closed() || !stderr.is_closed() {
            failures.push("process output pipes remained open at the deadline".to_owned());
        }
        failures_to_result(&failures)
    }

    fn record_drain<R: Read>(capture: &mut Capture<R>, stream: &str, failures: &mut Vec<String>) {
        if let Err(error) = capture.drain() {
            failures.push(format!("process {stream} cleanup failed ({error})"));
        }
    }

    fn terminate_group(child: &mut Child, group: u32) -> Result<(), String> {
        let raw =
            i32::try_from(group).map_err(|error| format!("process group id invalid ({error})"))?;
        let pid = Pid::from_raw(raw).ok_or_else(|| "process group id was zero".to_owned())?;
        match kill_process_group(pid, Signal::KILL) {
            Ok(()) => Ok(()),
            Err(error) if error == Errno::SRCH => Ok(()),
            Err(group_error) => match child.kill() {
                Ok(()) => Err(format!(
                    "process group kill failed ({group_error}); direct child kill succeeded but descendant termination is unconfirmed"
                )),
                Err(child_error) => match child.try_wait() {
                    Ok(Some(_)) => Err(format!(
                        "process group kill failed ({group_error}); direct child already exited ({child_error})"
                    )),
                    Ok(None) => Err(format!(
                        "process group kill failed ({group_error}) and child kill failed ({child_error})"
                    )),
                    Err(wait_error) => Err(format!(
                        "process group kill failed ({group_error}), child kill failed ({child_error}), and status failed ({wait_error})"
                    )),
                },
            },
        }
    }

    fn abort_without_captures(
        child: &mut Child,
        group: u32,
        deadline: Instant,
        cause: String,
    ) -> String {
        drop(child.stdout.take());
        drop(child.stderr.take());
        let mut failures = Vec::new();
        if let Err(error) = terminate_group(child, group) {
            failures.push(error);
        }
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(_)) => return combine_failure(cause, &failures),
                Ok(None) => pause_until(deadline),
                Err(error) => {
                    failures.push(format!("process reaping failed ({error})"));
                    break;
                }
            }
        }
        failures.push("child was not reaped before the deadline".to_owned());
        combine_failure(cause, &failures)
    }

    fn cleanup_reserve(timeout: Duration) -> Duration {
        (timeout / 4).min(MAX_CLEANUP_RESERVE)
    }

    fn pause_until(deadline: Instant) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            thread::sleep(remaining.min(POLL_INTERVAL));
        }
    }

    fn failures_to_result(failures: &[String]) -> Result<(), String> {
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn combine_failure(cause: String, failures: &[String]) -> String {
        match failures_to_result(failures) {
            Ok(()) => cause,
            Err(cleanup) => format!("{cause}; cleanup also failed: {cleanup}"),
        }
    }
}

#[cfg(unix)]
use unix::run_unix;

#[cfg(all(test, unix))]
mod tests;
