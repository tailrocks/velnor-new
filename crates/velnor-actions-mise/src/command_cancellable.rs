//! Deadline-bound subprocess polling without blocking pipe-reader threads.
#[cfg(unix)]
use super::process_group::{
    ObservedStatus, ProcessGroupAnchor, observe_exit, reap_observed, terminate_unanchored,
};
#[cfg(unix)]
use super::stream::Streams;
use super::{CancelHandle, IsolatedCommand, ProcessOutput};
use crate::CheckDeadline;
use crate::error::MiseError;
use std::time::{Duration, Instant};

#[cfg(unix)]
#[path = "command_cancellable_helpers.rs"]
mod helpers;
#[cfg(unix)]
use helpers::{ChildRunContext, finish_child_process, spawn_child};

impl IsolatedCommand {
    pub(crate) const PROCESS_CLEANUP_GRACE: Duration = Duration::from_millis(250);

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

    /// Run and return only after the original process group is drained.
    ///
    /// The cleanup grace is reserved inside `deadline`; a receipt is produced
    /// only after the leader was held through group termination, reaped, and
    /// the original group was observed absent.
    /// # Errors
    /// Returns `SpawnFailed` when execution or proof of group drain fails.
    #[cfg(unix)]
    pub fn run_quiescent_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        self.run_quiescent_cancellable_until(cap, deadline, &CancelHandle::new())
    }

    /// Quiescent run with external cancellation under the same deadline.
    /// # Errors
    /// Returns `SpawnFailed` when execution, cancellation, or group drain fails.
    #[cfg(unix)]
    pub fn run_quiescent_cancellable_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        cancel: &CancelHandle,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        let remaining = deadline.remaining()?;
        let work = remaining
            .checked_sub(Self::PROCESS_CLEANUP_GRACE)
            .filter(|duration| !duration.is_zero())
            .ok_or_else(|| {
                crate::checks::invalid("check_timeout", "insufficient_cleanup_budget")
            })?;
        let work_deadline = CheckDeadline::after(work)?;
        let cleanup_deadline = Instant::now()
            .checked_add(remaining)
            .ok_or_else(|| crate::checks::invalid("check_timeout", "deadline_overflow"))?;
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

    /// Unsupported on targets without Unix process-group proof.
    /// # Errors
    /// Returns `SpawnFailed` without starting a process.
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

    /// Unsupported on targets without Unix process-group proof.
    /// # Errors
    /// Returns `SpawnFailed` without starting a process.
    #[cfg(not(unix))]
    pub fn run_quiescent_cancellable_until(
        &self,
        cap: usize,
        deadline: CheckDeadline,
        _cancel: &CancelHandle,
    ) -> Result<(ProcessOutput, super::ProcessGroupDrainReceipt), MiseError> {
        self.run_quiescent_until(cap, deadline)
    }
}
