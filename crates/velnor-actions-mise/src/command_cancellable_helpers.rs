// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CancelHandle, IsolatedCommand, ObservedStatus, ProcessGroupAnchor, ProcessOutput, Streams,
    observe_exit, reap_observed, terminate_unanchored,
};
use crate::CheckDeadline;
use std::os::unix::process::CommandExt;
use std::process::Child;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(super) struct ChildRunContext<'a, 'streams> {
    pub(super) program: &'a str,
    pub(super) streams: &'a mut Streams<'streams>,
    pub(super) deadline: CheckDeadline,
    pub(super) cancel: &'a CancelHandle,
    pub(super) timeout_message: &'a str,
    pub(super) quiescent: bool,
    pub(super) reserved_cleanup_deadline: Option<Instant>,
}

pub(super) struct ChildPollResult {
    pub(super) status: Option<ObservedStatus>,
    pub(super) error: Option<String>,
}

pub(super) fn poll_child(
    child: &mut Child,
    stdout: std::process::ChildStdout,
    stderr: std::process::ChildStderr,
    streams: &mut Streams<'_>,
    deadline: CheckDeadline,
    cancel: &CancelHandle,
    timeout_message: &str,
) -> ChildPollResult {
    let mut stdout = Some(stdout);
    let mut stderr = Some(stderr);
    let mut status: Option<ObservedStatus> = None;
    loop {
        if cancel.is_cancelled() {
            return ChildPollResult {
                status,
                error: Some(super::super::SPAWN_CANCELLED_MESSAGE.to_owned()),
            };
        }
        let Ok(remaining) = deadline.remaining() else {
            return ChildPollResult {
                status,
                error: Some(timeout_message.to_owned()),
            };
        };
        if remaining.is_zero() {
            return ChildPollResult {
                status,
                error: Some(timeout_message.to_owned()),
            };
        }
        let stdout_poll = streams.poll_stdout(&mut stdout);
        let stderr_poll = streams.poll_stderr(&mut stderr);
        let progressed = stdout_poll.as_ref().is_ok_and(|value| *value)
            || stderr_poll.as_ref().is_ok_and(|value| *value);
        if cancel.is_cancelled() {
            return ChildPollResult {
                status,
                error: Some(super::super::SPAWN_CANCELLED_MESSAGE.to_owned()),
            };
        }
        if let Err(error) = stdout_poll {
            return ChildPollResult {
                status,
                error: Some(error),
            };
        }
        if let Err(error) = stderr_poll {
            return ChildPollResult {
                status,
                error: Some(error),
            };
        }
        if deadline.remaining().is_err() {
            return ChildPollResult {
                status,
                error: Some(timeout_message.to_owned()),
            };
        }
        match observe_exit(child) {
            Ok(observed) => status = observed.or(status),
            Err(error) => {
                return ChildPollResult {
                    status,
                    error: Some(error),
                };
            }
        }
        if status.is_some() && stdout.is_none() && stderr.is_none() {
            return ChildPollResult {
                status,
                error: None,
            };
        }
        if !progressed {
            std::thread::sleep(Duration::from_millis(5).min(remaining));
        }
    }
}

pub(super) fn spawn_child(
    command: &IsolatedCommand,
    anchor: &mut Option<ProcessGroupAnchor>,
    reserved_cleanup_deadline: Option<Instant>,
) -> Result<Child, String> {
    let mut child_command = command.command();
    match spawn_owned_command(&mut child_command, anchor.as_ref()) {
        Ok(child) => Ok(child),
        Err(error) => Err(cleanup_context(
            &error,
            cleanup_anchor(anchor, None, cleanup_deadline(reserved_cleanup_deadline)),
        )),
    }
}

pub(super) fn spawn_owned_command(
    command: &mut Command,
    anchor: Option<&ProcessGroupAnchor>,
) -> Result<Child, String> {
    command.process_group(anchor.map_or(0, ProcessGroupAnchor::pgid));
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())
}

pub(super) fn finish_child_process(
    context: &mut ChildRunContext<'_, '_>,
    child: &mut Child,
    anchor: &mut Option<ProcessGroupAnchor>,
) -> Result<
    (
        ProcessOutput,
        Option<super::super::ProcessGroupDrainReceipt>,
    ),
    crate::MiseError,
