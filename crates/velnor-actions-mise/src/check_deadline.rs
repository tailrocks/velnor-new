//! One monotonic deadline shared by every phase of a qualified named check.
use crate::MiseError;
use crate::checks::invalid;
use std::time::{Duration, Instant};

/// Absolute timeout boundary for preparation, probes, acquisition and task execution.
#[derive(Debug, Clone, Copy)]
pub struct CheckDeadline(Instant);

impl CheckDeadline {
    /// Derive the check deadline from the instant its execution boundary began.
    /// # Errors
    /// Returns a timeout error when the absolute instant overflows.
    pub fn from_start(start: Instant, timeout: Duration) -> Result<Self, MiseError> {
        start
            .checked_add(timeout)
            .map(Self)
            .ok_or_else(|| invalid("check_timeout", "deadline_overflow"))
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
        self.0
            .checked_duration_since(Instant::now())
            .filter(|remaining| !remaining.is_zero())
            .ok_or_else(|| invalid("check_timeout", "deadline_exhausted"))
    }
}

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
