//! Non-secret identities and durable states for exact-session refresh attempts.

use crate::config::ConnectionId;

/// Durable result for one same-ID credential refresh request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionRefreshState {
    /// The PATCH claim was committed before sending the request.
    Requested,
    /// A validated response returned the exact bound session ID.
    Succeeded,
    /// The caller received the pinned client's retryable transport error.
    TransportError,
    /// Startup proved the previous local request belongs to an abandoned owner.
    AbandonedUncertain,
    /// The result cannot authorize another request or session deletion.
    Uncertain,
}

impl SessionRefreshState {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Succeeded => "succeeded",
            Self::TransportError => "transport_error",
            Self::AbandonedUncertain => "abandoned_uncertain",
            Self::Uncertain => "uncertain",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, crate::error::HostError> {
        match value {
            "requested" => Ok(Self::Requested),
            "succeeded" => Ok(Self::Succeeded),
            "transport_error" => Ok(Self::TransportError),
            "abandoned_uncertain" => Ok(Self::AbandonedUncertain),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(crate::error::HostError::Journal),
        }
    }

    pub(super) const fn permits_next(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::TransportError | Self::AbandonedUncertain
        )
    }
}

/// Read-only attempt record; fields contain no credential or endpoint data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionRefreshRecord {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) session_intent_id: i64,
    pub(super) session_id: String,
    pub(super) generation: i64,
    pub(super) state: SessionRefreshState,
}

#[cfg(test)]
impl SessionRefreshRecord {
    /// Give the exact session identifier.
    #[must_use]
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Give the monotone local generation.
    #[must_use]
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }

    /// Give the recorded outcome without authorizing a remote effect.
    #[must_use]
    pub(crate) const fn state(&self) -> SessionRefreshState {
        self.state
    }
}

/// One-shot authority to record the result of one claimed PATCH.
#[must_use = "a claimed refresh must be recorded or left for startup recovery"]
pub(crate) struct SessionRefreshClaim {
    pub(super) connection_id: ConnectionId,
    pub(super) scale_set_id: i64,
    pub(super) session_intent_id: i64,
    pub(super) session_id: String,
    pub(super) generation: i64,
}

impl SessionRefreshClaim {
    pub(super) fn new(
        connection_id: ConnectionId,
        scale_set_id: i64,
        session_intent_id: i64,
        session_id: String,
        generation: i64,
    ) -> Self {
        Self {
            connection_id,
            scale_set_id,
            session_intent_id,
            session_id,
            generation,
        }
    }

    /// Give the durable session intent bound to this claim.
    #[must_use]
    pub(crate) const fn session_intent_id(&self) -> i64 {
        self.session_intent_id
    }

    /// Give the selected Scale Set bound to this claim.
    #[must_use]
    pub(crate) const fn scale_set_id(&self) -> i64 {
        self.scale_set_id
    }

    /// Give the exact session ID bound to this claim.
    #[must_use]
    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Give the local generation committed before the PATCH.
    #[must_use]
    pub(crate) const fn generation(&self) -> i64 {
        self.generation
    }
}