> {
    let fail = |message: &str| crate::MiseError::SpawnFailed {
        program: context.program.to_owned(),
        message: message.to_owned(),
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        let problem = cleanup_context(
            "child_pipe_missing",
            cleanup_anchor(
                anchor,
                Some(child),
                cleanup_deadline(context.reserved_cleanup_deadline),
            ),
        );
        return Err(fail(&problem));
    };
    let nonblocking_result = set_nonblocking(&stdout).and_then(|()| set_nonblocking(&stderr));
    if let Err(problem) = nonblocking_result {
        let message = cleanup_context(
            &problem,
            cleanup_anchor(
                anchor,
                Some(child),
                cleanup_deadline(context.reserved_cleanup_deadline),
            ),
        );
        return Err(fail(&message));
    }
    let polled = poll_child(
        child,
        stdout,
        stderr,
        context.streams,
        context.deadline,
        context.cancel,
        context.timeout_message,
    );
    finish_polled_child(context, child, anchor, polled)
}

fn finish_polled_child(
    context: &mut ChildRunContext<'_, '_>,
    child: &mut Child,
    anchor: &mut Option<ProcessGroupAnchor>,
    polled: ChildPollResult,
) -> Result<
    (
        ProcessOutput,
        Option<super::super::ProcessGroupDrainReceipt>,
    ),
    crate::MiseError,
> {
    let fail = |message: &str| crate::MiseError::SpawnFailed {
        program: context.program.to_owned(),
        message: message.to_owned(),
    };
    match (polled.status, polled.error) {
        (Some(status), None) => finish_success(
            ProcessOutput {
                stdout: Vec::new(),
                stderr: context.streams.take_stderr(),
                code: status.code,
                signal: status.signal,
                success: status.success,
            },
            child,
            anchor,
            context.quiescent,
            context.reserved_cleanup_deadline,
            &fail,
        ),
        (_, Some(problem)) => {
            let error = cleanup_context(
                &problem,
                cleanup_anchor(
                    anchor,
                    Some(child),
                    cleanup_deadline(context.reserved_cleanup_deadline),
                ),
            );
            Err(fail(&error))
        }
        (None, None) => {
            let error = cleanup_context(
                "child_status_unavailable",
                cleanup_anchor(
                    anchor,
                    Some(child),
                    cleanup_deadline(context.reserved_cleanup_deadline),
                ),
            );
            Err(fail(&error))
        }
    }
}

fn finish_success(
    output: ProcessOutput,
    child: &mut Child,
    anchor: &mut Option<ProcessGroupAnchor>,
    quiescent: bool,
    reserved_cleanup_deadline: Option<Instant>,
    fail: &impl Fn(&str) -> crate::MiseError,
) -> Result<
    (
        ProcessOutput,
        Option<super::super::ProcessGroupDrainReceipt>,
    ),
    crate::MiseError,
> {
    if quiescent {
        let proof = anchor
            .take()
            .ok_or_else(|| fail("strict_anchor_missing"))?
            .drain(Some(child), cleanup_deadline(reserved_cleanup_deadline))
            .map_err(|problem| fail(&problem))?;
        Ok((output, Some(proof)))
    } else {
        reap_observed(child).map_err(|problem| fail(&problem))?;
        Ok((output, None))
    }
}

#[cfg(unix)]
pub(super) fn set_nonblocking<Fd: std::os::fd::AsFd>(fd: Fd) -> Result<(), String> {
    let flags = rustix::fs::fcntl_getfl(&fd).map_err(|error| error.to_string())?;
    rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
pub(super) fn cleanup_context(primary: &str, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => primary.to_owned(),
        Err(problem) => {
            let mut end = problem
                .len()
                .min(super::super::output::MAX_CLEANUP_FAILURE_BYTES);
            while !problem.is_char_boundary(end) {
                end -= 1;
            }
            format!("{primary};cleanup_failed:{}", &problem[..end])
        }
    }
}

#[cfg(unix)]
pub(super) fn cleanup_anchor(
    anchor: &mut Option<ProcessGroupAnchor>,
    child: Option<&mut Child>,
    deadline: Instant,
) -> Result<(), String> {
    match anchor.take() {
        Some(anchor) => anchor.drain(child, deadline).map(|_| ()),
        None => child
            .ok_or_else(|| "process_group_child_missing".to_owned())
            .and_then(|child| terminate_unanchored(child, deadline)),
    }
}

#[cfg(unix)]
pub(super) fn cleanup_deadline(reserved: Option<Instant>) -> Instant {
    reserved.unwrap_or_else(|| Instant::now() + IsolatedCommand::PROCESS_CLEANUP_GRACE)
}
