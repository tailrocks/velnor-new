#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdError {
    #[error("zero id")]
    Zero,

    #[error("negative id")]
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StateError {
    #[error("stale epoch")]
    StaleEpoch,

    #[error("capacity exhausted")]
    CapacityExhausted,

    #[error("duplicate acquire")]
    DuplicateAcquire,

    #[error("illegal transition")]
    IllegalTransition,

    #[error("cleanup proof mismatch")]
    CleanupMismatch,

    #[error("unknown worker")]
    UnknownWorker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EvidenceError {
    #[error("duplicate execution")]
    DuplicateExecution,

    #[error("not proven: {0}")]
    NotProven(&'static str),

    #[error("incomplete execution set")]
    IncompleteExecutionSet,

    #[error("missing execution")]
    MissingExecution,
}
