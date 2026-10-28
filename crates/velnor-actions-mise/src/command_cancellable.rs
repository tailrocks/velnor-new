// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::output::{BoundedCommandOutput, BoundedCommandStatus, BoundedCommandTermination};
#[cfg(unix)]
use super::process_group::{
    ObservedStatus, ProcessGroupAnchor, observe_exit, reap_observed, terminate_unanchored,
};
#[cfg(unix)]
use super::stream::Streams;
use super::{CancelHandle, IsolatedCommand, ProcessFileOutput, ProcessOutput};
use crate::CheckDeadline;
use crate::error::MiseError;
use std::process::Command;
use std::time::{Duration, Instant};

#[cfg(unix)]
#[path = "command_cancellable_helpers.rs"]
mod helpers;
#[cfg(unix)]
use helpers::{
    ChildRunContext, cleanup_anchor, finish_child_process, poll_child, set_nonblocking,
    spawn_child, spawn_owned_command,
};

impl IsolatedCommand {
    pub(crate) const PROCESS_CLEANUP_GRACE: Duration = Duration::from_millis(250);

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

    #[cfg(unix)]
    pub fn run_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable_until(cap, deadline, &CancelHandle::new())
    }

    #[cfg(unix)]
    pub fn run_quiescent_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        self.run_quiescent_cancellable_until(cap, deadline, &CancelHandle::new())
    }

    #[cfg(unix)]
    pub fn run_quiescent_cancellable_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        cancel: &CancelHandle,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        let (work_deadline, cleanup_deadline) = quiescent_deadlines(deadline)?;
        let mut streams = Streams::memory(cap);
        let (mut output, receipt) = self.run_streams_with_policy(
            &mut streams,
            work_deadline,
            cancel,
            super::SPAWN_DEADLINE_MESSAGE,
            true,
            Some(cleanup_deadline),
        )?;
        output.stdout = streams.into_memory();
        receipt
            .map(|proof| (output, proof))
            .ok_or_else(|| MiseError::SpawnFailed {
                program: self.program.to_string_lossy().into_owned(),
                message: "quiescent_drain_receipt_missing".to_owned(),
            })
    }

    #[cfg(unix)]
    pub fn run_quiescent_stdout_to_file_until(
        &self,
        output: &mut std::fs::File,
        stdout_cap: u64,
        stderr_cap: usize,
        deadline: CheckDeadline,
    ) -> Result<(ProcessFileOutput, super::ProcessGroupDrainReceipt), MiseError> {
        if !output.metadata().is_ok_and(|metadata| metadata.is_file()) {
            return Err(MiseError::SpawnFailed {
                program: self.program.to_string_lossy().into_owned(),
                message: "stdout_sink_requires_regular_file".to_owned(),
            });
        }
        let (work_deadline, cleanup_deadline) = quiescent_deadlines(deadline)?;
        let mut streams = Streams::file(output, stdout_cap, stderr_cap);
        let (result, receipt) = self.run_streams_with_policy(
            &mut streams,
            work_deadline,
            &CancelHandle::new(),
            super::SPAWN_DEADLINE_MESSAGE,
            true,
            Some(cleanup_deadline),
        )?;
        let proof = receipt.ok_or_else(|| MiseError::SpawnFailed {
            program: self.program.to_string_lossy().into_owned(),
            message: "quiescent_drain_receipt_missing".to_owned(),
        })?;
        Ok((
            ProcessFileOutput {
                stdout_bytes: streams.stdout_bytes(),
                stderr: result.stderr,
                code: result.code,
                signal: result.signal,
                success: result.success,
            },
            proof,
        ))
    }

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
        let mut streams = Streams::memory(cap);
        let mut output = self.run_streams(&mut streams, deadline, cancel, timeout_message)?;
        output.stdout = streams.into_memory();
        Ok(output)
    }

    #[cfg(unix)]
    pub(super) fn run_streams(
        &self,
        streams: &mut Streams<'_>,
        deadline: CheckDeadline,
        cancel: &CancelHandle,
        timeout_message: &str,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_streams_with_policy(streams, deadline, cancel, timeout_message, false, None)
            .map(|(output, _)| output)
    }

    #[cfg(unix)]
    fn run_streams_with_policy(
        &self,
        streams: &mut Streams<'_>,
        deadline: CheckDeadline,
        cancel: &CancelHandle,
        timeout_message: &str,
        quiescent: bool,
        reserved_cleanup_deadline: Option<Instant>,
    ) -> Result<(ProcessOutput, Option<super::ProcessGroupDrainReceipt>), MiseError> {
        let effective = deadline.effective();
        let timeout_message = if effective == deadline {
            timeout_message
        } else {
            super::SPAWN_DEADLINE_MESSAGE
        };
        let deadline = effective;
        let program = self.program.to_string_lossy().into_owned();
        let fail = |message: &str| MiseError::SpawnFailed {
            program: program.clone(),
            message: message.to_owned(),
        };
        if cancel.is_cancelled() {
            return Err(fail(super::SPAWN_CANCELLED_MESSAGE));
        }
        deadline.remaining().map_err(|_| fail(timeout_message))?;
        let mut anchor = if quiescent {
            Some(ProcessGroupAnchor::spawn().map_err(|problem| fail(&problem))?)
        } else {
            None
        };
        let mut child = spawn_child(self, &mut anchor, reserved_cleanup_deadline)
            .map_err(|problem| fail(&problem))?;
        let mut context = ChildRunContext {
            program: &program,
            streams,
            deadline,
            cancel,
            timeout_message,
            quiescent,
            reserved_cleanup_deadline,
        };
        finish_child_process(&mut context, &mut child, &mut anchor)
    }

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

    #[cfg(not(unix))]
    pub fn run_until(
        &self,
        _cap: usize,
        _deadline: CheckDeadline,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable(0, Duration::ZERO, &CancelHandle::new())
    }

    #[cfg(not(unix))]
    pub fn run_cancellable_until(
        &self,
        _cap: usize,
        _deadline: CheckDeadline,
        _cancel: &CancelHandle,
    ) -> Result<ProcessOutput, MiseError> {
        self.run_cancellable(0, Duration::ZERO, &CancelHandle::new())
    }

    #[cfg(not(unix))]
    pub fn run_quiescent_until(
        &self,
        _cap: usize,
        _deadline: CheckDeadline,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        Err(MiseError::SpawnFailed {
            program: self.program.to_string_lossy().into_owned(),
            message: "quiescent_process_groups_require_unix".to_owned(),
        })
    }

    #[cfg(not(unix))]
    pub fn run_quiescent_cancellable_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        _cancel: &CancelHandle,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        self.run_quiescent_until(cap, deadline)
    }

    #[cfg(not(unix))]
    pub fn run_quiescent_stdout_to_file_until(
        &self,
        output: &mut std::fs::File,
        stdout_cap: u64,
        stderr_cap: usize,
        deadline: CheckDeadline,
    ) -> Result<(ProcessFileOutput, super::ProcessGroupDrainReceipt), MiseError> {
        let _ = (output, stdout_cap, stderr_cap, deadline);
        Err(MiseError::SpawnFailed {
            program: self.program.to_string_lossy().into_owned(),
            message: "quiescent_process_groups_require_unix".to_owned(),
        })
    }
}

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
    use std::time::Instant;

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
    use super::process_group::ProcessGroupAnchor;

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
    use super::process_group::observe_exit;
    use super::stream::Streams;

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
            super::SPAWN_DEADLINE_MESSAGE,
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
fn status_from_observed(status: super::process_group::ObservedStatus) -> BoundedCommandStatus {
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
    if error == super::SPAWN_DEADLINE_MESSAGE
        || error.starts_with(super::SPAWN_TIMEOUT_MESSAGE_PREFIX)
    {
        BoundedCommandTermination::TimedOut
    } else if error == super::SPAWN_CANCELLED_MESSAGE {
        BoundedCommandTermination::Cancelled
    } else if error.starts_with("stdout_limit_exceeded:")
        || error.starts_with("stderr_limit_exceeded:")
    {
        BoundedCommandTermination::OutputLimitExceeded
    } else {
        BoundedCommandTermination::ExecutionFailed
    }
}

#[cfg(unix)]
fn quiescent_deadlines(deadline: CheckDeadline) -> Result<(CheckDeadline, Instant), MiseError> {
    let remaining = deadline.remaining()?;
    let work = remaining
        .checked_sub(IsolatedCommand::PROCESS_CLEANUP_GRACE)
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| crate::checks::invalid("check_timeout", "insufficient_cleanup_budget"))?;
    let work_deadline = CheckDeadline::after(work)?;
    let cleanup_deadline = Instant::now()
        .checked_add(remaining)
        .ok_or_else(|| crate::checks::invalid("check_timeout", "deadline_overflow"))?;
    Ok((work_deadline, cleanup_deadline))
}
