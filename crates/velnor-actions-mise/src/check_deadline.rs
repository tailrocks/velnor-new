//! One monotonic deadline shared by every phase of a qualified named check.
use crate::MiseError;
use crate::checks::invalid;
use std::time::{Duration, Instant};

/// Absolute timeout boundary for preparation, probes, acquisition and task execution.
#[derive(Debug, Clone, Copy)]
pub struct CheckDeadline(Instant);

impl CheckDeadline {
    /// Start a deadline from the check's admitted timeout.
    #[must_use]
    pub fn after(timeout: Duration) -> Result<Self, MiseError> {
        Instant::now()
            .checked_add(timeout)
            .map(Self)
            .ok_or_else(|| invalid("check_timeout", "deadline_overflow"))
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
        let result = command.run_bounded(1024, deadline.remaining().expect("remaining budget"));
        assert!(result.is_err());
        assert!(deadline.remaining().is_err());
    }
}
