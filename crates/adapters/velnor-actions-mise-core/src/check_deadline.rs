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
mod tests;
