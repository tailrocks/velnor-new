//! Session failures. Response bodies are not copied into `Display`.

use crate::refresh::StatusClass;
use crate::{Certainty, WireError};

/// A session call failed, or its effect is not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[must_use]
pub enum SessionError {
    /// The exchange finished and the outcome is not success.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// HTTP 409. Do not delete the other session.
    #[error("session conflict")]
    Conflict,
    /// Timeout or reset. Not a definite failure.
    #[error("effect uncertain")]
    Uncertain,
}

impl SessionError {
    /// Timeout and reset are uncertain. Every other error is definite.
    #[must_use]
    pub const fn certainty(self) -> Certainty {
        match self {
            Self::Uncertain => Certainty::Uncertain,
            Self::Wire(_) | Self::Conflict => Certainty::Definite,
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
