// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::output::{BoundedCommandOutput, BoundedCommandStatus, BoundedCommandTermination};
#[cfg(unix)]
use super::super::process_group::{ObservedStatus, ProcessGroupAnchor, observe_exit};
#[cfg(unix)]
use super::super::stream::Streams;
#[cfg(unix)]
use super::CancelHandle;
#[cfg(unix)]
use super::helpers::{cleanup_anchor, poll_child, set_nonblocking, spawn_owned_command};
#[cfg(unix)]
use crate::CheckDeadline;
use std::process::Command;
use std::time::Duration;
#[cfg(unix)]
use std::time::Instant;

pub fn run_command_bounded(
    command: &mut Command,
    output_cap_bytes: usize,
    timeout: Duration,
    cleanup_budget: Duration,
) -> BoundedCommandOutput {
    #[cfg(unix)]
    {
        run_command_bounded_unix(command, output_cap_bytes, timeout, cleanup_budget)
    }
    #[cfg(not(unix))]
    {
        let _ = (command, output_cap_bytes, timeout, cleanup_budget);
        BoundedCommandOutput {
            stdout: Vec::new(),
            stderr: Vec::new(),
            status: BoundedCommandStatus::NotStarted,
            termination: BoundedCommandTermination::SpawnFailed,
            execution_error: Some("owned_process_groups_require_unix".to_owned()),
            cleanup_error: None,
        }
    }
}

#[cfg(unix)]
fn run_command_bounded_unix(
    command: &mut Command,
    output_cap_bytes: usize,
    timeout: Duration,
    cleanup_budget: Duration,
) -> BoundedCommandOutput {
    let mut result = empty_bounded_output();
    if output_cap_bytes == 0 || timeout.is_zero() || cleanup_budget.is_zero() {
        result.execution_error = Some("bounded_command_limits_must_be_positive".to_owned());
        result.termination = BoundedCommandTermination::ExecutionFailed;
        return result;
    }
    let start = Instant::now();
    let deadline = match CheckDeadline::from_start(start, timeout) {
        Ok(deadline) => deadline,
        Err(error) => {
            result.execution_error = Some(error.to_string());
            result.termination = BoundedCommandTermination::ExecutionFailed;
            return result;
        }
    };
    let (mut anchor, mut child) = match spawn_bounded_command(command, cleanup_budget) {
        Ok(spawned) => spawned,
        Err(output) => return output,
    };
    result.status = BoundedCommandStatus::Unavailable;
    collect_bounded_output(&mut child, output_cap_bytes, deadline, &mut result);
    result.termination = if result.execution_error.is_some() {
        termination_for_error(result.execution_error.as_deref())
    } else {
        BoundedCommandTermination::Completed
    };
    result.cleanup_error = cleanup_anchor(
        &mut anchor,
        Some(&mut child),
        cleanup_deadline_for(cleanup_budget),
    )
    .err();
    if result.cleanup_error.is_some() && result.termination == BoundedCommandTermination::Completed
    {
        result.termination = BoundedCommandTermination::CleanupFailed;
    }
    result
}

#[cfg(unix)]
fn empty_bounded_output() -> BoundedCommandOutput {
    BoundedCommandOutput {
        stdout: Vec::new(),
        stderr: Vec::new(),
        status: BoundedCommandStatus::NotStarted,
        termination: BoundedCommandTermination::SpawnFailed,
        execution_error: None,
        cleanup_error: None,
    }
}

#[cfg(unix)]
fn spawn_bounded_command(
    command: &mut Command,
    cleanup_budget: Duration,
) -> Result<(Option<ProcessGroupAnchor>, std::process::Child), BoundedCommandOutput> {
    let mut anchor = match ProcessGroupAnchor::spawn() {
        Ok(anchor) => Some(anchor),
        Err(error) => return Err(spawn_failure(error, None)),
    };
    match spawn_owned_command(command, anchor.as_ref()) {
        Ok(child) => Ok((anchor, child)),
        Err(error) => {
            let cleanup =
                cleanup_anchor(&mut anchor, None, cleanup_deadline_for(cleanup_budget)).err();
            Err(spawn_failure(error, cleanup))
        }
    }
}

#[cfg(unix)]
fn spawn_failure(error: String, cleanup_error: Option<String>) -> BoundedCommandOutput {
    let mut output = empty_bounded_output();
    output.execution_error = Some(error);
    output.cleanup_error = cleanup_error;
    output
}

#[cfg(unix)]
fn collect_bounded_output(
    child: &mut std::process::Child,
    output_cap_bytes: usize,
    deadline: CheckDeadline,
    result: &mut BoundedCommandOutput,
) {
    let Some(stdout) = child.stdout.take() else {
        result.execution_error = Some("child_stdout_pipe_missing".to_owned());
        return;
    };
    let Some(stderr) = child.stderr.take() else {
        result.execution_error = Some("child_stderr_pipe_missing".to_owned());
        return;
    };
    let mut streams = Streams::memory(output_cap_bytes);
    if let Err(error) = set_nonblocking(&stdout).and_then(|()| set_nonblocking(&stderr)) {
        result.execution_error = Some(error);
    } else {
        let polled = poll_child(
            child,
            stdout,
            stderr,
            &mut streams,
            deadline,
            &CancelHandle::new(),
            super::super::SPAWN_DEADLINE_MESSAGE,
        );
        result.status = polled
            .status
            .map_or(BoundedCommandStatus::Unavailable, status_from_observed);
        result.execution_error = polled.error;
    }
    if result.execution_error.is_some()
        && result.status == BoundedCommandStatus::Unavailable
        && let Ok(Some(status)) = observe_exit(child)
    {
        result.status = status_from_observed(status);
    }
    (result.stdout, result.stderr) = streams.into_prefixes();
}

#[cfg(unix)]
fn cleanup_deadline_for(budget: Duration) -> Instant {
    Instant::now()
        .checked_add(budget)
        .unwrap_or_else(Instant::now)
}

#[cfg(unix)]
fn status_from_observed(status: ObservedStatus) -> BoundedCommandStatus {
    match (status.code, status.signal) {
        (Some(code), _) => BoundedCommandStatus::Exited(code),
        (None, Some(signal)) => BoundedCommandStatus::Signaled(signal),
        (None, None) => BoundedCommandStatus::Unavailable,
    }
}

fn termination_for_error(error: Option<&str>) -> BoundedCommandTermination {
    let Some(error) = error else {
        return BoundedCommandTermination::ExecutionFailed;
    };
    if error == super::super::SPAWN_DEADLINE_MESSAGE
        || error.starts_with(super::super::SPAWN_TIMEOUT_MESSAGE_PREFIX)
    {
        BoundedCommandTermination::TimedOut
    } else if error == super::super::SPAWN_CANCELLED_MESSAGE {
        BoundedCommandTermination::Cancelled
    } else if error.starts_with("stdout_limit_exceeded:")
        || error.starts_with("stderr_limit_exceeded:")
    {
        BoundedCommandTermination::OutputLimitExceeded
    } else {
        BoundedCommandTermination::ExecutionFailed
    }
}
