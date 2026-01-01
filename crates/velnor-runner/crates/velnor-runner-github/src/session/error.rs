//! Session failures. Response bodies are not copied into `Display`.

use crate::refresh::StatusClass;
use crate::{Certainty, WireError};

/// A session call failed, or its effect is not known from the response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[must_use]
pub enum SessionError {
    /// The exchange finished and the outcome is not success.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// HTTP 409. Do not delete the other session.
    #[error("session conflict")]
    Conflict,
    /// Unknown or unusable remote outcome. Not a definite failure.
    #[error("effect uncertain")]
    Uncertain,
}

impl SessionError {
    /// Only local rejection and explicit service rejection are definite.
    #[must_use]
    pub const fn certainty(self) -> Certainty {
        match self {
            Self::Conflict
            | Self::Wire(
                WireError::Encode
                | WireError::Forbidden
                | WireError::RefreshExhausted
                | WireError::RegistrationRejected,
            ) => Certainty::Definite,
            _ => Certainty::Uncertain,
        }
    }
}

pub(crate) const fn reject(class: StatusClass) -> SessionError {
    match class {
        StatusClass::SessionConflict => SessionError::Conflict,
        StatusClass::Ok
        | StatusClass::EmptyPoll
        | StatusClass::Acked
        | StatusClass::RefreshOnce => SessionError::Wire(WireError::UnexpectedStatus),
    }
}
