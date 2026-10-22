//! Typed records for one-shot session creation.

use crate::config::ConnectionId;
use crate::journal::capacity_header::HeaderAllocation;

const NONCE_BYTES: usize = 32;

/// Durable progress for one exact session-create request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionCreateState {
    /// Stored before any session-create request.
    Prepared,
    /// The one-shot request claim was committed.
    Requested,
    /// The request may have created an unobservable session.
    Uncertain,
    /// The exact returned session ID was stored.
    Bound,
    /// The exact session deletion and header retirement were recorded.
    Retired,
}

impl SessionCreateState {
    pub(super) fn parse(value: &str) -> Result<Self, crate::error::HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "requested" => Ok(Self::Requested),
            "uncertain" => Ok(Self::Uncertain),
            "bound" => Ok(Self::Bound),
            "retired" => Ok(Self::Retired),
            _ => Err(crate::error::HostError::Journal),
        }
    }
}

/// Read-only view of one durable session-create attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionCreateRecord {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) local_nonce: String,
    pub(super) state: SessionCreateState,
    pub(super) session_id: Option<String>,
    pub(super) intent_id: Option<i64>,
}

/// Exact session row and header allocation written with the create response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BoundConnectionSession {
    pub(super) intent_id: i64,
    pub(super) allocation: HeaderAllocation,
}

impl BoundConnectionSession {
    /// Give the durable session-intent row.
    #[must_use]
    pub(crate) const fn intent_id(&self) -> i64 {
        self.intent_id
    }

    /// Give the header allocation committed with the returned session ID.
    #[must_use]
    pub(crate) const fn allocation(&self) -> &HeaderAllocation {
        &self.allocation
    }
}

impl SessionCreateRecord {
    /// Give the exact Scale Set ID.
    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Give the local operation nonce.
    #[must_use]
    pub(crate) fn local_nonce(&self) -> &str {
        &self.local_nonce
    }

    /// Give the durable state without authorizing a remote effect.
    #[must_use]
    pub(crate) const fn state(&self) -> SessionCreateState {
        self.state
    }

    /// Give the exact bound remote session ID, when the create response was recorded.
    #[must_use]
    pub(crate) fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Give the scoped session-intent row, when the create response was recorded.
    #[must_use]
    pub(crate) const fn intent_id(&self) -> Option<i64> {
        self.intent_id
    }
}

/// One-shot authority to send a session-create request.
#[must_use = "a claimed session create must be resolved or kept uncertain"]
pub(crate) struct SessionCreateClaim {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) local_nonce: String,
}

impl SessionCreateClaim {
    pub(super) fn new(connection_id: ConnectionId, scale_set_id: i64, local_nonce: String) -> Self {
        Self {
            connection_id,
            scale_set_id,
            local_nonce,
        }
    }
}

pub(super) fn validate_nonce(value: &str) -> Result<(), crate::error::HostError> {
    if value.len() != NONCE_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(crate::error::HostError::Journal);
    }
    Ok(())
}

pub(super) fn validate_session_id(value: &str) -> Result<(), crate::error::HostError> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(crate::error::HostError::Journal);
    }
    Ok(())
}
