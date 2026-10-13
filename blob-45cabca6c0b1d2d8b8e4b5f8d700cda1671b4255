//! Redacted errors. Display text never carries secrets or JIT material.

/// Identifier construction failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    /// Zero is not a worker, grant, or intent id.
    #[error("zero id")]
    Zero,
    /// Request ids from the service are non-negative.
    #[error("negative id")]
    Negative,
}

/// Lifecycle and capacity failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    /// The event epoch is not the worker epoch.
    #[error("stale epoch")]
    StaleEpoch,
    /// Occupancy is already at the configured maximum.
    #[error("capacity exhausted")]
    CapacityExhausted,
    /// This acquire intent already holds a grant.
    #[error("duplicate acquire")]
    DuplicateAcquire,
    /// The event is not legal for the current state.
    #[error("illegal transition")]
    IllegalTransition,
    /// Cleanup proof does not cover the owned ids.
    #[error("cleanup proof mismatch")]
    CleanupMismatch,
    /// The worker is not in this authority.
    #[error("unknown worker")]
    UnknownWorker,
}

/// Fail-closed parity. [`EvidenceError::NotProven`] is not success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    /// Two reports share one execution key.
    #[error("duplicate execution")]
    DuplicateExecution,
    /// A required check did not produce proof.
    #[error("not proven: {0}")]
    NotProven(&'static str),
    /// Observed reports do not cover the expected set.
    #[error("incomplete execution set")]
    IncompleteExecutionSet,
    /// An expected key has no report.
    #[error("missing execution")]
    MissingExecution,
}
