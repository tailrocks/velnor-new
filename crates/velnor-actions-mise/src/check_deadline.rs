//! One monotonic deadline shared by every phase of a qualified named check.
use crate::MiseError;
use crate::checks::invalid;
use std::cell::Cell;
use std::time::{Duration, Instant};

thread_local! {
    static OPERATION_DEADLINE: Cell<Option<CheckDeadline>> = const { Cell::new(None) };
}

struct RestoreDeadline(Option<CheckDeadline>);

impl Drop for RestoreDeadline {
    fn drop(&mut self) {
        OPERATION_DEADLINE.set(self.0);
    }
}

/// Absolute timeout boundary for preparation, probes, acquisition and task execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckDeadline(Instant);

impl CheckDeadline {
    /// Bound synchronous calls on this thread to one absolute operation budget.
    ///
    /// Nested scopes can only shorten the parent. The previous scope is restored
    /// on return, error, or unwinding. This is deliberately thread-local: callers
    /// must pass the deadline explicitly to workers or asynchronous execution.
    /// It checks before and after the closure; it cannot preempt filesystem or
    /// CPU work that does not checkpoint. The hosted job remains the outer cap.
    /// # Errors
    /// Rejects entry or completion after the absolute budget expires.
    pub fn scope<T>(self, operation: impl FnOnce() -> T) -> Result<T, MiseError> {
        let effective = self.effective();
        effective.remaining()?;
        let restore = RestoreDeadline(OPERATION_DEADLINE.replace(Some(effective)));
        let result = operation();
        effective.remaining()?;
        drop(restore);
        Ok(result)
    }

    /// Intersect a subprocess budget with the synchronous operation's budget.
    #[must_use]
    pub(crate) fn effective(self) -> Self {
        OPERATION_DEADLINE
            .get()
            .map_or(self, |parent| Self(self.0.min(parent.0)))
    }

    /// Check the enclosing synchronous operation, if one is active.
    /// # Errors
    /// Returns the ordinary deadline error when the operation has expired.
    pub fn check_scope() -> Result<(), MiseError> {
        OPERATION_DEADLINE
            .get()
            .map_or(Ok(()), |deadline| deadline.remaining().map(|_| ()))
    }

    /// Derive the check deadline from the instant its execution boundary began.
    /// # Errors
    /// Returns a timeout error when the absolute instant overflows.
    pub fn from_start(start: Instant, timeout: Duration) -> Result<Self, MiseError> {
        start
            .checked_add(timeout)
            .map(Self)
            .ok_or_else(|| invalid("check_timeout", "deadline_overflow"))
    }

    /// Reserve the subprocess runner's cleanup grace inside an operation budget.
    ///
    /// This work deadline leaves the existing process-group termination and
    /// bounded reap grace within the caller's total operational budget. It is
    /// not a physical preemption guarantee for kernel or non-checkpointed work.
    /// # Errors
    /// Rejects a budget with no work time left after cleanup, or instant overflow.
    pub fn from_start_with_cleanup_reserve(
        start: Instant,
        budget: Duration,
    ) -> Result<Self, MiseError> {
        let work = budget
            .checked_sub(crate::IsolatedCommand::PROCESS_CLEANUP_GRACE)
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| invalid("check_timeout", "insufficient_cleanup_budget"))?;
        Self::from_start(start, work)
    }

    /// Start a deadline from the check's admitted timeout.
    /// # Errors
    /// Returns a timeout error when the absolute instant overflows.
    pub fn after(timeout: Duration) -> Result<Self, MiseError> {
        Self::from_start(Instant::now(), timeout)
    }

    /// Return the remaining budget or fail once the shared deadline expires.
    /// # Errors
    /// Returns a check timeout error when no time remains.
    pub fn remaining(self) -> Result<Duration, MiseError> {
        self.effective()
            .0
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| invalid("check_timeout", "deadline_exhausted"))
    }
}

#[cfg(test)]
#[path = "check_deadline_scope_tests.rs"]
mod scope_tests;

#[cfg(test)]
mod tests {
    use super::CheckDeadline;
    use crate::IsolatedCommand;
    use std::ffi::OsString;
    use std::time::Duration;

    #[test]
    fn one_deadline_limits_a_later_probe_after_preparation_time_is_used() {
        let deadline = CheckDeadline::after(Duration::from_millis(150)).expect("deadline");
        std::thread::sleep(Duration::from_millis(80));
        let command = IsolatedCommand::qualified_check_probe(
            OsString::from("/bin/sleep"),
            vec![OsString::from("1")],
            Vec::new(),
        );
        let result = command.run_until(1024, deadline);
        assert!(result.is_err());
        assert!(deadline.remaining().is_err());
    }

    #[test]
    fn deadline_from_operation_start_includes_elapsed_setup_time() {
        let started = std::time::Instant::now()
            .checked_sub(Duration::from_millis(20))
            .expect("past instant");
        let deadline =
            CheckDeadline::from_start(started, Duration::from_millis(10)).expect("deadline");
        assert!(deadline.remaining().is_err());
    }
}
