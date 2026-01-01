//! Wire failures. Bodies that may contain JIT are not copied into Display.

/// Protocol or transport failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// JSON did not match the pinned DTO.
    #[error("malformed message")]
    Malformed,
    /// Envelope `messageType` was not `RunnerScaleSetJobMessages`.
    #[error("unsupported envelope")]
    UnsupportedEnvelope,
    /// Returned ids were not a subset of the request.
    #[error("acquire id outside request")]
    OutsideRequest,
    /// A second 401 refresh is not attempted.
    #[error("refresh exhausted")]
    RefreshExhausted,
    /// HTTP 403 is terminal.
    #[error("forbidden")]
    Forbidden,
    /// Status was not one of the pinned outcomes.
    #[error("unexpected status")]
    UnexpectedStatus,
    /// The scale set failed a registration invariant.
    #[error("registration rejected")]
    RegistrationRejected,
    /// JSON encoding failed.
    #[error("encode failed")]
    Encode,
    /// A mutex was poisoned.
    #[error("lock poisoned")]
    Poisoned,
}
