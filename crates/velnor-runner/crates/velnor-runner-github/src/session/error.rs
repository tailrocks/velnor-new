use crate::refresh::StatusClass;
use crate::{Certainty, WireError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[must_use]
pub enum SessionError {
    #[error(transparent)]
    Wire(#[from] WireError),

    #[error("session conflict")]
    Conflict,

    #[error("effect uncertain")]
    Uncertain,
}

impl SessionError {
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
