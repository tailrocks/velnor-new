#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    #[error("malformed message")]
    Malformed,

    #[error("unsupported envelope")]
    UnsupportedEnvelope,

    #[error("acquire id outside request")]
    OutsideRequest,

    #[error("multiple runners found")]
    MultipleResults,

    #[error("refresh exhausted")]
    RefreshExhausted,

    #[error("forbidden")]
    Forbidden,

    #[error("unexpected status")]
    UnexpectedStatus,

    #[error("registration rejected")]
    RegistrationRejected,

    #[error("encode failed")]
    Encode,

    #[error("lock poisoned")]
    Poisoned,
}
